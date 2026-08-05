package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.model.SyncResult
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

    suspend fun manualSync() {
        val repo = settingsRepository ?: return
        val config = withContext(Dispatchers.IO) { repo.loadSyncConfig() }
        if (config.enabled != true) {
            _state.value = SyncIndicatorState.Unconfigured
            return
        }
        _state.value = SyncIndicatorState.Syncing
        val result = SyncSession.runExclusive { _ ->
            withContext(Dispatchers.IO) {
                repo.performSync(config)
            }
        }
        when (result) {
            is ExclusiveResult.Busy -> refreshState()
            is ExclusiveResult.Success -> resolveBridgeResult(result.value)
        }
    }

    private fun resolveBridgeResult(result: BridgeResult<SyncResult>) {
        when (result) {
            is BridgeResult.Success -> resolveSyncStatus(result.data.status)
            is BridgeResult.Error -> _state.value = SyncIndicatorState.Failed
            BridgeResult.NotLoaded -> _state.value = SyncIndicatorState.Failed
        }
    }

    private fun resolveSyncStatus(status: SyncStatus) {
        when (status) {
            SyncStatus.Success,
            SyncStatus.NoChanges,
            SyncStatus.LatestWinsApplied,
            SyncStatus.BranchMissingRecovered -> {
                _state.value = SyncIndicatorState.Synced
            }
            SyncStatus.Conflict,
            SyncStatus.PartialConflict,
            SyncStatus.RecoverableError,
            SyncStatus.FatalError,
            SyncStatus.DirtyRepoBlocked,
            SyncStatus.Error -> {
                _state.value = SyncIndicatorState.Failed
            }
            SyncStatus.Syncing -> {
                _state.value = SyncIndicatorState.Syncing
            }
            SyncStatus.Idle,
            SyncStatus.ConfiguredNotTested -> {
                _state.value = SyncIndicatorState.Unconfigured
            }
        }
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
