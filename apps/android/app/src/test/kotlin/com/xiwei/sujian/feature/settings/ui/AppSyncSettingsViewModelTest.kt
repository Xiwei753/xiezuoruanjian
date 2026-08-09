package com.xiwei.sujian.feature.settings.ui

import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #6003 detekt：应用级同步测试从 SettingsViewModelTest 拆分 — 降低 TooManyFunctions。
 * 覆盖 project/app 同步配置独立性、AppSync SaveCommand 携带字段。
 * 重复 remoteUrl 字面量提取为 companion object 常量，避免 StringLiteralDuplication。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AppSyncSettingsViewModelTest {
    private fun createVm(): SettingsViewModel {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val repo = com.xiwei.sujian.feature.settings.data.SettingsRepository(context)
        val themeRepo = com.xiwei.sujian.app.theme.ThemeRepository(context)
        val syncRepo = com.xiwei.sujian.feature.sync.data.SyncRepository(context)
        val syncStatusRepo = com.xiwei.sujian.feature.sync.data.SyncStatusRepository(syncRepo)
        val coordinator = com.xiwei.sujian.feature.sync.data.SyncCoordinator(syncRepo, syncStatusRepo)
        return SettingsViewModel(repo, themeRepo, syncRepo, coordinator)
    }

    @Test
    fun `project and app sync config are independent`() {
        val vm = createVm()
        val projectConfig =
            com.xiwei.sujian.feature.sync.data.model.SyncConfig(
                enabled = true,
                remoteUrl = PROJECT_REMOTE_URL,
            )
        val appConfig = com.xiwei.sujian.feature.sync.data.model.SyncConfig(enabled = false, remoteUrl = APP_REMOTE_URL)
        vm.handleIntent(SettingsIntent.UpdateProjectSyncConfig(projectConfig))
        vm.handleIntent(SettingsIntent.UpdateAppSyncConfig(appConfig))
        // project config 不受 app config 影响
        assertEquals(true, vm.uiState.value.projectSyncConfig.enabled)
        assertEquals(PROJECT_REMOTE_URL, vm.uiState.value.projectSyncConfig.remoteUrl)
        // app config 不受 project config 影响
        assertEquals(false, vm.uiState.value.appSyncConfig.enabled)
        assertEquals(APP_REMOTE_URL, vm.uiState.value.appSyncConfig.remoteUrl)
    }

    @Test
    fun `project and app sync secrets are independent`() {
        val vm = createVm()
        val projectSecrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "project-token")
        val appSecrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "app-token")
        vm.handleIntent(SettingsIntent.UpdateProjectSyncSecrets(projectSecrets))
        vm.handleIntent(SettingsIntent.UpdateAppSyncSecrets(appSecrets))
        assertEquals("project-token", vm.uiState.value.projectSyncSecrets.token)
        assertEquals("app-token", vm.uiState.value.appSyncSecrets.token)
    }

    @Test
    fun `UpdateAppSyncConfig does not touch projectSyncConfig`() {
        val vm = createVm()
        val projectConfig =
            com.xiwei.sujian.feature.sync.data.model.SyncConfig(
                enabled = true,
                remoteUrl = PROJECT_REMOTE_URL,
            )
        vm.handleIntent(SettingsIntent.UpdateProjectSyncConfig(projectConfig))
        val appConfig = com.xiwei.sujian.feature.sync.data.model.SyncConfig(enabled = false, remoteUrl = APP_REMOTE_URL)
        vm.handleIntent(SettingsIntent.UpdateAppSyncConfig(appConfig))
        // project config 保持不变
        assertEquals(true, vm.uiState.value.projectSyncConfig.enabled)
        assertEquals(PROJECT_REMOTE_URL, vm.uiState.value.projectSyncConfig.remoteUrl)
    }

    @Test
    fun `UpdateProjectSyncConfig does not touch appSyncConfig`() {
        val vm = createVm()
        val appConfig = com.xiwei.sujian.feature.sync.data.model.SyncConfig(enabled = true, remoteUrl = APP_REMOTE_URL)
        vm.handleIntent(SettingsIntent.UpdateAppSyncConfig(appConfig))
        val projectConfig =
            com.xiwei.sujian.feature.sync.data.model.SyncConfig(
                enabled = false,
                remoteUrl = PROJECT_REMOTE_URL,
            )
        vm.handleIntent(SettingsIntent.UpdateProjectSyncConfig(projectConfig))
        // app config 保持不变
        assertEquals(true, vm.uiState.value.appSyncConfig.enabled)
        assertEquals(APP_REMOTE_URL, vm.uiState.value.appSyncConfig.remoteUrl)
    }

    @Test
    fun `SettingsSaveCommand AppSyncConfig carries config and revision`() {
        val config = com.xiwei.sujian.feature.sync.data.model.SyncConfig(autoSync = true)
        val cmd = SettingsSaveCommand.AppSyncConfig(config, 5L)
        assertEquals(true, cmd.config.autoSync)
        assertEquals(5L, cmd.revision)
    }

    @Test
    fun `SettingsSaveCommand AppSyncSecrets carries secrets and revision`() {
        val secrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "app-secret")
        val cmd = SettingsSaveCommand.AppSyncSecrets(secrets, 6L)
        assertEquals("app-secret", cmd.secrets.token)
        assertEquals(6L, cmd.revision)
    }

    private fun awaitUntil(
        predicate: () -> Boolean,
        message: String,
        timeoutMs: Long = 15_000,
    ) {
        val shadow = org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper())
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            shadow.idle()
            if (predicate()) return
            Thread.sleep(10)
        }
        org.junit.Assert.fail("$message (within ${timeoutMs}ms)")
    }

    companion object {
        private const val PROJECT_REMOTE_URL = "https://project.git"
        private const val APP_REMOTE_URL = "https://app.git"
    }
}
