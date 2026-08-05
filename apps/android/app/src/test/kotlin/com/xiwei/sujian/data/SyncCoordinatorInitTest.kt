package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.model.SyncTrigger
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SyncCoordinatorInitTest {

    @Test
    fun runSync_returnsNullWhenNotInitialized() = runTest {
        val result = SyncCoordinator.runSync(SyncTrigger.Manual)
        assertNull(result)
    }

    @Test
    fun refreshState_returnsEarlyWhenNotInitialized() = runTest {
        SyncStatusRepository.notifySyncSuccess()
        assertEquals(SyncIndicatorState.Synced, SyncStatusRepository.state.value)
        SyncStatusRepository.refreshState()
        assertEquals(SyncIndicatorState.Synced, SyncStatusRepository.state.value)
    }

    @Test
    fun syncCoordinator_allTriggersReturnNullWhenNotInitialized() = runTest {
        assertNull(SyncCoordinator.runSync(SyncTrigger.Manual))
        assertNull(SyncCoordinator.runSync(SyncTrigger.Auto))
        assertNull(SyncCoordinator.runSync(SyncTrigger.SettingsPage))
    }

    @Test
    fun syncStatusRepository_stateFlowIsProcessLevelSingleton() {
        val flow1 = SyncStatusRepository.state
        SyncStatusRepository.notifySyncStarted()
        assertEquals(SyncIndicatorState.Syncing, flow1.value)
    }
}
