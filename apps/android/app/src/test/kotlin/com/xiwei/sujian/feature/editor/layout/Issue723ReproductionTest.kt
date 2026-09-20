package com.xiwei.sujian.feature.editor.layout

import com.xiwei.sujian.feature.settings.ui.AppearanceSectionState
import com.xiwei.sujian.feature.settings.ui.EditorSectionState
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
 * 3) 换行光标映射已统一为 rawToDisplay（Issue #725 评论 5750735497 停止自绘屏幕 caret 后，
 *    wedgeStart/wedgeEnd/CaretAffinity 已删除，caret 在 U+200B wedge 的哪一侧只由 BasicTextField 自己决定）
 * 4) 文末空段落 caret-only 特判已删除（无 isIndentedEmptyParagraphCaret，
 *    无 isIndentedEmptyParagraphCaretFromTextStyle）
 */
@Suppress("TooManyFunctions")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class Issue723ReproductionTest {
    private companion object {
        const val LINE_SPACING_FIELD = "lineSpacing"
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
     *
     * Issue #725 评论 5750735497：停止自绘屏幕 caret 后，wedgeStart/wedgeEnd/CaretAffinity
     * 已删除，问题3 的"caret 专用映射入口"断言不再适用，本测试只确认问题1/2/4 结构层面已消除。
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
