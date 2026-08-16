package com.xiwei.sujian.feature.settings.ui

import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论 #1+#2：同步配置只有一份 — 全量同步覆盖设置/星图/主题/全部作品。
 *
 * 旧的 project/app 独立性测试已不再适用（双份状态已合并为单一 syncConfig/syncSecrets）。
 * 本测试覆盖单一 SyncConfig/SyncSecrets SaveCommand 携带字段、UpdateSyncConfig/UpdateSyncSecrets
 * 更新 uiState，确保合并后基础行为正确。
 *
 * 从 SettingsViewModelTest 拆出以保持该类函数数低于 detekt TooManyFunctions 阈值。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AppSyncSettingsViewModelTest {
    private fun createVm(): SettingsViewModel {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val bridge = com.xiwei.sujian.app.di.AppServiceProvider.getAppServiceBridge(context)
        val repo = com.xiwei.sujian.feature.settings.data.SettingsRepository(context, bridge)
        val themeRepo = com.xiwei.sujian.app.theme.ThemeRepository(context, bridge)
        val syncRepo = com.xiwei.sujian.feature.sync.data.SyncRepository(context, bridge)
        val syncStatusRepo = com.xiwei.sujian.feature.sync.data.SyncStatusRepository(syncRepo)
        val coordinator = com.xiwei.sujian.feature.sync.data.SyncCoordinator(syncRepo, syncStatusRepo)
        return SettingsViewModel(repo, themeRepo, syncRepo, coordinator)
    }

    @Test
    fun `UpdateSyncConfig updates uiState syncConfig`() {
        val vm = createVm()
        val config =
            com.xiwei.sujian.feature.sync.data.model.SyncConfig(
                enabled = true,
                remoteUrl = REMOTE_URL,
            )
        vm.handleIntent(SettingsIntent.UpdateSyncConfig(config))
        assertEquals(true, vm.uiState.value.syncConfig.enabled)
        assertEquals(REMOTE_URL, vm.uiState.value.syncConfig.remoteUrl)
    }

    @Test
    fun `UpdateSyncSecrets updates uiState syncSecrets`() {
        val vm = createVm()
        val secrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "sync-token")
        vm.handleIntent(SettingsIntent.UpdateSyncSecrets(secrets))
        assertEquals("sync-token", vm.uiState.value.syncSecrets.token)
    }

    @Test
    fun `SettingsSaveCommand SyncConfig carries config and revision`() {
        val config = com.xiwei.sujian.feature.sync.data.model.SyncConfig(autoSync = true)
        val cmd = SettingsSaveCommand.SyncConfig(config, 5L)
        assertEquals(true, cmd.config.autoSync)
        assertEquals(5L, cmd.revision)
    }

    @Test
    fun `SettingsSaveCommand SyncSecrets carries secrets and revision`() {
        val secrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "sync-secret")
        val cmd = SettingsSaveCommand.SyncSecrets(secrets, 6L)
        assertEquals("sync-secret", cmd.secrets.token)
        assertEquals(6L, cmd.revision)
    }

    companion object {
        private const val REMOTE_URL = "https://sync.git"
    }
}
