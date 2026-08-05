package com.xiwei.sujian.data

import com.xiwei.sujian.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.model.SyncResult
import com.xiwei.sujian.model.SyncStatus
import com.xiwei.sujian.model.SyncTrigger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.IOException

sealed class SyncOutcome {
    data class Completed(val result: SyncResult) : SyncOutcome()
    data object Disabled : SyncOutcome()
    data object Unconfigured : SyncOutcome()
    data object Busy : SyncOutcome()
    data class RetryableFailure(val status: SyncStatus) : SyncOutcome()
    data class TerminalFailure(val status: SyncStatus) : SyncOutcome()
}

class SyncCoordinator(
    private val settingsRepository: SettingsRepository,
    private val syncStatusRepository: SyncStatusRepository,
) {
    /**
     * #592 三：统一异常边界 — 每条路径必须结束在明确终态。
     *
     * - CancellationException → 原样抛出
     * - 临时网络/IO 异常 → RetryableFailure，红色
     * - 配置、冲突、脏仓库、致命协议错误 → TerminalFailure，红色
     * - 未配置或关闭 → Unconfigured/Disabled，灰色
     * - performSync 返回 Syncing → 协议错误，TerminalFailure
     */
    suspend fun runSync(trigger: SyncTrigger): SyncOutcome {
        DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "start")
        try {
            val config = withContext(Dispatchers.IO) { settingsRepository.loadSyncConfig() }
            if (config.enabled != true) {
                syncStatusRepository.notifyUnconfigured()
                return SyncOutcome.Unconfigured
            }
            val capability = withContext(Dispatchers.IO) { settingsRepository.getSyncCapability() }
            if (!capability.canRun) {
                syncStatusRepository.notifyUnconfigured()
                return SyncOutcome.Disabled
            }

            val exclusiveResult = SyncSession.runExclusive { _ ->
                syncStatusRepository.notifySyncStarted()
                val bridgeResult = withContext(Dispatchers.IO) { settingsRepository.performSync(config) }
                resolveAndPublish(bridgeResult)
                bridgeResult
            }

            return when (exclusiveResult) {
                is ExclusiveResult.Busy -> {
                    syncStatusRepository.refreshState()
                    SyncOutcome.Busy
                }
                is ExclusiveResult.Success -> {
                    when (val br = exclusiveResult.value) {
                        is BridgeResult.Success -> mapToOutcome(br.data)
                        is BridgeResult.Error -> classifyBridgeError(br)
                        BridgeResult.NotLoaded -> {
                            // 原生库未加载是致命错误，不可重试
                            DiagnosticsLogger.w("SyncCoordinator", "Native library not loaded — terminal failure")
                            SyncOutcome.TerminalFailure(SyncStatus.FatalError)
                        }
                    }
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: IOException) {
            // 临时 IO/网络异常 → 可重试
            DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "io_exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return SyncOutcome.RetryableFailure(SyncStatus.RecoverableError)
        } catch (e: RepositoryException) {
            // 仓库层异常按配置/协议错误处理 → 不可重试
            DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "repository_exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        } catch (e: Exception) {
            // 其他未知异常 → 可重试（保守策略：可能是临时网络/系统问题）
            DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return SyncOutcome.RetryableFailure(SyncStatus.Error)
        }
    }

    /**
     * BridgeResult.Error 按 errorCode 分类：
     * - 网络相关/IO 相关/可重试 → RetryableFailure
     * - 认证/冲突/协议/配置错误 → TerminalFailure
     */
    private fun classifyBridgeError(error: BridgeResult.Error): SyncOutcome {
        val code = error.code
        val isRetryable = code in RETRYABLE_ERROR_CODES
        return if (isRetryable) {
            SyncOutcome.RetryableFailure(SyncStatus.RecoverableError)
        } else {
            SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        }
    }

    private fun mapToOutcome(result: SyncResult): SyncOutcome = when (result.status) {
        SyncStatus.Success,
        SyncStatus.NoChanges,
        SyncStatus.LatestWinsApplied,
        SyncStatus.BranchMissingRecovered -> SyncOutcome.Completed(result)

        SyncStatus.RecoverableError,
        SyncStatus.Error -> SyncOutcome.RetryableFailure(result.status)

        SyncStatus.Conflict,
        SyncStatus.PartialConflict,
        SyncStatus.FatalError,
        SyncStatus.DirtyRepoBlocked -> SyncOutcome.TerminalFailure(result.status)

        SyncStatus.Syncing -> {
            // performSync 返回 Syncing 是协议错误，不可重试
            DiagnosticsLogger.w("SyncCoordinator", "performSync returned Syncing — protocol error, mapping to terminal failure")
            SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        }
        SyncStatus.Idle,
        SyncStatus.ConfiguredNotTested -> SyncOutcome.Unconfigured
    }

    private fun resolveAndPublish(result: BridgeResult<SyncResult>) {
        when (result) {
            is BridgeResult.Success -> resolveSyncStatus(result.data.status)
            is BridgeResult.Error -> syncStatusRepository.notifySyncFailed()
            BridgeResult.NotLoaded -> syncStatusRepository.notifySyncFailed()
        }
    }

    private fun resolveSyncStatus(status: SyncStatus) {
        when (status) {
            SyncStatus.Success,
            SyncStatus.NoChanges,
            SyncStatus.LatestWinsApplied,
            SyncStatus.BranchMissingRecovered -> {
                syncStatusRepository.notifySyncSuccess()
            }
            SyncStatus.Conflict,
            SyncStatus.PartialConflict,
            SyncStatus.RecoverableError,
            SyncStatus.FatalError,
            SyncStatus.DirtyRepoBlocked,
            SyncStatus.Error -> {
                syncStatusRepository.notifySyncFailed()
            }
            SyncStatus.Syncing -> {
                DiagnosticsLogger.w("SyncCoordinator", "performSync returned Syncing — forcing to Failed")
                syncStatusRepository.notifySyncFailed()
            }
            SyncStatus.Idle,
            SyncStatus.ConfiguredNotTested -> {
                syncStatusRepository.notifyUnconfigured()
            }
        }
    }

    companion object {
        /** 可重试的 Bridge errorCode：网络不可用、限流、临时 IO 错误 */
        internal val RETRYABLE_ERROR_CODES = setOf(
            "SYNC_NETWORK_UNAVAILABLE",
            "SYNC_RATE_LIMITED",
            "IO_ERROR",
            "NATIVE_NOT_LOADED",
        )
    }
}
