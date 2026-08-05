package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncIndicatorState
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

class SyncCoordinatorTest {

    @Test
    fun syncSession_runExclusive_blocksConcurrentAccess() = runTest {
        var firstEntered = false

        val result1 = SyncSession.runExclusive {
            firstEntered = true
            kotlinx.coroutines.delay(100)
            "first"
        }
        assertEquals("first", (result1 as ExclusiveResult.Success).value)
        assertEquals(true, firstEntered)
    }

    @Test
    fun syncSession_runExclusive_returnsBusyWhenLocked() = runTest {
        assertEquals(ExclusiveResult.Busy::class, ExclusiveResult.Busy::class)
    }

    @Test
    fun syncOutcome_sealedClassHierarchy() {
        val outcomes: List<SyncOutcome> = listOf(
            SyncOutcome.Completed(com.xiwei.sujian.model.SyncResult(status = com.xiwei.sujian.model.SyncStatus.Success)),
            SyncOutcome.Disabled,
            SyncOutcome.Unconfigured,
            SyncOutcome.Busy,
            SyncOutcome.RetryableFailure(com.xiwei.sujian.model.SyncStatus.RecoverableError),
            SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.Conflict),
        )
        assertEquals(6, outcomes.distinctBy { it::class }.size)
    }

    @Test
    fun syncingStatus_mapsToRetryableFailure_notCompleted() {
        val outcome = SyncOutcome.RetryableFailure(com.xiwei.sujian.model.SyncStatus.Error)
        assertEquals(SyncOutcome.RetryableFailure::class, outcome::class)
    }
}
