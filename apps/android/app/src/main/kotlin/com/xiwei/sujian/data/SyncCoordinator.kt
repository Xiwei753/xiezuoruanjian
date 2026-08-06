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
    /**
     * #592 三：携带具体 [SyncFailureKind]，使正式同步、试运行、连接诊断
     * 全部通过 kind.messageKey() 获得同一用户提示映射。
     */
    data class RetryableFailure(
        val status: SyncStatus,
        val kind: SyncFailureKind = SyncFailureKind.fromSyncStatus(status),
    ) : SyncOutcome()
    data class TerminalFailure(
        val status: SyncStatus,
        val kind: SyncFailureKind = SyncFailureKind.fromSyncStatus(status),
    ) : SyncOutcome()
}

class SyncCoordinator(
    private val settingsRepository: SettingsRepository,
    private val syncStatusRepository: SyncStatusRepository,
) {
    /**
     * #592 四：统一异常边界 — 每条路径必须结束在明确终态。
     *
     * #592 六：一次同步操作只使用同一份不可变 SyncProfileSnapshot
     * （generation + config + secrets）。调用方（AutoSyncWorker 等）可传入
     * 预先取得的 snapshot，避免二次读取；未传入时在锁内取一次完整快照。
     * snapshot 的 secrets 通过进程级 override 注入 Rust，整个操作不再从磁盘
     * 二次读取 config/secrets。
     *
     * 所有失败路径通过 [SyncFailureKind] 唯一分类：
     * - CancellationException → 原样抛出
     * - RetryableNetwork / RetryableIo → RetryableFailure，红色
     * - Authentication / Conflict / DirtyRepository / Protocol / NativeUnavailable / Fatal → TerminalFailure，红色
     * - 未配置或关闭 → Unconfigured/Disabled，灰色
     * - performSync 返回 Syncing → 协议错误，TerminalFailure
     *
     * BridgeResult.NotLoaded 与 errorCode "NATIVE_NOT_LOADED" 统一进入 NativeUnavailable，
     * 不再分别维护字符串白名单和独立分支。
     */
    suspend fun runSync(trigger: SyncTrigger, snapshot: SyncProfileSnapshot? = null): SyncOutcome {
        DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "start")
        try {
            val profile: SyncProfileSnapshot = snapshot ?: run {
                val result = withContext(Dispatchers.IO) {
                    SyncProfileGate.snapshotExclusive { settingsRepository.snapshotSyncProfile() }
                }
                when (result) {
                    is SyncProfileReadResult.Found -> result.snapshot
                    is SyncProfileReadResult.NotConfigured -> result.snapshot
                    is SyncProfileReadResult.Failed -> {
                        DiagnosticsLogger.w("SyncCoordinator", "Sync profile snapshot failed: ${result.message}")
                        syncStatusRepository.notifySyncFailed()
                        return result.kind.toOutcome()
                    }
                }
            }
            val config = profile.config
            if (config.enabled != true) {
                syncStatusRepository.notifyUnconfigured()
                return SyncOutcome.Unconfigured
            }
            val capability = withContext(Dispatchers.IO) { settingsRepository.getSyncCapability() }
            if (!capability.canRun) {
                syncStatusRepository.notifyUnconfigured()
                return SyncOutcome.Disabled
            }

            // #592 六：整个操作只使用这份 snapshot 的凭据。
            // #595 十：override 必须在取得同步独占锁（runExclusive）之后写入，
            // 两个同步同时触发时不可能出现“A 写入 token A、B 写入 token B、
            // A 实际使用 token B”；设置失败立即终止（不静默继续）；
            // 操作结束后 finally 清除 override，陈旧凭据不得泄漏到后续操作
            // （Core 的 refresh_secrets_override 在已有 override 时不再读磁盘）。
            // #595 三：活动正文 flush 与同步执行必须在同一独占锁内串行 —
            // flush 保存磁盘版本后同步立即以该版本为 base；如果 flush 在锁外，
            // 两个同步触发可交叉 flush/执行，正文版本屏障失效。
            val exclusiveResult = SyncSession.runExclusive { _ ->
                val flushOk = WorkspaceDocumentGate.flushActiveDocument()
                if (!flushOk) {
                    DiagnosticsLogger.w("SyncCoordinator", "Active document flush failed before sync — aborting (typed DocumentSaveFailed)")
                    syncStatusRepository.notifySyncFailed()
                    return@runExclusive BridgeResult.Error(
                        ResultEnvelope.errorOf("DOCUMENT_FLUSH_FAILED", "Active document could not be persisted before sync"),
                        SyncFailureKind.DocumentSaveFailed,
                    )
                }
                syncStatusRepository.notifySyncStarted()
                val overrideOk = withContext(Dispatchers.IO) {
                    settingsRepository.setSyncSecretsOverrideStrict(profile.secrets)
                }
                if (!overrideOk) {
                    val error = BridgeResult.Error(
                        ResultEnvelope.errorOf("SYNC_CREDENTIALS_OVERRIDE_FAILED", "Failed to set sync credentials override"),
                        SyncFailureKind.Fatal,
                    )
                    resolveAndPublish(error)
                    return@runExclusive error
                }
                try {
                    val bridgeResult = withContext(Dispatchers.IO) { settingsRepository.performSync(config) }
                    resolveAndPublish(bridgeResult)
                    bridgeResult
                } finally {
                    withContext(Dispatchers.IO) { settingsRepository.clearSyncSecretsOverride() }
                }
            }

            return when (exclusiveResult) {
                is ExclusiveResult.Busy -> {
                    SyncOutcome.Busy
                }
                is ExclusiveResult.Success -> {
                    when (val br = exclusiveResult.value) {
                        is BridgeResult.Success -> mapToOutcome(br.data)
                        is BridgeResult.Error -> classifyFailure(br).toOutcome()
                        BridgeResult.NotLoaded -> {
                            DiagnosticsLogger.w("SyncCoordinator", "Native library not loaded — NativeUnavailable")
                            SyncFailureKind.NativeUnavailable.toOutcome()
                        }
                    }
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: IOException) {
            DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "io_exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return SyncFailureKind.RetryableIo.toOutcome()
        } catch (e: RepositoryException) {
            DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "repository_exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return e.kind.toOutcome()
        } catch (e: Exception) {
            DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return SyncFailureKind.Fatal.toOutcome()
        }
    }

    /**
     * #592 七：类型化失败直接来自 Bridge 边界（WriterException 变体），
     * 不再维护 Android 字符串错误码表；未知错误默认 Fatal。
     */
    internal fun classifyFailure(error: BridgeResult.Error): SyncFailureKind =
        SyncFailureKind.fromBridgeError(error)



    private fun mapToOutcome(result: SyncResult): SyncOutcome = when (result.status) {
        SyncStatus.Success,
        SyncStatus.NoChanges,
        SyncStatus.LatestWinsApplied,
        SyncStatus.BranchMissingRecovered -> SyncOutcome.Completed(result)

        SyncStatus.RecoverableError -> SyncOutcome.RetryableFailure(result.status, SyncFailureKind.fromSyncStatus(result.status))

        SyncStatus.Error,
        SyncStatus.Conflict,
        SyncStatus.PartialConflict,
        SyncStatus.FatalError,
        SyncStatus.DirtyRepoBlocked -> SyncOutcome.TerminalFailure(result.status, SyncFailureKind.fromSyncStatus(result.status))

        SyncStatus.Syncing -> {
            DiagnosticsLogger.w("SyncCoordinator", "performSync returned Syncing — protocol error, mapping to terminal failure")
            SyncOutcome.TerminalFailure(SyncStatus.FatalError, SyncFailureKind.Fatal)
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
}
