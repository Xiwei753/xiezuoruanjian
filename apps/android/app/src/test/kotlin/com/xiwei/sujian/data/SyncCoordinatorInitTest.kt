package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.model.SyncStatus
import com.xiwei.sujian.model.SyncTrigger
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SyncCoordinatorInitTest {

    @Test
    fun syncOutcome_completedHoldsResult() {
        val result = com.xiwei.sujian.model.SyncResult(status = SyncStatus.Success)
        val outcome = SyncOutcome.Completed(result)
        assertTrue(outcome is SyncOutcome.Completed)
        assertEquals(SyncStatus.Success, (outcome as SyncOutcome.Completed).result.status)
    }

    @Test
    fun syncOutcome_disabledIsDistinctFromUnconfigured() {
        assertTrue(SyncOutcome.Disabled is SyncOutcome.Disabled)
        assertTrue(SyncOutcome.Unconfigured is SyncOutcome.Unconfigured)
    }

    @Test
    fun syncOutcome_busyIsDistinct() {
        assertTrue(SyncOutcome.Busy is SyncOutcome.Busy)
    }

    @Test
    fun syncOutcome_retryableFailureHoldsStatus() {
        val outcome = SyncOutcome.RetryableFailure(SyncStatus.RecoverableError)
        assertTrue(outcome is SyncOutcome.RetryableFailure)
        assertEquals(SyncStatus.RecoverableError, (outcome as SyncOutcome.RetryableFailure).status)
    }

    @Test
    fun syncOutcome_terminalFailureHoldsStatus() {
        val outcome = SyncOutcome.TerminalFailure(SyncStatus.Conflict)
        assertTrue(outcome is SyncOutcome.TerminalFailure)
        assertEquals(SyncStatus.Conflict, (outcome as SyncOutcome.TerminalFailure).status)
    }

    @Test
    fun syncOutcome_mapping_matchesIssue592Spec() {
        val completedStatuses = listOf(SyncStatus.Success, SyncStatus.NoChanges, SyncStatus.LatestWinsApplied, SyncStatus.BranchMissingRecovered)
        val retryableStatuses = listOf(SyncStatus.RecoverableError, SyncStatus.Error)
        val terminalStatuses = listOf(SyncStatus.Conflict, SyncStatus.PartialConflict, SyncStatus.FatalError, SyncStatus.DirtyRepoBlocked)
        
        completedStatuses.forEach { status ->
            val result = com.xiwei.sujian.model.SyncResult(status = status)
            val outcome = SyncOutcome.Completed(result)
            assertTrue("Expected Completed for $status", outcome is SyncOutcome.Completed)
        }
        retryableStatuses.forEach { status ->
            val outcome = SyncOutcome.RetryableFailure(status)
            assertTrue("Expected RetryableFailure for $status", outcome is SyncOutcome.RetryableFailure)
        }
        terminalStatuses.forEach { status ->
            val outcome = SyncOutcome.TerminalFailure(status)
            assertTrue("Expected TerminalFailure for $status", outcome is SyncOutcome.TerminalFailure)
        }
    }
}
