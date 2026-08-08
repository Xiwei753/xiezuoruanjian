package com.xiwei.sujian.feature.settings.ui

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
        val repo = com.xiwei.sujian.core.interop.settings.SettingsRepository(context)
        val themeRepo = com.xiwei.sujian.app.theme.ThemeRepository(context)
        val syncRepo = com.xiwei.sujian.feature.sync.data.SyncRepository(context)
        val syncStatusRepo = com.xiwei.sujian.core.interop.sync.SyncStatusRepository(syncRepo)
        val coordinator = com.xiwei.sujian.core.interop.sync.SyncCoordinator(syncRepo, syncStatusRepo)
        val factory = SettingsViewModel.Factory(repo, themeRepo, syncRepo, coordinator)
        val vm = factory.create(SettingsViewModel::class.java)
        assertNotNull(vm)
    }

    @Test
    fun constructorInjectedDeps_noInitializeNeeded() {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val repo = com.xiwei.sujian.core.interop.settings.SettingsRepository(context)
        val themeRepo = com.xiwei.sujian.app.theme.ThemeRepository(context)
        val syncRepo = com.xiwei.sujian.feature.sync.data.SyncRepository(context)
        val syncStatusRepo = com.xiwei.sujian.core.interop.sync.SyncStatusRepository(syncRepo)
        val coordinator = com.xiwei.sujian.core.interop.sync.SyncCoordinator(syncRepo, syncStatusRepo)
        val vm = SettingsViewModel(repo, themeRepo, syncRepo, coordinator)
        assertEquals(16f, vm.uiState.value.fontSize, 0.01f)
    }

    @Test
    fun factory_producesDistinctInstances() {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val repo = com.xiwei.sujian.core.interop.settings.SettingsRepository(context)
        val themeRepo = com.xiwei.sujian.app.theme.ThemeRepository(context)
        val syncRepo = com.xiwei.sujian.feature.sync.data.SyncRepository(context)
        val syncStatusRepo = com.xiwei.sujian.core.interop.sync.SyncStatusRepository(syncRepo)
        val coordinator = com.xiwei.sujian.core.interop.sync.SyncCoordinator(syncRepo, syncStatusRepo)
        val factory = SettingsViewModel.Factory(repo, themeRepo, syncRepo, coordinator)
        val vm1 = factory.create(SettingsViewModel::class.java)
        val vm2 = factory.create(SettingsViewModel::class.java)
        assertNotNull(vm1)
        assertNotNull(vm2)
        assertEquals(true, vm1 !== vm2)
    }
}
