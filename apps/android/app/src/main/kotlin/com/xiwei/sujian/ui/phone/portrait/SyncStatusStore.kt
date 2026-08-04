package com.xiwei.sujian.ui.phone.portrait

import android.content.Context
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SyncSession
import com.xiwei.sujian.model.SyncConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

class SyncStatusStore(
    private val settingsRepository: SettingsRepository,
) {
    private val _state = MutableStateFlow(SyncIndicatorState.Unconfigured)
    val state: StateFlow<SyncIndicatorState> = _state.asStateFlow()

    suspend fun refreshState() {
        val indicatorState = withContext(Dispatchers.IO) {
            val config = settingsRepository.loadSyncConfig()
            val capability = settingsRepository.getSyncCapability()
            when {
                config.enabled != true -> SyncIndicatorState.Unconfigured
                !capability.canRun -> SyncIndicatorState.Unconfigured
                else -> {
                    val syncState = settingsRepository.loadSyncState()
                    when (syncState.status) {
                        com.xiwei.sujian.model.SyncStatus.Syncing -> SyncIndicatorState.Syncing
                        com.xiwei.sujian.model.SyncStatus.Success,
                        com.xiwei.sujian.model.SyncStatus.NoChanges,
                        com.xiwei.sujian.model.SyncStatus.LatestWinsApplied,
                        com.xiwei.sujian.model.SyncStatus.BranchMissingRecovered -> SyncIndicatorState.Synced
                        com.xiwei.sujian.model.SyncStatus.Conflict,
                        com.xiwei.sujian.model.SyncStatus.PartialConflict,
                        com.xiwei.sujian.model.SyncStatus.RecoverableError,
                        com.xiwei.sujian.model.SyncStatus.FatalError,
                        com.xiwei.sujian.model.SyncStatus.DirtyRepoBlocked,
                        com.xiwei.sujian.model.SyncStatus.Error -> SyncIndicatorState.Failed
                        com.xiwei.sujian.model.SyncStatus.Idle,
                        com.xiwei.sujian.model.SyncStatus.ConfiguredNotTested -> SyncIndicatorState.Unconfigured
                    }
                }
            }
        }
        _state.value = indicatorState
    }

    suspend fun manualSync() {
        val config = settingsRepository.loadSyncConfig()
        if (config.enabled != true) return
        _state.value = SyncIndicatorState.Syncing
        val result = SyncSession.runExclusive { _ ->
            withContext(Dispatchers.IO) {
                settingsRepository.performSync(config)
            }
        }
        refreshState()
    }
}
