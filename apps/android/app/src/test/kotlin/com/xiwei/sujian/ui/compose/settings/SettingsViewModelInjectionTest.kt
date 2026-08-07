package com.xiwei.sujian.ui.compose.settings

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsViewModelInjectionTest {

    @Test
    fun factory_createsViewModelWithNonNullDeps() {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val repo = com.xiwei.sujian.data.SettingsRepository(context)
        val syncStatusRepo = com.xiwei.sujian.data.SyncStatusRepository(repo)
        val coordinator = com.xiwei.sujian.data.SyncCoordinator(repo, syncStatusRepo)
        val factory = SettingsViewModel.Factory(repo, coordinator)
        val vm = factory.create(SettingsViewModel::class.java)
        assertNotNull(vm)
    }

    @Test
    fun constructorInjectedDeps_noInitializeNeeded() {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val repo = com.xiwei.sujian.data.SettingsRepository(context)
        val syncStatusRepo = com.xiwei.sujian.data.SyncStatusRepository(repo)
        val coordinator = com.xiwei.sujian.data.SyncCoordinator(repo, syncStatusRepo)
        val vm = SettingsViewModel(repo, coordinator)
        assertEquals(16f, vm.uiState.value.fontSize, 0.01f)
    }

    @Test
    fun factory_producesDistinctInstances() {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val repo = com.xiwei.sujian.data.SettingsRepository(context)
        val syncStatusRepo = com.xiwei.sujian.data.SyncStatusRepository(repo)
        val coordinator = com.xiwei.sujian.data.SyncCoordinator(repo, syncStatusRepo)
        val factory = SettingsViewModel.Factory(repo, coordinator)
        val vm1 = factory.create(SettingsViewModel::class.java)
        val vm2 = factory.create(SettingsViewModel::class.java)
        assertNotNull(vm1)
        assertNotNull(vm2)
        assertEquals(true, vm1 !== vm2)
    }
}
