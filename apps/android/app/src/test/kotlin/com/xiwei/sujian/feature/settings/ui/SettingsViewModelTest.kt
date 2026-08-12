package com.xiwei.sujian.feature.settings.ui

import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsViewModelTest {
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
    fun `handleIntent UpdateProjectSyncConfig updates uiState projectSyncConfig`() {
        val vm = createVm()
        val config = com.xiwei.sujian.feature.sync.data.model.SyncConfig(autoSync = true)
        vm.handleIntent(SettingsIntent.UpdateProjectSyncConfig(config))
        assertEquals(true, vm.uiState.value.projectSyncConfig.autoSync)
    }

    @Test
    fun `handleIntent UpdateProjectSyncSecrets updates uiState projectSyncSecrets`() {
        val vm = createVm()
        val secrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "test-token")
        vm.handleIntent(SettingsIntent.UpdateProjectSyncSecrets(secrets))
        assertEquals("test-token", vm.uiState.value.projectSyncSecrets.token)
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
    fun `section states derive from uiState`() {
        val vm = createVm()
        // stateIn(WhileSubscribed) 只在有订阅时推进上游；先挂三个分类的收集器。
        val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Main)
        val appearanceValues = mutableListOf<AppearanceSectionState>()
        val laboratoryValues = mutableListOf<LaboratorySectionState>()
        val syncValues = mutableListOf<SyncSectionState>()
        scope.launch { vm.appearanceState.collect { appearanceValues += it } }
        scope.launch { vm.laboratoryState.collect { laboratoryValues += it } }
        scope.launch { vm.syncState.collect { syncValues += it } }
        awaitUntil(
            predicate = { appearanceValues.isNotEmpty() && laboratoryValues.isNotEmpty() && syncValues.isNotEmpty() },
            message = "collectors must attach and receive initial values",
        )
        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(experimentalFullscreenMode = true) })
        vm.handleIntent(SettingsIntent.UpdateFontSize(20f))
        vm.handleIntent(
            SettingsIntent.UpdateProjectSyncConfig(
                com.xiwei.sujian.feature.sync.data.model.SyncConfig(enabled = true),
            ),
        )
        awaitUntil(
            predicate = { laboratoryValues.last().immersiveFullscreen },
            message = "laboratory state must emit the new value",
        )
        // 各分类 state 立即反映对应字段的新值。
        assertTrue(laboratoryValues.last().immersiveFullscreen)
        assertEquals(20f, appearanceValues.last().fontSize, 0.01f)
        assertTrue(syncValues.last().projectSyncConfig.enabled == true)
        // 无关字段不受影响：外观主题模式仍是默认 system。
        assertEquals("system", appearanceValues.last().appearanceMode)
        scope.cancel()
    }

    @Test
    fun `laboratory change only re-emits laboratory state`() {
        val vm = createVm()
        val appearanceValues = mutableListOf<AppearanceSectionState>()
        val laboratoryValues = mutableListOf<LaboratorySectionState>()
        val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Main)
        val appearanceJob =
            scope.launch { vm.appearanceState.collect { appearanceValues += it } }
        val laboratoryJob =
            scope.launch { vm.laboratoryState.collect { laboratoryValues += it } }
        awaitUntil(
            predicate = { appearanceValues.isNotEmpty() && laboratoryValues.isNotEmpty() },
            message = "collectors must attach and receive initial values",
        )
        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(experimentalFullscreenMode = true) })
        awaitUntil(
            predicate = { laboratoryValues.last().immersiveFullscreen },
            message = "laboratory state must emit the new value",
        )
        // 实验室开关变化不得传播到外观分类（distinctUntilChanged 过滤无关字段）。
        assertEquals(
            "外观分类只收到初始值，实验室变化不触发其重发",
            1,
            appearanceValues.size,
        )
        appearanceJob.cancel()
        laboratoryJob.cancel()
        scope.cancel()
    }

    @Test
    fun `SettingsSaveCommand Local carries settings revision and affectsTheme`() {
        val settings = LocalSettings(editorFontSize = 18f)
        val cmd = SettingsSaveCommand.Local(settings, 1L, affectsTheme = true)
        assertEquals(18f, cmd.settings.editorFontSize, 0.01f)
        assertEquals(1L, cmd.revision)
        assertTrue(cmd.affectsTheme)
    }

    @Test
    fun `hasDifferentThemeFrom is true when theme fields change`() {
        val base = LocalSettings()
        assertTrue(base.hasDifferentThemeFrom(LocalSettings(appearanceMode = "light")))
        assertTrue(base.hasDifferentThemeFrom(LocalSettings(colorSource = "dynamic")))
        assertTrue(base.hasDifferentThemeFrom(LocalSettings(dynamicColorEnabled = true)))
        assertTrue(base.hasDifferentThemeFrom(LocalSettings(selectedBuiltinThemeId = "ink")))
        assertTrue(base.hasDifferentThemeFrom(LocalSettings(selectedPaletteId = "p1")))
    }

    @Test
    fun `hasDifferentThemeFrom is false for non-theme fields`() {
        val base = LocalSettings()
        assertFalse(base.hasDifferentThemeFrom(LocalSettings(experimentalFullscreenMode = true)))
        assertFalse(base.hasDifferentThemeFrom(LocalSettings(diagnosticsVerbose = false)))
        assertFalse(base.hasDifferentThemeFrom(LocalSettings(editorFontSize = 20f)))
        assertFalse(base.hasDifferentThemeFrom(LocalSettings(autoSaveDelayMs = 500L)))
        assertFalse(base.hasDifferentThemeFrom(LocalSettings()))
    }

    @Test
    fun `mergeCommand keeps affectsTheme when later local save is non-theme`() {
        val vm = createVm()
        val themeSettings = LocalSettings(appearanceMode = "light")
        val themeCmd = SettingsSaveCommand.Local(themeSettings, 1L, affectsTheme = true)
        val nonThemeCmd =
            SettingsSaveCommand.Local(
                themeSettings.copy(experimentalFullscreenMode = true),
                2L,
                affectsTheme = false,
            )
        vm.mergeCommand(themeCmd)
        vm.mergeCommand(nonThemeCmd)
        assertTrue("后一个非主题保存不能盖掉前一个主题变化", vm.pendingCommands.local?.affectsTheme == true)
        // 合并后的命令携带最新的 settings 与 revision，但 affectsTheme 取并集。
        assertEquals(2L, vm.pendingCommands.local?.revision)
        assertTrue(vm.pendingCommands.local?.settings?.experimentalFullscreenMode == true)
    }

    @Test
    fun `SettingsSaveCommand FontSize carries fontSize and revision`() {
        val cmd = SettingsSaveCommand.FontSize(20f, 2L)
        assertEquals(20f, cmd.fontSize, 0.01f)
        assertEquals(2L, cmd.revision)
    }

    @Test
    fun `SettingsSaveCommand ProjectSyncConfig carries config and revision`() {
        val config = com.xiwei.sujian.feature.sync.data.model.SyncConfig(autoSync = true)
        val cmd = SettingsSaveCommand.ProjectSyncConfig(config, 3L)
        assertEquals(true, cmd.config.autoSync)
        assertEquals(3L, cmd.revision)
    }

    @Test
    fun `SettingsSaveCommand ProjectSyncSecrets carries secrets and revision`() {
        val secrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "secret")
        val cmd = SettingsSaveCommand.ProjectSyncSecrets(secrets, 4L)
        assertEquals("secret", cmd.secrets.token)
        assertEquals(4L, cmd.revision)
    }

    @Test
    fun `handleIntent PerformSync ends in terminal failure via serial transaction`() {
        val vm = createVm()
        val config =
            com.xiwei.sujian.feature.sync.data.model.SyncConfig(
                enabled = false,
                autoSync = false,
                remoteUrl = "https://unit.example/repo.git",
            )
        vm.handleIntent(SettingsIntent.UpdateProjectSyncConfig(config))
        // 保存并同步必须走 SaveAndRunSync 串行事务（保存队列屏障），并结束在明确终态：
        // 未配置 → 失败；不允许绕过保存队列或停留在 RUNNING/IDLE（#592）。
        vm.handleIntent(SettingsIntent.PerformSync)
        awaitUntil(
            predicate = { vm.uiState.value.projectPerformSyncState == SyncCommandState.FAILURE },
            message = "PerformSync must end in terminal FAILURE state via SaveAndRunSync transaction",
        )
        assertEquals(SyncCommandState.FAILURE, vm.uiState.value.projectPerformSyncState)
    }

    @Test
    fun `sync profile load failure surfaces Failed state instead of silent defaults`() {
        val vm = createVm()
        // 测试环境无 native 库：config/凭据读取全部失败。
        // #595 四：设置页必须显示 Failed（真实错误），不得通过 toConfigSecretsOrNull()
        // 把失败静默转成默认空 token（那会伪装成“尚未配置”）。
        awaitUntil(
            predicate = { vm.uiState.value.projectSyncProfileLoadState !is SyncProfileLoadState.Loading },
            message = "initial sync profile load must settle",
        )
        assertTrue(
            "读取失败必须显示为 SyncProfileLoadState.Failed，实际: ${vm.uiState.value.projectSyncProfileLoadState}",
            vm.uiState.value.projectSyncProfileLoadState is SyncProfileLoadState.Failed,
        )
        assertTrue(
            "Failed 不是 Unconfigured",
            vm.uiState.value.projectSyncProfileLoadState !is SyncProfileLoadState.Unconfigured,
        )
        // 失败时字段保留已确认值（初始默认），不因失败清空。
        assertEquals(16f, vm.uiState.value.fontSize, 0.01f)
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
}
