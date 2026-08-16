package com.xiwei.sujian.feature.settings.ui

import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论二：编辑器副作用传播（affectsEditor / hasDifferentEditorFrom / mergeCommand OR）单测。
 *
 * 从 SettingsViewModelTest 拆出以保持该类函数数低于 detekt TooManyFunctions 阈值（20），
 * 不削弱测试——四个测试方法原样迁入，正反例覆盖完整。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsEditorAffectsTest {
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
    fun `hasDifferentEditorFrom is true when editor fields change`() {
        val base = LocalSettings()
        assertTrue(base.hasDifferentEditorFrom(LocalSettings(editorFontSize = 20f)))
        assertTrue(base.hasDifferentEditorFrom(LocalSettings(editorLineSpacingMultiplier = 2.0f)))
        assertTrue(base.hasDifferentEditorFrom(LocalSettings(autoIndentEnabled = false)))
        assertTrue(base.hasDifferentEditorFrom(LocalSettings(autoIndentWidth = 4.0f)))
        assertTrue(base.hasDifferentEditorFrom(LocalSettings(editorTypingAnimationEnabled = false)))
        assertTrue(base.hasDifferentEditorFrom(LocalSettings(editorTypingAnimationDurationMs = 200)))
        assertTrue(base.hasDifferentEditorFrom(LocalSettings(editorSmoothCursorEnabled = false)))
        assertTrue(base.hasDifferentEditorFrom(LocalSettings(editorSmoothCursorDurationMs = 120)))
        assertTrue(base.hasDifferentEditorFrom(LocalSettings(editorCoordinatedTextCursorAnimationEnabled = false)))
        assertTrue(base.hasDifferentEditorFrom(LocalSettings(useSelfRenderEditorOnAndroid = false)))
    }

    @Test
    fun `hasDifferentEditorFrom is false for non-editor fields`() {
        val base = LocalSettings()
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(autoSaveEnabled = false)))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(autoSaveDelayMs = 500L)))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(aiEnabled = true)))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(diagnosticsEnabled = false)))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(diagnosticsVerbose = false)))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(experimentalFullscreenMode = true)))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(appearanceMode = "light")))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(colorSource = "dynamic")))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(dynamicColorEnabled = true)))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(selectedBuiltinThemeId = "ink")))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings(selectedPaletteId = "p1")))
        assertFalse(base.hasDifferentEditorFrom(LocalSettings()))
    }

    @Test
    fun `SettingsSaveCommand Local carries affectsEditor`() {
        val settings = LocalSettings(editorFontSize = 18f)
        val cmd = SettingsSaveCommand.Local(settings, 1L, affectsTheme = false, affectsEditor = true)
        assertTrue(cmd.affectsEditor)
        assertFalse(cmd.affectsTheme)
        // 默认值：未指定 affectsEditor 时为 false，不触发编辑器重载。
        val defaultCmd = SettingsSaveCommand.Local(settings, 1L, affectsTheme = false)
        assertFalse(defaultCmd.affectsEditor)
    }

    @Test
    fun `mergeCommand keeps affectsEditor when later local save is non-editor`() {
        val vm = createVm()
        val editorSettings = LocalSettings(editorFontSize = 20f)
        val editorCmd =
            SettingsSaveCommand.Local(editorSettings, 1L, affectsTheme = false, affectsEditor = true)
        val nonEditorCmd =
            SettingsSaveCommand.Local(
                editorSettings.copy(autoSaveEnabled = false),
                2L,
                affectsTheme = false,
                affectsEditor = false,
            )
        vm.mergeCommand(editorCmd)
        vm.mergeCommand(nonEditorCmd)
        assertTrue("后一个非编辑器保存不能盖掉前一个编辑器变化", vm.pendingCommands.local?.affectsEditor == true)
        // 合并后的命令携带最新的 settings 与 revision，但 affectsEditor 取并集。
        assertEquals(2L, vm.pendingCommands.local?.revision)
        assertFalse(vm.pendingCommands.local?.settings?.autoSaveEnabled == true)
    }
}
