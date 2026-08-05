package com.xiwei.sujian.ui.compose.settings

import com.xiwei.sujian.model.LocalSettings
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsViewModelTest {

    private fun createVm(): SettingsViewModel {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val repo = com.xiwei.sujian.data.SettingsRepository(context)
        val syncStatusRepo = com.xiwei.sujian.data.SyncStatusRepository(repo)
        val coordinator = com.xiwei.sujian.data.SyncCoordinator(repo, syncStatusRepo)
        return SettingsViewModel(repo, coordinator)
    }

    @Test
    fun `handleIntent UpdateLocal updates uiState settings`() {
        val vm = createVm()
        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorFontSize = 18f) })
        assertEquals(18f, vm.uiState.value.settings.editorFontSize, 0.01f)
    }

    @Test
    fun `handleIntent UpdateFontSize updates uiState fontSize`() {
        val vm = createVm()
        vm.handleIntent(SettingsIntent.UpdateFontSize(20f))
        assertEquals(20f, vm.uiState.value.fontSize, 0.01f)
    }

    @Test
    fun `handleIntent UpdateSyncConfig updates uiState syncConfig`() {
        val vm = createVm()
        val config = com.xiwei.sujian.model.SyncConfig(autoSync = true)
        vm.handleIntent(SettingsIntent.UpdateSyncConfig(config))
        assertEquals(true, vm.uiState.value.syncConfig.autoSync)
    }

    @Test
    fun `handleIntent UpdateSyncSecrets updates uiState syncSecrets`() {
        val vm = createVm()
        val secrets = com.xiwei.sujian.model.SyncSecrets(token = "test-token")
        vm.handleIntent(SettingsIntent.UpdateSyncSecrets(secrets))
        assertEquals("test-token", vm.uiState.value.syncSecrets.token)
    }

    @Test
    fun `handleIntent Refresh keeps uiState at defaults when no repo injected`() {
        val vm = createVm()
        vm.handleIntent(SettingsIntent.Refresh)
        assertEquals(16f, vm.uiState.value.fontSize, 0.01f)
    }

    @Test
    fun `consumeSaveError clears saveErrorResId`() {
        val vm = createVm()
        vm.handleIntent(SettingsIntent.UpdateFontSize(20f))
        vm.consumeSaveError()
        assertNull(vm.uiState.value.saveErrorResId)
    }

    @Test
    fun `SettingsSaveCommand Local carries settings and revision`() {
        val settings = LocalSettings(editorFontSize = 18f)
        val cmd = SettingsSaveCommand.Local(settings, 1L)
        assertEquals(18f, cmd.settings.editorFontSize, 0.01f)
        assertEquals(1L, cmd.revision)
    }

    @Test
    fun `SettingsSaveCommand FontSize carries fontSize and revision`() {
        val cmd = SettingsSaveCommand.FontSize(20f, 2L)
        assertEquals(20f, cmd.fontSize, 0.01f)
        assertEquals(2L, cmd.revision)
    }

    @Test
    fun `SettingsSaveCommand SyncConfig carries config and revision`() {
        val config = com.xiwei.sujian.model.SyncConfig(autoSync = true)
        val cmd = SettingsSaveCommand.SyncConfig(config, 3L)
        assertEquals(true, cmd.config.autoSync)
        assertEquals(3L, cmd.revision)
    }

    @Test
    fun `SettingsSaveCommand SyncSecrets carries secrets and revision`() {
        val secrets = com.xiwei.sujian.model.SyncSecrets(token = "secret")
        val cmd = SettingsSaveCommand.SyncSecrets(secrets, 4L)
        assertEquals("secret", cmd.secrets.token)
        assertEquals(4L, cmd.revision)
    }
}
