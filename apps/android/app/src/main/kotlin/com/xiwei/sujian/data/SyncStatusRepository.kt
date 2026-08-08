package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.model.SyncStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

class SyncStatusRepository(
    private val settingsRepository: SettingsRepository,
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

    suspend fun refreshState(projectId: String?) {
        if (projectId == null) {
            _state.value = SyncIndicatorState.Unconfigured
            return
        }
        val indicatorState =
            try {
                withContext(Dispatchers.IO) {
                    val config = settingsRepository.loadSyncConfig(projectId)
                    val capability = settingsRepository.getSyncCapability(projectId)
                    when {
                        config.enabled != true -> SyncIndicatorState.Unconfigured
                        !capability.canRun -> SyncIndicatorState.Unconfigured
                        else -> {
                            val syncState = settingsRepository.loadSyncState(projectId)
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
