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
 * #630 评论 5307423953 Part B：状态来源改为全局 [SyncRepository.loadSyncConfig] +
 * [SyncRepository.getSyncCapability] + [SyncRepository.loadFullSyncState]（全量同步
 * 持久状态）。不再读 [SyncRepository.loadAppSyncState] 拿 App target 冒充总体状态 —
 * 单 target 失败后 App target 自己的 state 可能仍是 Success，会误把全局灯刷绿。
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
                            // #630 评论 5307423953 Part B：只看 FullSyncState.overallStatus
                            // 决定灯色，不再看 loadAppSyncState()。
                            val fullState = settingsRepository.loadFullSyncState()
                            if (fullState == null) {
                                // 从未同步过 — 已配置但无全量状态记录
                                SyncIndicatorState.Unconfigured
                            } else {
                                when (fullState.overallStatus) {
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
                }
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (_: Exception) {
                _state.value
            }
        _state.value = indicatorState
    }
}
