package com.xiwei.sujian.feature.editor.layout

import com.xiwei.sujian.feature.settings.ui.AppearanceSectionState
import com.xiwei.sujian.feature.settings.ui.EditorSectionState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #723 评论 5748592923 回归测试（修复后反转断言）。
 *
 * 本测试在修复后断言 issue 描述的四个问题结构已从当前代码中消除：
 * 1) 字号/行距入口已移回写作区设置（AppearanceSectionState 不再持有 fontSize/lineSpacing，
 *    EditorSectionState 持有 lineSpacing/coordinatedAnimationEnabled）
 * 2) 协同动画设置模型已建立（EditorSectionState 持有 coordinatedAnimationEnabled）
 * 3) 换行光标映射已有 caret 专用入口（EditorSoftBreakProjection 有 wedgeStart/wedgeEnd/CaretAffinity，
 *    wedgeStart 用 insertPoint<rawOffset 计数，wedgeEnd 用 insertPoint<=rawOffset 计数）
 * 4) 文末空段落 caret-only 特判已删除（无 isIndentedEmptyParagraphCaret，
 *    无 isIndentedEmptyParagraphCaretFromTextStyle）
 *
 * 诊断包证据：同一份 51 长度布局，selectionEnd=51 自绘 caret y=287，
 * selectionEnd=50 y=212（字号 16sp、行距 1.5、首行缩进 2 字符）。
 * 修复后 caret 专用 wedgeStart/wedgeEnd + CaretAffinity.Start 让本地输入产生的
 * collapsed caret 使用 Start affinity，不再统一走 wedge End 导致跳行错位。
 */
