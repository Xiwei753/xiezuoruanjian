package com.xiwei.sujian.feature.settings.data

// #617 评论六：沉浸式全屏开关的专用布尔状态契约测试。
// localSettingsState 已删除 — 窗口执行层只认 immersiveFullscreenEnabled 这一位：
// 1. 构造时直接从 SharedPreferences 取真实值（冷启动第一帧无系统栏闪现）；
// 2. getLocalSettings() 只返回完整 LocalSettings，不扰动这一位；
// 3. 其它本地设置字段的读写不改变这一位（保存成功路径只同步全屏布尔）。
//
// 单元测试环境不加载原生库：saveLocalSettings 走 NotLoaded → Failed 分支，
// 保存成功路径的同步无法在此注入，由构造初始化 + 非扰动契约覆盖可测部分。

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsRepositoryImmersiveFullscreenStateTest {
    private fun prefs(suffix: String) =
        RuntimeEnvironment.getApplication()
            .getSharedPreferences("sujian_diagnostics_$suffix", android.content.Context.MODE_PRIVATE)

    @Test
    fun `immersiveFullscreenEnabled initializes from prefs at construction`() {
        prefs("init_true").edit().putBoolean("experimental_fullscreen_mode", true).commit()
        val repo =
            SettingsRepository(
                RuntimeEnvironment.getApplication(),
                preferencesSuffix = "init_true",
            )
        assertTrue(
            "冷启动第一帧必须直接读到已开启的沉浸式全屏，不得先 show 再 hide",
            repo.immersiveFullscreenEnabled.value,
        )

        val repoDefault =
            SettingsRepository(
                RuntimeEnvironment.getApplication(),
                preferencesSuffix = "init_default",
            )
        assertFalse(
            "未开启过沉浸式全屏时默认为 false",
            repoDefault.immersiveFullscreenEnabled.value,
        )
    }

    @Test
    fun `getLocalSettings does not touch the fullscreen state`() {
        prefs("get_no_touch").edit().putBoolean("experimental_fullscreen_mode", true).commit()
        val repo =
            SettingsRepository(
                RuntimeEnvironment.getApplication(),
                preferencesSuffix = "get_no_touch",
            )
        assertTrue(repo.immersiveFullscreenEnabled.value)

        // 修改其它诊断 prefs 后重新读取完整设置 — 全屏位不得被 getLocalSettings 扰动。
        prefs("get_no_touch").edit().putBoolean("diagnostics_verbose", false).commit()
        repo.getLocalSettings()
        assertTrue(
            "getLocalSettings 只返回完整设置，不得改动全屏状态",
            repo.immersiveFullscreenEnabled.value,
        )
    }
}
