package com.xiwei.sujian.data

import com.xiwei.sujian.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.model.SyncResult
import com.xiwei.sujian.model.SyncStatus
import com.xiwei.sujian.model.SyncTrigger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * 唯一同步协调器 — 所有同步入口统一收口。
 *
 * 固定流程：检查配置和能力 → 取得 SyncSession 执行权 → 立即发布 Syncing →
 * 执行同步 → 按真实 SyncStatus 发布 Synced 或 Failed → 持久化最终同步状态。
 *
 * 顶栏手动同步、AutoSyncWorker、前台服务同步、设置页同步和导入/拉取同步
 * 全部调用 [runSync]，不再自行切换状态。
 */
object SyncCoordinator {
    private var settingsRepository: SettingsRepository? = null

    fun initialize(settingsRepository: SettingsRepository) {
        this.settingsRepository = settingsRepository
    }

    /**
     * 统一同步主链。
     *
     * @return 同步结果；调用方为 AutoSyncWorker 等后台入口时，
     *   可用返回值判断 WorkManager Result。
     *   返回 null 表示未配置或仓库未初始化，调用方应视为无操作。
     */
    suspend fun runSync(trigger: SyncTrigger): SyncResult? {
        val repo = settingsRepository ?: return null
        val config = withContext(Dispatchers.IO) { repo.loadSyncConfig() }
        if (config.enabled != true) {
            SyncStatusRepository.notifyUnconfigured()
            return null
        }
        val capability = withContext(Dispatchers.IO) { repo.getSyncCapability() }
        if (!capability.canRun) {
            SyncStatusRepository.notifyUnconfigured()
            return null
        }

        val exclusiveResult = SyncSession.runExclusive { _ ->
            SyncStatusRepository.notifySyncStarted()
            val bridgeResult = withContext(Dispatchers.IO) { repo.performSync(config) }
            resolveAndPublish(bridgeResult)
            bridgeResult
        }

        return when (exclusiveResult) {
            is ExclusiveResult.Busy -> {
                SyncStatusRepository.refreshState()
                null
            }
            is ExclusiveResult.Success -> {
                when (val br = exclusiveResult.value) {
                    is BridgeResult.Success -> br.data
                    else -> null
                }
            }
        }
    }

    private fun resolveAndPublish(result: BridgeResult<SyncResult>) {
        when (result) {
            is BridgeResult.Success -> resolveSyncStatus(result.data.status)
            is BridgeResult.Error -> SyncStatusRepository.notifySyncFailed()
            BridgeResult.NotLoaded -> SyncStatusRepository.notifySyncFailed()
        }
    }

    private fun resolveSyncStatus(status: SyncStatus) {
        when (status) {
            SyncStatus.Success,
            SyncStatus.NoChanges,
            SyncStatus.LatestWinsApplied,
            SyncStatus.BranchMissingRecovered -> {
                SyncStatusRepository.notifySyncSuccess()
            }
            SyncStatus.Conflict,
            SyncStatus.PartialConflict,
            SyncStatus.RecoverableError,
            SyncStatus.FatalError,
            SyncStatus.DirtyRepoBlocked,
            SyncStatus.Error -> {
                SyncStatusRepository.notifySyncFailed()
            }
            SyncStatus.Syncing -> {
                SyncStatusRepository.notifySyncStarted()
            }
            SyncStatus.Idle,
            SyncStatus.ConfiguredNotTested -> {
                SyncStatusRepository.notifyUnconfigured()
            }
        }
    }
}
