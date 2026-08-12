package com.xiwei.sujian.feature.settings.ui

import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #618 六 复审契约：编辑器分类的 section state 投影只订阅自己消费的字段。
 *
 * - 改主题颜色（外观字段变化）不得重发 [SettingsViewModel.editorState] —
 *   编辑器分类标题行只显示字号，不随主题/颜色来源/动态色/行距重组；
 * - 改字号必须重发 [SettingsViewModel.editorState] 且携带新值 —
 *   编辑器分类标题行与外观分类的字号滑杆都显示当前字号（必要更新）。
 *
 * 独立成类：保持 SettingsViewModelTest 的函数数低于 detekt TooManyFunctions 阈值。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsEditorSectionStateTest {
    @Test
    fun `editor state re-emits on fontSize change but not on theme change`() {
        val vm = createVm()
        val appearanceValues = mutableListOf<AppearanceSectionState>()
        val editorValues = mutableListOf<EditorSectionState>()
        val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Main)
        val appearanceJob =
            scope.launch { vm.appearanceState.collect { appearanceValues += it } }
        val editorJob =
            scope.launch { vm.editorState.collect { editorValues += it } }
        awaitUntil(
            predicate = { appearanceValues.isNotEmpty() && editorValues.isNotEmpty() },
            message = "collectors must attach and receive initial values",
        )

        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(appearanceMode = "dark") })
        awaitUntil(
            predicate = { appearanceValues.last().appearanceMode == "dark" },
            message = "appearance state must emit the new appearance mode",
        )
        // 主题/外观变化不得波及编辑器分类：editorState 只收到初始值。
        assertEquals(
            "外观主题变化不得重发 editorState",
            1,
            editorValues.size,
        )

        vm.handleIntent(SettingsIntent.UpdateFontSize(20f))
        awaitUntil(
            predicate = { editorValues.size >= 2 },
            message = "font size change must re-emit editor state",
        )
        assertEquals(20f, editorValues.last().fontSize, 0.01f)
        appearanceJob.cancel()
        editorJob.cancel()
        scope.cancel()
    }

    private fun createVm(): SettingsViewModel {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val repo = com.xiwei.sujian.feature.settings.data.SettingsRepository(context)
        val themeRepo = com.xiwei.sujian.app.theme.ThemeRepository(context)
        val syncRepo = com.xiwei.sujian.feature.sync.data.SyncRepository(context)
        val syncStatusRepo = com.xiwei.sujian.feature.sync.data.SyncStatusRepository(syncRepo)
        val coordinator = com.xiwei.sujian.feature.sync.data.SyncCoordinator(syncRepo, syncStatusRepo)
        return SettingsViewModel(repo, themeRepo, syncRepo, coordinator)
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
