package com.xiwei.sujian.feature.settings.data

// #617 评论四：本地设置可观察状态（localSettingsState）契约测试。
// 单元测试环境不加载原生库（NotLoaded 路径），getLocalSettings() 仍必须把
// 诊断/实验 prefs 合并结果发布到 StateFlow — 这是沉浸式全屏执行层的唯一输入。

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsRepositoryLocalSettingsStateTest {
    private fun createRepo(preferencesSuffix: String = "state_test"): SettingsRepository {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        return SettingsRepository(context, preferencesSuffix = preferencesSuffix)
    }

    @Test
    fun `getLocalSettings publishes merged diag prefs into localSettingsState`() {
        val repo = createRepo()
        assertFalse(repo.localSettingsState.value.experimentalFullscreenMode)

        val context = org.robolectric.RuntimeEnvironment.getApplication()
        context.getSharedPreferences("sujian_diagnostics_state_test", android.content.Context.MODE_PRIVATE)
            .edit()
            .putBoolean("experimental_fullscreen_mode", true)
            .commit()

        repo.getLocalSettings()
        assertTrue(
            "getLocalSettings 必须把 prefs 合并结果发布到 localSettingsState",
            repo.localSettingsState.value.experimentalFullscreenMode,
        )
    }

    @Test
    fun `localSettingsState reflects non-theme local settings fields`() {
        val repo = createRepo("state_fields")
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        context.getSharedPreferences("sujian_diagnostics_state_fields", android.content.Context.MODE_PRIVATE)
            .edit()
            .putBoolean("diagnostics_verbose", false)
            .commit()

        repo.getLocalSettings()
        assertFalse(repo.localSettingsState.value.diagnosticsVerbose)
        // 未写入的 prefs 保持默认值
        assertEquals(true, repo.localSettingsState.value.diagnosticsEnabled)
        assertEquals("system", repo.localSettingsState.value.appearanceMode)
    }
}
