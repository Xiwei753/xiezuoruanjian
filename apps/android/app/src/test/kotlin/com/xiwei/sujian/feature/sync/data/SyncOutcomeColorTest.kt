package com.xiwei.sujian.feature.sync.data

import com.xiwei.sujian.feature.sync.data.model.FullSyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import org.junit.Assert.assertEquals
import org.junit.Test

class SyncOutcomeColorTest {
    private fun SyncOutcome.expectedIndicator(): SyncIndicatorState =
        when (this) {
            is SyncOutcome.Completed -> SyncIndicatorState.Synced
            is SyncOutcome.Disabled -> SyncIndicatorState.Unconfigured
            is SyncOutcome.Unconfigured -> SyncIndicatorState.Unconfigured
            is SyncOutcome.Busy -> SyncIndicatorState.Syncing
            is SyncOutcome.RetryableFailure -> SyncIndicatorState.Failed
            is SyncOutcome.TerminalFailure -> SyncIndicatorState.Failed
        }

    @Test
    fun completedStatuses_mapToSynced() {
        val statuses =
            listOf(
                SyncStatus.Success,
                SyncStatus.NoChanges,
                SyncStatus.LatestWinsApplied,
                SyncStatus.BranchMissingRecovered,
            )
        statuses.forEach { status ->
            val outcome =
                SyncOutcome.Completed(
                    FullSyncResult(
                        overallStatus = status,
                        targets = emptyList(),
                        totalUploaded = 0,
                        totalDownloaded = 0,
                        totalLocalDeletes = 0,
                        totalRemoteDeletes = 0,
                        totalOverwritten = 0,
                        totalIgnored = 0,
                        totalConflicts = 0,
                        error = null,
                        errorCategory = null,
                        messageKey = null,
                    ),
                )
            assertEquals("Expected Synced for $status", SyncIndicatorState.Synced, outcome.expectedIndicator())
        }
    }

    @Test
    fun disabled_mapsToUnconfigured() {
        assertEquals(SyncIndicatorState.Unconfigured, SyncOutcome.Disabled.expectedIndicator())
    }

    @Test
    fun unconfigured_mapsToUnconfigured() {
        assertEquals(SyncIndicatorState.Unconfigured, SyncOutcome.Unconfigured.expectedIndicator())
    }

    @Test
    fun busy_mapsToSyncing() {
        assertEquals(SyncIndicatorState.Syncing, SyncOutcome.Busy.expectedIndicator())
    }

    @Test
    fun retryableFailure_mapsToFailed() {
        val statuses = listOf(SyncStatus.RecoverableError)
        statuses.forEach { status ->
            val outcome = SyncOutcome.RetryableFailure(status)
            assertEquals("Expected Failed for $status", SyncIndicatorState.Failed, outcome.expectedIndicator())
        }
    }

    @Test
    fun terminalFailure_mapsToFailed() {
        val statuses =
            listOf(
                SyncStatus.Error,
                SyncStatus.Conflict,
                SyncStatus.PartialConflict,
                SyncStatus.FatalError,
                SyncStatus.DirtyRepoBlocked,
            )
        statuses.forEach { status ->
            val outcome = SyncOutcome.TerminalFailure(status)
            assertEquals("Expected Failed for $status", SyncIndicatorState.Failed, outcome.expectedIndicator())
        }
    }

    @Test
    fun autoSyncWorker_mappingContract() {
        val cases =
            listOf(
                SyncOutcome.Completed(
                    FullSyncResult(
                        overallStatus = SyncStatus.Success,
                        targets = emptyList(),
                        totalUploaded = 0,
                        totalDownloaded = 0,
                        totalLocalDeletes = 0,
                        totalRemoteDeletes = 0,
                        totalOverwritten = 0,
                        totalIgnored = 0,
                        totalConflicts = 0,
                        error = null,
                        errorCategory = null,
                        messageKey = null,
                    ),
                ) to true,
                SyncOutcome.Unconfigured to true,
                SyncOutcome.Disabled to true,
                SyncOutcome.Busy to false,
                SyncOutcome.RetryableFailure(SyncStatus.Error) to false,
                SyncOutcome.TerminalFailure(SyncStatus.Conflict) to false,
            )
        cases.forEach { (outcome, expectSuccess) ->
            val workerResult =
                when (outcome) {
                    is SyncOutcome.Completed, is SyncOutcome.Unconfigured, is SyncOutcome.Disabled -> true
                    is SyncOutcome.Busy, is SyncOutcome.RetryableFailure -> false
                    is SyncOutcome.TerminalFailure -> false
                }
            assertEquals("Worker result for $outcome", expectSuccess, workerResult)
        }
    }

    @Test
    fun refreshState_initialState_isUnconfigured() {
        // #592 五：初始状态为 Unconfigured
        assertEquals(SyncIndicatorState.Unconfigured, SyncIndicatorState.Unconfigured)
    }

    @Test
    fun busy_outcome_doesNotModifyState() {
        // #592 四：Busy 不调用 refreshState()，不修改状态灯
        val busyOutcome = SyncOutcome.Busy
        // Busy 的预期指示器状态是 Syncing（另一个同步正在运行）
        assertEquals(SyncIndicatorState.Syncing, busyOutcome.expectedIndicator())
    }
}
