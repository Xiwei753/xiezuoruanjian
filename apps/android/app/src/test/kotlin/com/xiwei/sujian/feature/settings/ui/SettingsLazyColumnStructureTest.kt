package com.xiwei.sujian.feature.settings.ui

import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #633 评论 5379618506：设置页 Column+verticalScroll 结构测试。
 *
 * 旧的 LazyColumn/item/key/contentType 结构已替换为 Column+verticalScroll。
 * 一个 category = 一个 SettingsExpandableCategory（独占 Transition+AnimatedVisibility）。
 *
 * 验证：
 * - SettingsRoute 使用 rememberScrollState + verticalScroll（不再用 LazyColumn）；
 * - Settings 导航使用 noPageTransitionMetadata（无双页 crossfade）。
 */
class SettingsLazyColumnStructureTest {
    // ── SettingsRoute JankStats interaction markers（评论25 项2） ──

    @Test
    fun settingsRoute_observesScrollInProgressForInteraction() {
        val source = settingsRouteSource()
        assertTrue(
            "SettingsRoute must observe scrollState.isScrollInProgress",
            source.contains("scrollState.isScrollInProgress") &&
                source.contains("settings_scroll"),
        )
    }

    @Test
    fun settingsRoute_usesColumnWithVerticalScroll() {
        val source = settingsRouteSource()
        assertTrue(
            "SettingsRoute must use rememberScrollState",
            source.contains("rememberScrollState"),
        )
        assertTrue(
            "SettingsRoute must use verticalScroll",
            source.contains("verticalScroll"),
        )
    }

    @Test
    fun settingsRoute_doesNotUseLazyColumn() {
        val source = settingsRouteSource()
        assertTrue(
            "SettingsRoute must not use LazyColumn",
            !source.contains("LazyColumn"),
        )
        assertTrue(
            "SettingsRoute must not use rememberLazyListState",
            !source.contains("rememberLazyListState"),
        )
    }

    @Test
    fun settingsRoute_screenMetricsStateSetToSettings() {
        val navSuiteSource =
            java.io.File(
                "src/main/kotlin/com/xiwei/sujian/app/navigation/SujianNavigationSuite.kt",
            ).readText()
        assertTrue(
            "SujianNavigationSuite must set screen=Settings for Settings route",
            navSuiteSource.contains("resolveScreenState") &&
                navSuiteSource.contains("\"Settings\""),
        )
    }

    private fun settingsRouteSource(): String =
        java.io.File(
            "src/main/kotlin/com/xiwei/sujian/feature/settings/ui/SettingsRoute.kt",
        ).readText()

    // ── 8 个设置内容文件应提供 @Composable XxxSettingsContent(vm) ──

    @Test
    fun appearanceSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AppearanceSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "AppearanceSettingsContent" }
        assertTrue("AppearanceSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun editorSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.EditorSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "EditorSettingsContent" }
        assertTrue("EditorSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun saveSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SaveSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "SaveSettingsContent" }
        assertTrue("SaveSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun syncSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SyncSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "SyncSettingsContent" }
        assertTrue("SyncSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun aiSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AiSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "AiSettingsContent" }
        assertTrue("AiSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun diagnosticsSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.DiagnosticsSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "DiagnosticsSettingsContent" }
        assertTrue("DiagnosticsSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun laboratorySettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.LaboratorySettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "LaboratorySettingsContent" }
        assertTrue("LaboratorySettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun aboutSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AboutSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "AboutSettingsContent" }
        assertTrue("AboutSettingsContent 函数应存在", hasFunction)
    }

    // ── Settings 导航使用 noPageTransitionMetadata（无双页 crossfade） ──

    @Test
    fun settingsRoute_hasNoPageTransitionMetadata() {
        val source =
            java.io.File(
                "src/main/kotlin/com/xiwei/sujian/app/navigation/SujianNavigationSuite.kt",
            ).readText()
        assertTrue(
            "Settings NavEntry must use noPageTransitionMetadata",
            source.contains("SujianRoute.Settings") &&
                source.contains("noPageTransitionMetadata"),
        )
    }
}
