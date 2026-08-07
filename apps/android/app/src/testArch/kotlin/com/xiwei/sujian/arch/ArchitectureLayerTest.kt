package com.xiwei.sujian.arch

import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 分层依赖规则约束测试（对应 AGENTS.md 跨平台边界）。
 *
 * 规则：
 * 1. Compose/UI 层通过 ViewModel/协调器调用业务，不直接调用 UniFFI、JNA 和具体 Bridge。
 * 2. Bridge/Repository 层不依赖 Compose、Activity、View 和页面状态。
 *
 * 当前代码库存在既有违规的测试用 @Ignore 标记，并在注释中列出违规文件，
 * 以便后续修复时移除 @Ignore 让测试重新生效。
 */
class ArchitectureLayerTest {
    private val sourceRoot = ArchTestSupport.appSourceRoot

    // ------------------------------------------------------------------
    // 规则 1：UI 层不直接调用 UniFFI / JNA / 具体 Bridge
    // ------------------------------------------------------------------

    /**
     * UI 层不应直接引用 UniFFI 生成绑定（`uniffi.writer_core.*`）。
     *
     * 既有违规（@Ignore）：UI 主题与设置路由直接使用了 UniFFI 的 DTO 类型，
     * 应通过 ViewModel/协调器或 app 层 DTO 间接访问。
     *  - ui/compose/theme/ThemeStore.kt
     *  - ui/compose/theme/ThemeUiState.kt
     *  - ui/compose/theme/ThemeController.kt
     *  - ui/compose/theme/SujianTheme.kt
     *  - ui/compose/settings/SettingsRoute.kt
     *  - ui/ThemePaletteHelper.kt
     */
    @Test
    fun `ui layer does not directly reference uniffi bindings`() {
        val violations =
            ArchTestSupport.findViolations(
                sourceRoot,
                pathFilter = "/ui/",
                forbiddenReferences = listOf("uniffi.writer_core"),
            )
        assertTrue(
            "UI 层不应直接引用 UniFFI 绑定。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty(),
        )
    }

    /**
     * UI 层不应直接引用 JNA（`com.sun.jna.*`）。
     */
    @Test
    fun `ui layer does not directly reference jna`() {
        val violations =
            ArchTestSupport.findViolations(
                sourceRoot,
                pathFilter = "/ui/",
                forbiddenReferences = listOf("com.sun.jna"),
            )
        assertTrue(
            "UI 层不应直接引用 JNA。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty(),
        )
    }

    /**
     * UI 层不应直接引用具体 Bridge 类（BridgeProvider / *Bridge / BridgeResult / BridgeMappers）。
     * ViewModel/协调器引用 Repository 是允许的，但直接引用 Bridge 基础设施违反分层。
     *
     * 既有违规（@Ignore）：多个 UI 文件直接 import BridgeProvider/BridgeResult。
     *  - ui/compose/SujianAppState.kt
     *  - ui/compose/workspace/WorkspaceViewModel.kt
     *  - ui/compose/starmap/StarMapRoute.kt
     *  - ui/compose/stats/StatsScreen.kt
     "ui/compose/starmap/StarMapViewModel.kt",
     *  - ui/compose/settings/SettingsRoute.kt
     */
    @Test
    fun `ui layer does not directly reference concrete bridge classes`() {
        val forbiddenBridgeClasses =
            listOf(
                "com.xiwei.sujian.data.BridgeProvider",
                "com.xiwei.sujian.data.BridgeMappers",
                "com.xiwei.sujian.data.WorkspaceBridge",
                "com.xiwei.sujian.data.ChapterBridge",
                "com.xiwei.sujian.data.ProjectBridge",
                "com.xiwei.sujian.data.SettingsBridge",
                "com.xiwei.sujian.data.StatsBridge",
                "com.xiwei.sujian.data.SyncBridge",
                "com.xiwei.sujian.data.WritingBridge",
                "com.xiwei.sujian.data.ActionBridge",
                "com.xiwei.sujian.data.AppServiceBridge",
                "com.xiwei.sujian.data.StarMapBridge",
                "com.xiwei.sujian.data.LayoutPolicyBridge",
                "com.xiwei.sujian.data.ScreenPolicyBridge",
            )
        val allViolations =
            ArchTestSupport.findViolations(
                sourceRoot,
                pathFilter = "/ui/",
                forbiddenReferences = forbiddenBridgeClasses,
            )
        val violations = allViolations
        assertTrue(
            "UI 层不应直接引用具体 Bridge 类。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty(),
        )
    }

    // ------------------------------------------------------------------
    // 规则 2：Bridge/Repository 层不依赖 Compose / Activity / View / 页面状态
    // ------------------------------------------------------------------

    /**
     * Bridge/Repository 层（data/）不应依赖 Compose 运行时与 UI。
     */
    @Test
    fun `bridge repository layer does not depend on compose`() {
        val violations =
            ArchTestSupport.findViolations(
                sourceRoot,
                pathFilter = "/data/",
                forbiddenReferences =
                    listOf(
                        "androidx.compose",
                        "androidx.activity",
                        "android.view",
                        "com.xiwei.sujian.ui",
                    ),
            )
        assertTrue(
            "Bridge/Repository 层不应依赖 Compose/Activity/View/UI。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty(),
        )
    }

    /**
     * Bridge/Repository 层（data/）不应依赖 editor/v2 的显示/动画状态。
     */
    @Test
    fun `bridge repository layer does not depend on editor display state`() {
        val violations =
            ArchTestSupport.findViolations(
                sourceRoot,
                pathFilter = "/data/",
                forbiddenReferences =
                    listOf(
                        "com.xiwei.sujian.editor.v2.visual",
                        "com.xiwei.sujian.editor.v2.motion",
                        "com.xiwei.sujian.editor.v2.compose",
                        "com.xiwei.sujian.editor.v2.render",
                    ),
            )
        assertTrue(
            "Bridge/Repository 层不应依赖 editor 显示/动画状态。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty(),
        )
    }

    /**
     * UI 层不应直接依赖 editor/v2/input 基础设施（input 是编辑器内部模块）。
     */
    @Test
    fun `ui layer does not directly depend on editor input infrastructure`() {
        val violations =
            ArchTestSupport.findViolations(
                sourceRoot,
                pathFilter = "/ui/",
                forbiddenReferences =
                    listOf(
                        "com.xiwei.sujian.editor.v2.input",
                    ),
            )
        assertTrue(
            "UI 层不应直接依赖 editor/v2/input 基础设施。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty(),
        )
    }
}
