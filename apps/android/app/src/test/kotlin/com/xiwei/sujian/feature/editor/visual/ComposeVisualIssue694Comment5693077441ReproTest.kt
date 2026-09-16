package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #694 评论 5693077441 复现测试 — 上一轮修完 5692161955 后还剩的两个问题：
 *
 * 问题1：Android 调用方仍传整章正文给 Core。
 *   ComposeLocalVisualRebase.classifyLocalVisualPlanFromCore 把整章 oldText/newText 都传给 Core，
 *   导致长正文（含换行、> 8 cluster）末尾输入少量文字时，按整章判定得到 LineReflow/Run 动画，
 *   而非按本次 affected text 判定得到 Glyph 动画。
 *
 * 问题2：ZWJ fallback 会把 emoji 后面的普通文字一起吞进 cluster。
 *   splitGraphemeClusterRangesWithZwjMerge 旧逻辑用"group 曾经包含过 ZWJ"
 *   （currentContainsZwj 一旦 true 永远 true），把 `👨‍👩‍👧‍👦a` 结尾的 a 合进 emoji family。
 *
 * 本测试在当前实现下应 **FAIL**，体现 2 个 bug。修复后应 PASS。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength")
class ComposeVisualIssue694Comment5693077441ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 问题1：长文输入场景（评论明确要求） ====================

    /**
     * 问题1 核心复现：已有几十字且包含换行的正文，在末尾一次输入"我们"，
     * 应按本次 affected text（"我们"）判定，得 GlyphAnimation + 2 个 cluster，
     * 而非因整章含换行被判成 LineReflowAnimation 或因整章 > 8 cluster 被判成 RunAnimation。
     *
     * 当前实现（修复前）：classifyLocalVisualPlanFromCore 把整章 oldText/newText 都传给 Core，
     * Core 按整章判定（含换行 -> LineReflow，或 > 8 cluster -> Run），动画粒度错误。FAIL。
     * 修复后：只传 affected slice（"我们"），Core 按 2 cluster 判定得 GlyphAnimation + 2 unit。PASS。
     */
    @Test
    fun longTextWithNewlineAppendWe_shouldClassifyByAffectedTextNotWholeChapter() {
        // #694 评论 5693077441 问题1 回归：
        // 已有几十字且包含换行的正文，末尾一次输入"我们"，
        // 应按本次 affected text（"我们"）判定，得 GlyphAnimation + 2 个 cluster，
        // 而非因整章含换行被判成 LineReflowAnimation 或因整章 > 8 cluster 被判成 RunAnimation。
        val oldText = "第一章\n\n这是正文的第一段，已经有不少字了。\n\n第二段也有些内容。"
        val newText = oldText + "我们"
        val layouts = captureLayouts(oldText, newText)
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5693077441-p1",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(oldText.length, oldText.length), 0)
        state.recordLocalInput(
            oldText = oldText,
            newText = newText,
            oldSelection = TextRange(oldText.length, oldText.length),
            newSelection = TextRange(newText.length, newText.length),
            changes =
                listOf(
                    LocalInputChange(
                        newRange = TextRange(oldText.length, newText.length),
                        oldRange = TextRange(oldText.length, oldText.length),
                    ),
                ),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(newText.length, newText.length), 0, compositionActive = false)

        val patch = state.latestPatch.value
        assertNotNull("本地输入 patch 应生成", patch)
        assertEquals(
            "问题1：长正文含换行，末尾输入\"我们\"应按 affected text 判定为 GlyphAnimation，" +
                "不应因整章含换行被判成 LineReflowAnimation 或因整章 > 8 cluster 被判成 RunAnimation",
            AnimationModeDto.GLYPH_ANIMATION,
            patch!!.animationMode,
        )
        assertTrue(
            "问题1：应按\"我们\"的 2 个 grapheme cluster 拆成 2 个 insertedUnits，" +
                "实际=${patch.insertedUnits}（size=${patch.insertedUnits.size}）",
            patch.insertedUnits.size == 2,
        )
    }

    /**
     * 问题1 补充：长文末尾输入单个字，应按 affected text 判定得 GlyphAnimation + 1 unit。
     */
    @Test
    fun longTextWithNewlineAppendSingleChar_shouldUseGlyphAnimation() {
        val oldText = "标题\n\n正文内容已经有很多字了，超过八个 grapheme cluster。"
        val newText = oldText + "我"
        val layouts = captureLayouts(oldText, newText)
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5693077441-p2",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(oldText.length, oldText.length), 0)
        state.recordLocalInput(
            oldText = oldText,
            newText = newText,
            oldSelection = TextRange(oldText.length, oldText.length),
            newSelection = TextRange(newText.length, newText.length),
            changes =
                listOf(
                    LocalInputChange(
                        newRange = TextRange(oldText.length, newText.length),
                        oldRange = TextRange(oldText.length, oldText.length),
                    ),
                ),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(newText.length, newText.length), 0, compositionActive = false)

        val patch = state.latestPatch.value
        assertNotNull("本地输入 patch 应生成", patch)
        assertEquals(
            "长正文含换行，末尾输入单个\"我\"应按 affected text 判定为 GlyphAnimation",
            AnimationModeDto.GLYPH_ANIMATION,
            patch!!.animationMode,
        )
        assertTrue(
            "应得 1 个 insertedUnit，实际 size=${patch.insertedUnits.size}",
            patch.insertedUnits.size == 1,
        )
    }

    // ==================== 问题2：ZWJ fallback 把 emoji 后普通文字吞进 cluster ====================

    /**
     * 问题2 核心复现：`👨‍👩‍👧‍👦a` 应是 2 个 grapheme unit（emoji family 1 个 + a 1 个），
     * 不能把 a 合进 emoji family。
     *
     * 当前实现（修复前）：splitGraphemeClusterRangesWithZwjMerge 旧逻辑用
     * "group 曾经包含过 ZWJ"（currentContainsZwj 一旦 true 永远 true），
     * 把 `👨‍👩‍👧‍👦a` 结尾的 a 合进 emoji family，得到 1 个 unit。FAIL。
     * 修复后：ZWJ 合并改成"当前 segment 是 ZWJ 或上一 cluster 以 ZWJ 结尾才合并下一段"，
     * 得到 [emoji family] + [a] = 2 个 unit。PASS。
     */
    @Test
    fun emojiFamilyFollowedByNormalChar_shouldBeTwoGraphemeUnits() {
        // #694 评论 5693077441 问题2 回归：
        // 👨‍👩‍👧‍👦a 应是 2 个 grapheme unit（emoji family 1 个 + a 1 个），
        // 不能把 a 合进 emoji family。
        val emojiFamilyAndChar = "👨‍👩‍👧‍👦a"
        val layouts = captureLayouts("", emojiFamilyAndChar)
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5693077441-p3",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.recordLocalInput(
            oldText = "",
            newText = emojiFamilyAndChar,
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(emojiFamilyAndChar.length, emojiFamilyAndChar.length),
            changes =
                listOf(
                    LocalInputChange(
                        newRange = TextRange(0, emojiFamilyAndChar.length),
                        oldRange = TextRange(0, 0),
                    ),
                ),
        )
        state.onAuthoritativeLayout(
            layouts[1],
            TextRange(emojiFamilyAndChar.length, emojiFamilyAndChar.length),
            0,
            compositionActive = false,
        )

        val patch = state.latestPatch.value
        assertNotNull("本地输入 patch 应生成", patch)
        assertTrue(
            "问题2：👨‍👩‍👧‍👦a 期望 2 个 grapheme unit（emoji family 1 个 + a 1 个），" +
                "实际=${patch!!.insertedUnits}（size=${patch.insertedUnits.size}）\n" +
                "ZWJ 合并不能把 a 合进 emoji family",
            patch.insertedUnits.size == 2,
        )
    }

    // ==================== 辅助方法 ====================

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = 14f.sp),
                        constraints = Constraints(maxWidth = 1000),
                    ),
                )
            }
        }
        return results
    }
}
