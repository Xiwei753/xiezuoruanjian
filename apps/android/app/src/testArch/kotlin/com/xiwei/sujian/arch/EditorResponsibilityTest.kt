package com.xiwei.sujian.arch

import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * editor/v2 子模块职责边界约束测试。
 *
 * 规则：
 * 3. editor/v2/input 只产生输入操作。
 * 4. editor/v2/visual 与 motion 只处理显示和动画状态。
 * 5. visual/motion 不写 Repository、章节正文和持久 Session 状态。
 *
 * 当前代码库存在既有违规的测试用 @Ignore 标记，并在注释中列出违规文件。
 */
class EditorResponsibilityTest {

    private val sourceRoot = ArchTestSupport.appSourceRoot

    // ------------------------------------------------------------------
    // 规则 3：editor/v2/input 只产生输入操作
    // ------------------------------------------------------------------

    /**
     * input 子模块不应直接引用 Repository / Bridge / data 层。
     * input 只产生输入操作（InputCommandPort），不应直接持久化或调用业务。
     */
    @Test
    fun `input does not reference repository or bridge`() {
        val violations = ArchTestSupport.findViolations(
            sourceRoot,
            pathFilter = "/editor/v2/input/",
            forbiddenReferences = listOf(
                "com.xiwei.sujian.data",
                "com.xiwei.sujian.workspace",
            )
        )
        assertTrue(
            "editor/v2/input 不应引用 Repository/Bridge/data 层。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty()
        )
    }

    /**
     * input 子模块不应直接依赖 Compose UI / Activity。
     */
    @Test
    fun `input does not depend on compose ui or activity`() {
        val violations = ArchTestSupport.findViolations(
            sourceRoot,
            pathFilter = "/editor/v2/input/",
            forbiddenReferences = listOf(
                "androidx.compose.ui",
                "androidx.compose.material3",
                "androidx.compose.foundation",
                "androidx.activity",
            )
        )
        assertTrue(
            "editor/v2/input 不应依赖 Compose UI/Activity。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty()
        )
    }

    /**
     * input 子模块不应直接引用 UniFFI 生成绑定（除 EditorTransactionCauseDto）。
     *
     * EditorTransactionCauseDto 是 input→pipeline 交互的必要契约，
     * pipeline 层（EditorCommandPort）接口签名使用此类型，input 无法避免引用。
     * 其他 UniFFI 类型（如 DTO 数据类、Bridge 类等）仍被禁止。
     */
    @Test
    fun `input does not directly reference uniffi bindings`() {
        val inputFiles = ArchTestSupport.collectKotlinFiles(sourceRoot, "/editor/v2/input/")
        // 允许 EditorTransactionCauseDto（pipeline 接口契约），禁止其他 UniFFI 引用。
        val allowedFqns = listOf("uniffi.writer_core.EditorTransactionCauseDto")
        val violations = mutableMapOf<java.io.File, List<String>>()
        for (file in inputFiles) {
            val lineViolations = ArchTestSupport.findForbiddenPrefixRefs(file, "uniffi.writer_core", allowedFqns)
            if (lineViolations.isNotEmpty()) {
                violations[file] = lineViolations.values.toList()
            }
        }
        assertTrue(
            "editor/v2/input 不应直接引用 UniFFI 绑定（EditorTransactionCauseDto 除外）。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty()
        )
    }

    // ------------------------------------------------------------------
    // 规则 4 & 5：visual/motion 只处理显示和动画状态，不写 Repository/章节正文/持久 Session
    // ------------------------------------------------------------------

    /**
     * visual/motion 子模块不应引用 Repository / Bridge / data 层。
     */
    @Test
    fun `visual and motion do not reference repository or bridge`() {
        val visualViolations = ArchTestSupport.findViolations(
            sourceRoot,
            pathFilter = "/editor/v2/visual/",
            forbiddenReferences = listOf(
                "com.xiwei.sujian.data",
                "com.xiwei.sujian.workspace",
            )
        )
        val motionViolations = ArchTestSupport.findViolations(
            sourceRoot,
            pathFilter = "/editor/v2/motion/",
            forbiddenReferences = listOf(
                "com.xiwei.sujian.data",
                "com.xiwei.sujian.workspace",
            )
        )
        val all = visualViolations + motionViolations
        assertTrue(
            "editor/v2/visual 与 motion 不应引用 Repository/Bridge/data 层。违规:\n${ArchTestSupport.formatViolations(all)}",
            all.isEmpty()
        )
    }