@Suppress("TooManyFunctions")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class Issue723ReproductionTest {
    private companion object {
        const val LINE_SPACING_FIELD = "lineSpacing"
        const val WEDGE_START_METHOD = "wedgeStart"
    }

    // ===== 问题 1：字号/行距入口已移回写作区设置 =====

    /**
     * AppearanceSectionState 不再持有 fontSize 字段 —— 字号入口已移回编辑器页。
     */
    @Test
    fun problem1_appearanceSectionState_noFontSize() {
        val field = AppearanceSectionState::class.java.declaredFields.firstOrNull { it.name == "fontSize" }
        assertNull(
            "AppearanceSectionState 不应再持有 fontSize 字段（字号入口已移回编辑器页）",
            field,
        )
    }

    /**
     * AppearanceSectionState 不再持有 lineSpacing 字段 —— 行距入口已移回编辑器页。
     */
    @Test
    fun problem1_appearanceSectionState_noLineSpacing() {
        val field = AppearanceSectionState::class.java.declaredFields.firstOrNull { it.name == LINE_SPACING_FIELD }
        assertNull(
            "AppearanceSectionState 不应再持有 lineSpacing 字段（行距入口已移回编辑器页）",
            field,
        )
    }

    /**
     * EditorSectionState 持有 lineSpacing 字段 —— 编辑器设置页现在有行距入口。
     */
    @Test
    fun problem1_editorSectionState_hasLineSpacing() {
        val field = EditorSectionState::class.java.declaredFields.firstOrNull { it.name == LINE_SPACING_FIELD }
        assertNotNull(
            "EditorSectionState 应持有 lineSpacing 字段（编辑器页现在有行距入口）",
            field,
        )
    }

    // ===== 问题 2：协同动画设置模型已建立 =====

    /**
     * EditorSectionState 持有 coordinatedAnimationEnabled 字段 ——
     * 协同动画（吞字/吐字）开关已进入编辑器设置状态模型。
     */
    @Test
    fun problem2_editorSectionState_hasCoordinatedAnimationEnabled() {
        val field =
            EditorSectionState::class.java.declaredFields.firstOrNull {
                it.name == "coordinatedAnimationEnabled"
            }
        assertNotNull(
            "EditorSectionState 应持有 coordinatedAnimationEnabled 字段（协同动画设置模型已建立）",
            field,
        )
    }

    // ===== 问题 3：换行光标映射已有 caret 专用入口 =====

    /**
     * EditorSoftBreakProjection 有 wedgeStart 方法 ——
     * caret 专用 wedge Start 映射入口已建立。
     */
    @Test
    fun problem3_softBreakProjection_hasWedgeStart() {
        val method =
            EditorSoftBreakProjection::class.java.declaredMethods.firstOrNull {
                it.name == WEDGE_START_METHOD
            }
        assertNotNull(
            "EditorSoftBreakProjection 应有 wedgeStart 方法（caret 专用 wedge Start 映射）",
            method,
        )
    }

    /**
     * EditorSoftBreakProjection 有 wedgeEnd 方法 ——
     * caret 专用 wedge End 映射入口已建立。
     */
    @Test
    fun problem3_softBreakProjection_hasWedgeEnd() {
        val method =
            EditorSoftBreakProjection::class.java.declaredMethods.firstOrNull {
                it.name == "wedgeEnd"
            }
        assertNotNull(
            "EditorSoftBreakProjection 应有 wedgeEnd 方法（caret 专用 wedge End 映射）",
            method,
        )
    }

    /**
     * feature/editor/layout 包下有 CaretAffinity 类型 ——
     * caret affinity 枚举已建立，caret 映射显式选 start/end。
     *
     * CaretAffinity 是 [EditorSoftBreakProjection] 的嵌套枚举，
     * 全限定名为 `EditorSoftBreakProjection$CaretAffinity`。
     */
    @Test
    fun problem3_hasCaretAffinityType() {
        val className = "com.xiwei.sujian.feature.editor.layout.EditorSoftBreakProjection\$CaretAffinity"
        val clazz =
            try {
                Class.forName(className)
            } catch (_: ClassNotFoundException) {
                null
            }
        assertNotNull(
            "应存在 CaretAffinity 类型（caret affinity 区分已建立）",
            clazz,
        )
    }

    // ===== 问题 3：wedgeStart/wedgeEnd 行为断言 =====

    /**
     * wedgeStart 在 insertPoint 处使用 insertPoint < rawOffset 计数（wedge Start affinity）。
     *
     * 构造 insertPoints=[5]（在 raw offset 5 前插入 U+200B）：
     * - wedgeStart(5) = 5 + countInsertsBefore(5) = 5 + 0 = 5
     *   （因为 insertPoints[0]=5 < 5 为 false，不计入 → wedge Start：caret 落在 U+200B 之前）
     * - wedgeEnd(5) = 5 + countInsertsUpTo(5) = 5 + 1 = 6
     *   （wedge End：caret 落在 U+200B 之后）
     *
     * 修复后 caret 专用映射区分 Start/End，本地输入产生的 collapsed caret 使用 Start，
     * 与 AndroidX 文本编辑后的 wedge affinity 一致。
     */
    @Test
    fun problem3_wedgeStart_wedgeEnd_affinityBehavior() {
        val projection = EditorSoftBreakProjection(rawLength = 10, insertPoints = listOf(5))
        assertEquals(11, projection.displayLength)

        // wedgeStart(5) = 5 + 0 = 5（wedge Start：insertPoint 5 < 5 为 false，不计入）
        val wedgeStartAtInsertPoint = projection.wedgeStart(5)
        assertEquals(
            "wedgeStart(5) 应为 5（wedge Start affinity：insertPoint 5<5 为 false 不计入），" +
                "caret 落在 U+200B 之前",
            5,
            wedgeStartAtInsertPoint,
        )

        // wedgeEnd(5) = 5 + 1 = 6（wedge End：insertPoint 5 <= 5 计入）
        val wedgeEndAtInsertPoint = projection.wedgeEnd(5)
        assertEquals(
            "wedgeEnd(5) 应为 6（wedge End affinity：insertPoint 5<=5 计入），" +
                "caret 落在 U+200B 之后",
            6,
            wedgeEndAtInsertPoint,
        )

        // rawToDisplay 保留为 range 映射（wedge End 语义）
        assertEquals(6, projection.rawToDisplay(5))
        assertEquals(4, projection.rawToDisplay(4))
        assertEquals(7, projection.rawToDisplay(6))

        // 关键：wedgeStart(5) != wedgeEnd(5)，caret 专用映射区分 affinity
        assertFalse(
            "wedgeStart(5) != wedgeEnd(5)，caret 专用映射区分 Start/End affinity",
            wedgeStartAtInsertPoint == wedgeEndAtInsertPoint,
        )
    }

    /**
     * 诊断包证据对应的边界场景：修复后 wedgeStart/wedgeEnd 区分 affinity。
     *
     * 诊断包：selectionEnd=51 自绘 caret y=287，selectionEnd=50 y=212。
     * 修复前 rawToDisplay 统一走 wedge End，无法区分 collapsed caret 的 Start/End affinity，
     * 导致跳行错位 75px。修复后 wedgeStart/wedgeEnd 提供不同映射值，
     * 本地输入产生的 collapsed caret 使用 Start affinity。
     */
    @Test
    fun problem3_diagnosticEvidence_wedgeStartWedgeEndDistinguished() {
        val projection = EditorSoftBreakProjection(rawLength = 51, insertPoints = listOf(50))
        assertEquals(52, projection.displayLength)

        // wedgeStart(50) = 50 + 0 = 50（wedge Start：50 < 50 为 false）
        val wedgeStartAt50 = projection.wedgeStart(50)
        assertEquals(50, wedgeStartAt50)

        // wedgeEnd(50) = 50 + 1 = 51（wedge End：50 <= 50 计入）
        val wedgeEndAt50 = projection.wedgeEnd(50)
        assertEquals(51, wedgeEndAt50)

        // wedgeStart(51) = 51 + 1 = 52（wedge Start：50 < 51 计入）
        val wedgeStartAt51 = projection.wedgeStart(51)
        assertEquals(52, wedgeStartAt51)

        // wedgeEnd(51) = 51 + 1 = 52（wedge End：50 <= 51 计入）
        val wedgeEndAt51 = projection.wedgeEnd(51)
        assertEquals(52, wedgeEndAt51)

        // 修复后 wedgeStart/wedgeEnd 入口存在，caret 专用映射区分 affinity
        val wedgeStartMethod =
            EditorSoftBreakProjection::class.java.declaredMethods.firstOrNull {
                it.name == WEDGE_START_METHOD
            }
        assertNotNull(
            "修复后应有 wedgeStart 入口，caret 专用映射区分 affinity，" +
                "解决诊断包证据（y=287 vs y=212 跳行错位）",
            wedgeStartMethod,
        )
    }

    // ===== 问题 4：文末空段落 caret-only 特判已删除 =====

    /**
     * ComposeLayoutSnapshot 不再有 isIndentedEmptyParagraphCaret 扩展函数 ——
     * 空段落 caret-only 缩进特判已删除，缩进进入显示布局本身。
     */
    @Test
    fun problem4_composeLayoutSnapshot_noIndentedEmptyParagraphCaretPredicate() {
        val method =
            try {
                Class.forName("com.xiwei.sujian.feature.editor.layout.EditorLayoutStateKt")
                    .declaredMethods.firstOrNull { it.name == "isIndentedEmptyParagraphCaret" }
            } catch (_: ClassNotFoundException) {
                null
            }
        assertNull(
            "EditorLayoutStateKt 不应有 isIndentedEmptyParagraphCaret 方法" +
                "（空段落 caret-only 缩进特判已删除）",
            method,
        )
    }

    /**
     * WritingEditorSurface 不再有 isIndentedEmptyParagraphCaretFromTextStyle 函数 ——
     * 空段落缩进进入显示布局本身，不再把系统 caret 透明掉。
     */
    @Test
    fun problem4_writingEditorSurface_noIndentedEmptyParagraphCaretFromTextStyle() {
        val method =
            try {
                Class.forName("com.xiwei.sujian.feature.editor.ui.WritingEditorSurfaceKt")
                    .declaredMethods.firstOrNull {
                        it.name == "isIndentedEmptyParagraphCaretFromTextStyle"
                    }
            } catch (_: ClassNotFoundException) {
                null
            }
        assertNull(
            "WritingEditorSurfaceKt 不应有 isIndentedEmptyParagraphCaretFromTextStyle 方法" +
                "（空段落缩进进入显示布局本身，不再透明系统 caret）",
            method,
        )
    }

    // ===== 综合修复确认 =====

    /**
     * 综合断言：四个问题结构全部已从当前代码中消除。
     * 诊断包已提供运行时证据（y=287 vs y=212），本测试确认代码结构层面的问题已消除。
     */
    @Test
    fun allFourProblems_eliminatedFromCurrentCode() {
        // 问题 1：字号/行距已移回编辑器页
        val appearanceNoFontSize =
            AppearanceSectionState::class.java.declaredFields.none { it.name == "fontSize" }
        val appearanceNoLineSpacing =
            AppearanceSectionState::class.java.declaredFields.none { it.name == LINE_SPACING_FIELD }
        val editorHasLineSpacing =
            EditorSectionState::class.java.declaredFields.any { it.name == LINE_SPACING_FIELD }
        assertTrue("问题1：字号不在外观页", appearanceNoFontSize)
        assertTrue("问题1：行距不在外观页", appearanceNoLineSpacing)
        assertTrue("问题1：编辑器页有行距", editorHasLineSpacing)

        // 问题 2：协同动画设置模型已建立
        val editorHasCoordinated =
            EditorSectionState::class.java.declaredFields.any {
                it.name == "coordinatedAnimationEnabled"
            }
        assertTrue("问题2：编辑器页有协同动画开关", editorHasCoordinated)

        // 问题 3：有 caret 专用映射
        val hasWedgeStart =
            EditorSoftBreakProjection::class.java.declaredMethods.any { it.name == WEDGE_START_METHOD }
        val hasWedgeEnd =
            EditorSoftBreakProjection::class.java.declaredMethods.any { it.name == "wedgeEnd" }
        assertTrue("问题3：有 wedgeStart", hasWedgeStart)
        assertTrue("问题3：有 wedgeEnd", hasWedgeEnd)

        // 问题 4：空段落 caret-only 特判已删除
        val noIndentedPredicate =
            try {
                Class.forName("com.xiwei.sujian.feature.editor.layout.EditorLayoutStateKt")
                    .declaredMethods.none { it.name == "isIndentedEmptyParagraphCaret" }
            } catch (_: ClassNotFoundException) {
                true
            }
        assertTrue("问题4：空段落 caret-only 特判已删除", noIndentedPredicate)
    }
}
