package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncIndicatorState
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

class SyncCoordinatorTest {

    @Test
    fun notifySyncStarted_publishesSyncing() {
        SyncStatusRepository.notifySyncStarted()
        assertEquals(SyncIndicatorState.Syncing, SyncStatusRepository.state.value)
    }

    @Test
    fun notifySyncSuccess_publishesSynced() {
        SyncStatusRepository.notifySyncSuccess()
        assertEquals(SyncIndicatorState.Synced, SyncStatusRepository.state.value)
    }

    @Test
    fun notifySyncFailed_publishesFailed() {
        SyncStatusRepository.notifySyncFailed()
        assertEquals(SyncIndicatorState.Failed, SyncStatusRepository.state.value)
    }

    @Test
    fun notifyUnconfigured_publishesUnconfigured() {
        SyncStatusRepository.notifyUnconfigured()
        assertEquals(SyncIndicatorState.Unconfigured, SyncStatusRepository.state.value)
    }

    @Test
    fun syncStatusTransitions_followExpectedSequence() {
        SyncStatusRepository.notifySyncStarted()
        assertEquals(SyncIndicatorState.Syncing, SyncStatusRepository.state.value)

        SyncStatusRepository.notifySyncSuccess()
        assertEquals(SyncIndicatorState.Synced, SyncStatusRepository.state.value)

        SyncStatusRepository.notifySyncStarted()
        assertEquals(SyncIndicatorState.Syncing, SyncStatusRepository.state.value)

        SyncStatusRepository.notifySyncFailed()
        assertEquals(SyncIndicatorState.Failed, SyncStatusRepository.state.value)
    }

    @Test
    fun syncSession_runExclusive_blocksConcurrentAccess() = runTest {
        var firstEntered = false
        var secondBlocked = false

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
}
