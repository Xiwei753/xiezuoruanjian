package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #708 评论 5725146968 修复1 的验证测试 —
 *
 * 修复1：纯输入时首帧光标回抽
 * - 旧 bug：`publishLocalHandoffScene()` 只在删除时才把旧 caret 放进 scene.cursorRect，
 *   普通插入时 scene.cursorRect == null，draw 层直接算出新光标位置，
 *   下一帧 timeline 又用旧位置做起点，导致光标回抽。
 * - 修复后：只要 `cursorEnabled && cursorMotionPath != null`，首帧 scene 就用
 *   `originCursorRect` 作为 cursorRect。
 *
 * Issue #725 评论 5750735497：停止自绘屏幕 caret 后，scene.cursorRect / cursorMotionPath
 * 已删除，fix1 的两个测试（断言首帧 scene.cursorRect == originCursorRect）不再适用，已移除。
 *
 * #711 评论 5738906634：删除 ReflowMove 路线后，原修复2（Reflow 部分重叠差集）已不再适用 —
 * ComposeOverlayOwnership 已删除，幸存正文不再由 overlay 接管。
 * 本文件保留 `backspace_across_visual_lines_only_deleted_glyph_in_overlay` 断言：
 * 快速连续 Backspace 跨视觉行时，只有本次 deleted glyph / 已有活动动画 glyph 属于 overlay，
 * 幸存正文始终由 BasicTextField 绘制。
 */
@Suppress("LongMethod", "MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue708Comment5725146968ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== #711：ReflowMove 路线删除后的新语义断言 ====================

    /**
     * #711 评论 5738906634：删除 ReflowMove 路线后 —
     * 快速连续 Backspace 跨视觉行时，只有本次 deleted glyph / 已有活动动画 glyph 属于 overlay，
     * 幸存正文始终由 BasicTextField 绘制。
     *
     * 场景："abc" -> "ac"（删除 'b'）-> "a"（删除 'c'）。
     * - 第一笔删除 'b'：'b' [1,2) 是被删除的旧字，可以作为 ghost 由 overlay 吞掉。
     * - 第二笔删除 'c'：'c' [1,2)（在 "ac" 中）是被删除的旧字，可以作为 ghost 由 overlay 吞掉。
     * - 'a' [0,1) 是幸存正文，不应进 hiddenRanges，直接由 BasicTextField 画。
     *
     * 暴露断言：连续 Backspace 后，'a' 的 range 不应在 hiddenRanges 中。
     */
    @Test
    fun backspace_across_visual_lines_only_deleted_glyph_in_overlay() {
        val layouts = captureLayoutsWithWidth(arrayOf("abc", "ac", "a"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-711-5725146968-backspace",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout："abc"，caret 在 offset 2
        state.onAuthoritativeLayout(layouts[0], TextRange(2, 2), 0)

        // 第一笔 Backspace: "abc" -> "ac"（删除 offset 1 的 'b'）
        state.recordLocalInput(
            oldText = "abc",
            newText = "ac",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 1), oldRange = TextRange(1, 2))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        val sceneAfterFirstDelete = state.drawSnapshot().scene
        // 'a' [0,1) 是幸存正文，不应进 hiddenRanges
        val aRange = TextRange(0, 1)
        val aInHiddenAfterFirst =
            sceneAfterFirstDelete.hiddenRanges.any { it.start == aRange.start && it.end == aRange.end }
        assertTrue(
            "backspace: 第一笔删除 'b' 后，幸存正文 'a' [0,1) 不应进 hiddenRanges，" +
                "实际 hiddenRanges=${sceneAfterFirstDelete.hiddenRanges}" +
                "（只有被删除的 'b' ghost 可以由 overlay 接管，幸存正文由 BasicTextField 画）",
            !aInHiddenAfterFirst,
        )

        // 第二笔 Backspace: "ac" -> "a"（删除 offset 1 的 'c'）
        state.recordLocalInput(
            oldText = "ac",
            newText = "a",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 1), oldRange = TextRange(1, 2))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0)

        val sceneAfterSecondDelete = state.drawSnapshot().scene
        // 'a' [0,1) 仍是幸存正文，不应进 hiddenRanges
        val aInHiddenAfterSecond =
            sceneAfterSecondDelete.hiddenRanges.any { it.start == aRange.start && it.end == aRange.end }
        assertTrue(
            "backspace: 第二笔删除 'c' 后，幸存正文 'a' [0,1) 仍不应进 hiddenRanges，" +
                "实际 hiddenRanges=${sceneAfterSecondDelete.hiddenRanges}" +
                "（一路删除到第一视觉行时，前面的幸存正文始终由 BasicTextField 绘制）",
            !aInHiddenAfterSecond,
        )
    }

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L
    }

    @Suppress("LongParameterList")
    private fun makePatch(
        id: Long,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        offsetMap: List<VisualOffsetMapEntry>? = null,
        insertedUnits: List<TextRange> = emptyList(),
        deletedUnits: List<TextRange> = emptyList(),
        retainedMoves: List<RetainedMove> = emptyList(),
        durationMs: Long = 100L,
        motionPolicy: EditorMotionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
    ): ComposeVisualPatch =
        ComposeVisualPatch(
            id = id,
            coreTransactionIds = listOf(id),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = offsetMap,
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = retainedMoves,
            durationMs = durationMs,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            motionPolicy = motionPolicy,
        )

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> = captureLayoutsWithWidth(texts, 1000)

    private fun captureLayoutsWithWidth(
        texts: Array<out String>,
        maxWidth: Int,
        fontSizeSp: Float = 14f,
    ): List<TextLayoutResult> =
        captureLayoutsWithMultipleWidths(
            *texts.map { it to maxWidth }.toTypedArray(),
            fontSizeSp = fontSizeSp,
        )

    /**
     * #708 评论 5725146968：一次 setContent 内捕获多种宽度的 layout —
     * composeRule.setContent 每个测试只能调用一次，
     * 需要不同宽度 layout 的测试用本方法一次取齐。
     */
    private fun captureLayoutsWithMultipleWidths(
        vararg textWidthPairs: Pair<String, Int>,
        fontSizeSp: Float = 14f,
    ): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            for ((text, width) in textWidthPairs) {
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = fontSizeSp.sp),
                        constraints = Constraints(maxWidth = width),
                    ),
                )
            }
        }
        return results
    }
}