    /**
     * visual/motion 不应直接依赖 Activity / View / input 基础设施。
     */
    @Test
    fun `visual and motion do not depend on activity view or input`() {
        val forbidden = listOf(
            "androidx.activity",
            "android.view",
            "com.xiwei.sujian.editor.v2.input",
        )
        val visualViolations = ArchTestSupport.findViolations(
            sourceRoot, "/editor/v2/visual/", forbidden
        )
        val motionViolations = ArchTestSupport.findViolations(
            sourceRoot, "/editor/v2/motion/", forbidden
        )
        val all = visualViolations + motionViolations
        assertTrue(
            "visual/motion 不应依赖 Activity/View/input。违规:\n${ArchTestSupport.formatViolations(all)}",
            all.isEmpty()
        )
    }

    /**
     * visual 子模块不应直接引用 UniFFI 生成绑定（除 DTO 契约类型）。
     *
     * EditorOperationKindDto 和 AnimationModeDto 是 visual→pipeline/mirror 交互的必要契约，
     * 这些类型的接口签名在 pipeline 层定义，visual 无法避免引用。
     * 其他 UniFFI 类型（如 Bridge 类、非 DTO 数据类等）仍被禁止。
     */
    @Test
    fun `visual does not directly reference uniffi bindings`() {
        val visualFiles = ArchTestSupport.collectKotlinFiles(sourceRoot, "/editor/v2/visual/")
        val allowedFqns = listOf(
            "uniffi.writer_core.EditorOperationKindDto",
            "uniffi.writer_core.AnimationModeDto",
        )
        val violations = mutableMapOf<java.io.File, List<String>>()
        for (file in visualFiles) {
            val lineViolations = ArchTestSupport.findForbiddenPrefixRefs(file, "uniffi.writer_core", allowedFqns)
            if (lineViolations.isNotEmpty()) {
                violations[file] = lineViolations.values.toList()
            }
        }
        assertTrue(
            "editor/v2/visual 不应直接引用 UniFFI 绑定（DTO 契约类型除外）。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty()
        )
    }

    /**
     * motion 子模块不应直接引用 UniFFI 生成绑定。
     */
    @Test
    fun `motion does not directly reference uniffi bindings`() {
        val violations = ArchTestSupport.findViolations(
            sourceRoot,
            pathFilter = "/editor/v2/motion/",
            forbiddenReferences = listOf("uniffi.writer_core"),
        )
        assertTrue(
            "editor/v2/motion 不应直接引用 UniFFI 绑定。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty()
        )
    }

    /**
     * motion 子模块不应依赖 Compose UI 框架（material3/foundation/ui）。
     *
     * 注意：`androidx.compose.runtime.Immutable` 是编译器标记注解，
     * 用于帮助 Compose 编译器推断不可变性，不属于 UI 框架依赖，单独放行。
     *
     * 既有违规（@Ignore）：motion 使用 `@Immutable` 注解。
     *  - editor/v2/motion/EditorMotionPolicy.kt
     *  - editor/v2/motion/TargetMotionConstraint.kt
     *  - editor/v2/motion/VisualTrackState.kt
     */
    @Test
    fun `motion does not depend on compose ui framework`() {
        val motionFiles = ArchTestSupport.collectKotlinFiles(sourceRoot, "/editor/v2/motion/")
        // 允许 androidx.compose.runtime.Immutable/@Immutable 注解，禁止其他 Compose UI 依赖。
        val forbidden = listOf(
            "androidx.compose.ui",
            "androidx.compose.material3",
            "androidx.compose.foundation",
            "androidx.compose.animation",
        )
        val violations = ArchTestSupport.findViolationsIn(motionFiles, forbidden)
        assertTrue(
            "editor/v2/motion 不应依赖 Compose UI 框架。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty()
        )
    }

    /**
     * visual 子模块不应依赖 Compose UI 框架（material3/foundation/ui）。
     * visual 只处理显示状态计算，不应直接渲染 Composable。
     */
    @Test
    fun `visual does not depend on compose ui framework`() {
        val visualFiles = ArchTestSupport.collectKotlinFiles(sourceRoot, "/editor/v2/visual/")
        val forbidden = listOf(
            "androidx.compose.ui",
            "androidx.compose.material3",
            "androidx.compose.foundation",
            "androidx.compose.animation",
        )
        val violations = ArchTestSupport.findViolationsIn(visualFiles, forbidden)
        assertTrue(
            "editor/v2/visual 不应依赖 Compose UI 框架。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty()
        )
    }
}
