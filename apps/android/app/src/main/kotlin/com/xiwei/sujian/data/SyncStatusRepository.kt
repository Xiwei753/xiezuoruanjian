package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.model.SyncStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

object SyncStatusRepository {
    private var settingsRepository: SettingsRepository? = null

    private val _state = MutableStateFlow(SyncIndicatorState.Unconfigured)
    val state: StateFlow<SyncIndicatorState> = _state.asStateFlow()

    fun initialize(settingsRepository: SettingsRepository) {
        this.settingsRepository = settingsRepository
    }

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
        val repo = settingsRepository ?: return
        val indicatorState = withContext(Dispatchers.IO) {
            val config = repo.loadSyncConfig()
            val capability = repo.getSyncCapability()
            when {
                config.enabled != true -> SyncIndicatorState.Unconfigured
                !capability.canRun -> SyncIndicatorState.Unconfigured
                else -> {
                    val syncState = repo.loadSyncState()
                    when (syncState.status) {
                        SyncStatus.Syncing -> SyncIndicatorState.Syncing
                        SyncStatus.Success,
                        SyncStatus.NoChanges,
                        SyncStatus.LatestWinsApplied,
                        SyncStatus.BranchMissingRecovered -> SyncIndicatorState.Synced
                        SyncStatus.Conflict,
                        SyncStatus.PartialConflict,
                        SyncStatus.RecoverableError,
                        SyncStatus.FatalError,
                        SyncStatus.DirtyRepoBlocked,
                        SyncStatus.Error -> SyncIndicatorState.Failed
                        SyncStatus.Idle,
                        SyncStatus.ConfiguredNotTested -> SyncIndicatorState.Unconfigured
                    }
                }
            }
        }
        _state.value = indicatorState
    }
}
