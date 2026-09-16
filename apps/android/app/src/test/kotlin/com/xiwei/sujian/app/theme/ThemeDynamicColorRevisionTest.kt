package com.xiwei.sujian.app.theme

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.core.designsystem.theme.ColorSource
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.lang.reflect.Field

/**
 * #698 评论 5697617362：ThemeStore.onDynamicColorsChanged / dynamicColorRevision 回归测试。
 *
 * onDynamicColorsChanged 只在 resolvedColorSource==ANDROID_DYNAMIC 时推进 dynamicColorRevision；
 * 其他来源（built_in / saved_palette fallback）不推进。revision 不写入 Core 配置，仅驱动主题树重组。
 *
 * 注：Robolectric 无法加载 NDK 原生库，saveLocalSettings 是 no-op，getLocalSettings 恒返回默认值
 * （built_in + 空选择），reload 在 SDK 34 上规范化为 android_dynamic。非 android_dynamic 反路径
 * 通过反射直接设置 _uiState 验证。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ThemeDynamicColorRevisionTest {
    private lateinit var context: Context
    private lateinit var settingsRepository: SettingsRepository
    private lateinit var themeRepository: ThemeRepository
    private lateinit var uiStateField: Field

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        val dir = java.nio.file.Files.createTempDirectory("sujian_theme_dyn_rev_test_").toString()
        val bridge = AppServiceBridge(WriterAppServiceHolder(dir, dir))
        settingsRepository = SettingsRepository(context, bridge)
        themeRepository = ThemeRepository(context, bridge)
        ThemeStore.initialize(themeRepository, settingsRepository)
        uiStateField = ThemeStore::class.java.getDeclaredField("_uiState")
        uiStateField.isAccessible = true
    }

    @Suppress("UNCHECKED_CAST")
    private fun setUiState(state: ThemeUiState) {
        val flow = uiStateField.get(ThemeStore) as MutableStateFlow<ThemeUiState>
        flow.value = state
    }

    @Test
    fun onDynamicColorsChanged_incrementsRevisionWhenAndroidDynamic() {
        ThemeStore.reload()
        // 默认值在 SDK 34 上规范化为 android_dynamic
        assertEquals(
            "reload 后应为 android_dynamic",
            ColorSource.ANDROID_DYNAMIC,
            ThemeStore.uiState.value.resolvedColorSource,
        )
        assertEquals("初始 revision 应为 0", 0L, ThemeStore.uiState.value.dynamicColorRevision)

        ThemeStore.onDynamicColorsChanged()
        assertEquals("第一次推进后 revision 应为 1", 1L, ThemeStore.uiState.value.dynamicColorRevision)

        ThemeStore.onDynamicColorsChanged()
        assertEquals("第二次推进后 revision 应为 2", 2L, ThemeStore.uiState.value.dynamicColorRevision)
    }

    @Test
    fun onDynamicColorsChanged_doesNothingWhenBuiltIn() {
        setUiState(
            ThemeUiState(
                colorSource = "built_in",
                selectedBuiltinThemeId = "paper_light",
            ),
        )
        assertEquals(0L, ThemeStore.uiState.value.dynamicColorRevision)

        ThemeStore.onDynamicColorsChanged()
        assertEquals(
            "built_in 来源时 revision 不得推进",
            0L,
            ThemeStore.uiState.value.dynamicColorRevision,
        )
    }

    @Test
    fun onDynamicColorsChanged_doesNothingWhenSavedPaletteFallbackToBuiltin() {
        // saved_palette 但 selectedPaletteRecord==null → resolvedColorSource fallback BUILT_IN
        setUiState(
            ThemeUiState(
                colorSource = "saved_palette",
                selectedPaletteId = "device01:fp01",
            ),
        )
        assertEquals(
            "record 为空时应 fallback 到 BUILT_IN",
            ColorSource.BUILT_IN,
            ThemeStore.uiState.value.resolvedColorSource,
        )
        assertEquals(0L, ThemeStore.uiState.value.dynamicColorRevision)

        ThemeStore.onDynamicColorsChanged()
        assertEquals(
            "saved_palette fallback 到 built_in 时 revision 不得推进",
            0L,
            ThemeStore.uiState.value.dynamicColorRevision,
        )
    }
}
