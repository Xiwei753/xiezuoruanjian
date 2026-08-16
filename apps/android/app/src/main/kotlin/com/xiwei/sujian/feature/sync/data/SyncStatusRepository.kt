package com.xiwei.sujian.feature.sync.data
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

/**
 * #630 评论 #1：全局同步状态指示器。
 *
 * 状态来源：全局 [SyncRepository.loadSyncConfig] + [SyncRepository.getSyncCapability] +
 * App target 的 [SyncRepository.loadAppSyncState]（全量同步的总体完成时间由 App target
 * 状态最贴近）。不再按 projectId 路由。
 */
class SyncStatusRepository(
    private val settingsRepository: SyncRepository,
) {
    private val _state = MutableStateFlow(SyncIndicatorState.Unconfigured)
    val state: StateFlow<SyncIndicatorState> = _state.asStateFlow()

    fun notifySyncStarted() {
        _state.value = SyncIndicatorState.Syncing
    }

    fun notifySyncSuccess() {
        _state.value = SyncIndicatorState.Synced
    }

    fun notifySyncFailed() {
        _state.value = SyncIndicatorState.Failed
    }

    fun notifyUnconfigured() {
        _state.value = SyncIndicatorState.Unconfigured
    }

    suspend fun refreshState() {
        val indicatorState =
            try {
                withContext(Dispatchers.IO) {
                    val config = settingsRepository.loadSyncConfig()
                    val capability = settingsRepository.getSyncCapability()
                    when {
                        config.enabled != true -> SyncIndicatorState.Unconfigured
                        !capability.canRun -> SyncIndicatorState.Unconfigured
                        else -> {
                            val syncState = settingsRepository.loadAppSyncState()
                            when (syncState.status) {
                                SyncStatus.Syncing -> SyncIndicatorState.Syncing
                                SyncStatus.Success,
                                SyncStatus.NoChanges,
                                SyncStatus.LatestWinsApplied,
                                SyncStatus.BranchMissingRecovered,
                                -> SyncIndicatorState.Synced
                                SyncStatus.Conflict,
                                SyncStatus.PartialConflict,
                                SyncStatus.RecoverableError,
                                SyncStatus.FatalError,
                                SyncStatus.DirtyRepoBlocked,
                                SyncStatus.Error,
                                -> SyncIndicatorState.Failed
                                SyncStatus.Idle,
                                SyncStatus.ConfiguredNotTested,
                                -> SyncIndicatorState.Unconfigured
                            }
                        }
                    }
                }
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (_: Exception) {
                _state.value
            }
        _state.value = indicatorState
    }
}
