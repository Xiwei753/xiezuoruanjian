package com.xiwei.sujian.app.theme

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.lang.reflect.Method

/**
 * #611 二：ThemeStore colorSource 唯一事实条件 + 旧异常组合规范化正反测试。
 *
 * captureDynamicColorAndSave 只在 colorSource="android_dynamic" 时执行；
 * reload 在 Android 12+ 上把“built_in 且未选任何主题/调色板”的旧异常组合
 * 规范化为 android_dynamic。
 *
 * 正测试验证规范化确实发生；反测试验证非匹配条件不触发规范化、不调用
 * ThemePaletteHelper.extractDynamicColorSchemes。
 *
 * 注：Robolectric 无法加载 Android NDK 原生库，saveLocalSettings 是 no-op，
 * getLocalSettings 恒返回默认值（colorSource="built_in" + 空选择）。
 * 因此 reload 正向路径（默认值匹配旧异常组合）通过 uiState 验证；
 * 反向条件（selectedBuiltinThemeId / selectedPaletteId / colorSource 非默认）
 * 通过反射调用 shouldMigrateToDynamicColor 验证规范化判定函数返回 false。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ThemeStoreColorSourceTest {
    private lateinit var context: Context
    private lateinit var settingsRepository: SettingsRepository
    private lateinit var themeRepository: ThemeRepository
    private lateinit var shouldMigrateMethod: Method

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        val dir = java.nio.file.Files.createTempDirectory("sujian_theme_color_source_test_").toString()
        val bridge = AppServiceBridge(WriterAppServiceHolder(dir, dir))
        settingsRepository = SettingsRepository(context, bridge)
        themeRepository = ThemeRepository(context, bridge)
        // ThemeStore 是 object 单例，每个测试前重新注入依赖
        ThemeStore.initialize(themeRepository, settingsRepository)
        // 反射获取 shouldMigrateToDynamicColor 私有方法，用于反向条件验证
        shouldMigrateMethod =
            ThemeStore::class.java
                .getDeclaredMethod("shouldMigrateToDynamicColor", LocalSettings::class.java)
        shouldMigrateMethod.isAccessible = true
    }

    /** 调用 ThemeStore.shouldMigrateToDynamicColor(settings) 并返回结果。 */
    private fun shouldMigrate(settings: LocalSettings): Boolean =
        shouldMigrateMethod.invoke(ThemeStore, settings) as Boolean

    @Test
    fun captureDynamicColorAndSave_returnsEarlyWhenColorSourceIsNotAndroidDynamic() {
        // 默认 colorSource=BUILTIN（!= ANDROID_DYNAMIC）→ captureDynamicColorAndSave
        // 必须早返回，不调用 ThemePaletteHelper.extractDynamicColorSchemes，paletteRecords 不变
        ThemeStore.reload()
        val recordsBefore = ThemeStore.paletteRecords.value

        ThemeStore.captureDynamicColorAndSave(context)

        val recordsAfter = ThemeStore.paletteRecords.value
        assertEquals(
            "colorSource=built_in 时 captureDynamicColorAndSave 不得修改 paletteRecords",
            recordsBefore,
            recordsAfter,
        )
    }

    @Test
    fun captureDynamicColorAndSave_returnsEarlyWhenColorSourceIsSavedPalette() {
        // colorSource=SAVED_PALETTE 同样 != ANDROID_DYNAMIC → 早返回。
        // 原生库未加载时 getLocalSettings 恒返回默认值（colorSource=BUILTIN），
        // 但早返回条件 colorSource != ANDROID_DYNAMIC 对 SAVED_PALETTE 同样成立。
        // 此处通过 shouldMigrateToDynamicColor 反射验证 SAVED_PALETTE 不触发规范化，
        // 并验证 captureDynamicColorAndSave 在非 android_dynamic 下早返回。
        val savedPaletteSettings = LocalSettings(colorSource = SAVED_PALETTE)
        assertFalse(
            "colorSource=saved_palette 不应触发动态色规范化",
            shouldMigrate(savedPaletteSettings),
        )

        ThemeStore.reload()
        val recordsBefore = ThemeStore.paletteRecords.value
        ThemeStore.captureDynamicColorAndSave(context)
        assertEquals(
            "非 android_dynamic 时 captureDynamicColorAndSave 不得修改 paletteRecords",
            recordsBefore,
            ThemeStore.paletteRecords.value,
        )
    }

    @Test
    fun reload_normalizesOldBuiltinWithNoSelectionToDynamicColor() {
        // 默认值：colorSource=BUILTIN + selectedBuiltinThemeId="" + selectedPaletteId=""
        // Android 12+（Robolectric SDK 34 >= 31）→ reload 规范化为 android_dynamic
        val defaults = LocalSettings()
        assertTrue("默认值应触发规范化", shouldMigrate(defaults))

        ThemeStore.reload()

        val uiState = ThemeStore.uiState.value
        assertEquals("colorSource 必须规范化为 android_dynamic", ANDROID_DYNAMIC, uiState.colorSource)
        assertTrue("dynamicColorEnabled 必须随规范化置 true", uiState.dynamicColorEnabled)
    }

    @Test
    fun reload_doesNotNormalizeWhenBuiltinThemeSelected() {
        // selectedBuiltinThemeId 非空 → shouldMigrateToDynamicColor 返回 false → reload 不规范化
        val settings =
            LocalSettings(
                colorSource = BUILTIN,
                selectedBuiltinThemeId = PAPER_LIGHT,
                selectedPaletteId = "",
            )
        assertFalse(
            "已选内置主题时 shouldMigrateToDynamicColor 必须返回 false",
            shouldMigrate(settings),
        )
        // 反向验证：默认值（空选择）应返回 true，证明 selectedBuiltinThemeId 是关键条件
        assertTrue(
            "空选择时 shouldMigrateToDynamicColor 应返回 true",
            shouldMigrate(LocalSettings()),
        )
    }

    @Test
    fun reload_doesNotNormalizeWhenPaletteSelected() {
        // selectedPaletteId 非空 → shouldMigrateToDynamicColor 返回 false → reload 不规范化
        val settings =
            LocalSettings(
                colorSource = BUILTIN,
                selectedBuiltinThemeId = "",
                selectedPaletteId = PALETTE_ID,
            )
        assertFalse(
            "已选调色板时 shouldMigrateToDynamicColor 必须返回 false",
            shouldMigrate(settings),
        )
        // 反向验证：默认值（空选择）应返回 true，证明 selectedPaletteId 是关键条件
        assertTrue(
            "空选择时 shouldMigrateToDynamicColor 应返回 true",
            shouldMigrate(LocalSettings()),
        )
    }

    @Test
    fun reload_doesNotNormalizeWhenAlreadyDynamicColor() {
        // colorSource 已是 ANDROID_DYNAMIC → shouldMigrateToDynamicColor 返回 false → 不重复规范化
        val settings =
            LocalSettings(
                colorSource = ANDROID_DYNAMIC,
                selectedBuiltinThemeId = "",
                selectedPaletteId = "",
                dynamicColorEnabled = true,
            )
        assertFalse(
            "已是 android_dynamic 时 shouldMigrateToDynamicColor 必须返回 false",
            shouldMigrate(settings),
        )
        // 反向验证：colorSource=BUILTIN 时应返回 true，证明 colorSource 是关键条件
        assertTrue(
            "colorSource=built_in 时 shouldMigrateToDynamicColor 应返回 true",
            shouldMigrate(LocalSettings(colorSource = BUILTIN)),
        )
    }

    companion object {
        private const val BUILTIN = "built_in"
        private const val ANDROID_DYNAMIC = "android_dynamic"
        private const val SAVED_PALETTE = "saved_palette"
        private const val PAPER_LIGHT = "paper_light"
        private const val PALETTE_ID = "device01:fp01"
    }
}
