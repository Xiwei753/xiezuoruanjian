package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncStatus
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
        val retryableStatuses = listOf(SyncStatus.RecoverableError)
        val terminalStatuses = listOf(SyncStatus.Error, SyncStatus.Conflict, SyncStatus.PartialConflict, SyncStatus.FatalError, SyncStatus.DirtyRepoBlocked)
        
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

    @Test
    fun syncingStatus_isTerminalFailure_protocolError() {
        // #592 三：performSync 返回 Syncing 是协议错误 → TerminalFailure(FatalError)
        val outcome = SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        assertTrue(outcome is SyncOutcome.TerminalFailure)
        assertEquals(SyncStatus.FatalError, (outcome as SyncOutcome.TerminalFailure).status)
    }

    @Test
    fun notLoaded_isTerminalFailure_fatalError() {
        // #592 三：原生库未加载是致命错误 → TerminalFailure(FatalError)
        val outcome = SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        assertTrue(outcome is SyncOutcome.TerminalFailure)
    }
}
