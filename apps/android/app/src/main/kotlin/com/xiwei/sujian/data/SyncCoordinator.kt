package com.xiwei.sujian.data

import com.xiwei.sujian.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.model.SyncResult
import com.xiwei.sujian.model.SyncStatus
import com.xiwei.sujian.model.SyncTrigger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

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
                        is BridgeResult.Error -> SyncOutcome.RetryableFailure(SyncStatus.Error)
                        BridgeResult.NotLoaded -> SyncOutcome.RetryableFailure(SyncStatus.Error)
                    }
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Exception) {
            DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return SyncOutcome.RetryableFailure(SyncStatus.Error)
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
            DiagnosticsLogger.w("SyncCoordinator", "performSync returned Syncing — protocol error, mapping to failure")
            SyncOutcome.RetryableFailure(SyncStatus.Error)
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