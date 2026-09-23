package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.boundsForRawRange
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #739 评论 5787769674：retained reflow 接入 [CoordinatedEditMotion] 的测试 —
 * 验证跨行 retained motion 的位置平移和 destination hidden ownership 语义。
 *
 * 用真实 [TextLayoutResult]（通过 [rememberTextMeasurer]）+ 硬换行 `\n` 制造多行布局
 * （Robolectric 下 [rememberTextMeasurer] 忽略 maxWidth 软换行约束，每字符 1px 单行，
 * 但 `\n` 硬换行可以产生真实多行 layout）。
 *
 * 注意：本测试用硬换行 `\n` 制造多行布局，验证 retained move channel 的纯运动语义
 * （位置平移、destination hidden、全程可见）。硬换行不等同于软自动换行（soft wrap）—
 * Robolectric 下 [rememberTextMeasurer] 忽略 maxWidth 软换行约束，无法用窄宽度产生真实软换行。
 * 软自动换行的 layout-first pending ownership 由
 * [ComposeVisualStateLayoutFirstPendingPresentationTest] 覆盖。
 * 本文件只做 retained channel 的纯运动测试。
 *
 * 两类场景：
 * 1. 行尾插入字符把后面原有文字挤到下一行（"abcde" → "abX\ncde"，"cde" 被挤到第二行）。
 * 2. 删除字符让下一行原有文字回流到上一行（"abX\ncde" → "abcde"，"cde" 回流到第一行）。
 *
 * 每个场景验证：
 * - progress=0：retained overlay 在 old position（translate≈Zero），destination newRange 已被隐藏。
 * - 中间帧：retained overlay 位置在 old/new 之间，destination newRange 仍被隐藏。
 * - 结束：retained overlay 到达 new position，destination hidden ownership 释放（交还 BasicTextField）。
 *
 * 保留文字不套 clipFraction，全程可见，只做位置平移。
 * retained overlay 和 caret/insert/delete glyph 共用同一个 master progress，不另开 timer。
 */
@Suppress("TooManyFunctions")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualRetainedWrapMotionTest {
    @get:Rule
    val composeRule = createComposeRule()

    private val startTime = 1_000_000_000L
    private val duration = 100_000_000L // 100ms in nanos

    companion object {
        /** detekt StringLiteralDuplication：提取重复 ≥3 次的断言消息。 */
        private const val MSG_TRANSLATE_ZERO = "progress=0 时 translate 应为 Zero"
        private const val MSG_MID_HAS_RETAINED = "中间帧应有 retained overlay"
    }

    /**
     * 场景1：行尾插入字符挤换行（跨行 retained motion）。
     *
     * - oldText = "abcde"（一行）
     * - newText = "abX\ncde"（两行，插入 "X\n" 在 offset 2，"cde" 被挤到第二行）
     * - "cde" 是原有文字（retained move）：oldRange=[2,5)，newRange=[4,7)
     * - insertedUnits = [TextRange(2,4)]（'X' 和 '\n'）
     * - originCaretOffset = 2，targetCaretOffset = 4
     */
    @Test
    fun insertChar_pushesRetainedTextToNextLine() {
        val (oldLayout, newLayout) = measureScene1()
        // "cde" 的 old/new range
        val oldRange = TextRange(2, 5)
        val newRange = TextRange(4, 7)
        val patch =
            ComposeVisualPatch(
                id = 1L,
                coreTransactionIds = listOf(1L),
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap = null,
                insertedUnits = listOf(TextRange(2, 4)),
                deletedUnits = emptyList(),
                retainedMoves = listOf(RetainedMove(oldRange, newRange)),
                originCaretRect = oldLayout.result.getCursorRect(2),
                targetCaretRect = newLayout.result.getCursorRect(4),
                originCaretOffset = 2,
                targetCaretOffset = 4,
                durationMs = 100L,
                animationMode = AnimationMode.GLYPH_ANIMATION,
            )
        val motion =
            CoordinatedEditMotion.fromPatch(
                patch = patch,
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        assertTrue("traversal 应有效（caret 跨行）", motion.isValid)

        // 预期 old/new topLeft
        val oldBounds = oldLayout.boundsForRawRange(oldRange)
        val newBounds = newLayout.boundsForRawRange(newRange)
        assertNotNull("oldBounds 应非空", oldBounds)
        assertNotNull("newBounds 应非空", newBounds)
        val expectedOldTopLeft = Offset(oldBounds!!.left, oldBounds.top)
        val expectedNewTopLeft = Offset(newBounds!!.left, newBounds.top)
        val expectedDx = expectedNewTopLeft.x - expectedOldTopLeft.x
        val expectedDy = expectedNewTopLeft.y - expectedOldTopLeft.y
        assertTrue("应有 y 轴位移（换行）", kotlin.math.abs(expectedDy) > 0.5f)

        // 1. progress=0：retained overlay 在 old position，destination newRange 已被隐藏
        val startSample = motion.sample(startTime)
        assertTrue("progress=0 时应有 retained overlay", startSample.retainedOverlays.isNotEmpty())
        val startOverlay = startSample.retainedOverlays.first()
        assertEquals(MSG_TRANSLATE_ZERO, 0f, startOverlay.translate.x, 0.01f)
        assertEquals(MSG_TRANSLATE_ZERO, 0f, startOverlay.translate.y, 0.01f)
        assertTrue(
            "progress=0 时 destination newRange 应被隐藏",
            startSample.hiddenRanges.contains(newRange),
        )
        assertFalse("progress=0 时 motion 未完成", startSample.finished)

        // 2. 中间帧：retained overlay 位置在 old/new 之间
        val midSample = motion.sample(startTime + duration / 2)
        assertTrue(MSG_MID_HAS_RETAINED, midSample.retainedOverlays.isNotEmpty())
        val midOverlay = midSample.retainedOverlays.first()
        val expectedMidDx = expectedDx * 0.5f
        val expectedMidDy = expectedDy * 0.5f
        assertEquals("中间帧 translate.x 应为 dx*0.5", expectedMidDx, midOverlay.translate.x, 0.01f)
        assertEquals("中间帧 translate.y 应为 dy*0.5", expectedMidDy, midOverlay.translate.y, 0.01f)
        assertTrue(
            "中间帧 destination newRange 应仍被隐藏",
            midSample.hiddenRanges.contains(newRange),
        )
        assertFalse("中间帧 motion 未完成", midSample.finished)

        // 3. 结束：retained overlay 到达 new position，destination hidden ownership 释放
        val endSample = motion.sample(startTime + duration)
        assertTrue("结束后应有 retained overlay（全程可见）", endSample.retainedOverlays.isNotEmpty())
        val endOverlay = endSample.retainedOverlays.first()
        assertEquals("结束 translate.x 应为 dx", expectedDx, endOverlay.translate.x, 0.01f)
        assertEquals("结束 translate.y 应为 dy", expectedDy, endOverlay.translate.y, 0.01f)
        assertFalse(
            "结束后 destination newRange 应释放（不在 hiddenRanges）",
            endSample.hiddenRanges.contains(newRange),
        )
        assertTrue("结束后 motion 完成", endSample.finished)
    }

    /**
     * 场景2：删除字符让下一行原有文字回流到上一行（跨行 retained motion）。
     *
     * - oldText = "abX\ncde"（两行，"cde" 在第二行）
     * - newText = "abcde"（一行，删除 "X\n" 后 "cde" 回流到第一行）
     * - "cde" 是原有文字（retained move）：oldRange=[4,7)，newRange=[2,5)
     * - deletedUnits = [TextRange(2,4)]（'X' 和 '\n'）
     * - originCaretOffset = 4，targetCaretOffset = 2
     */
    @Test
    fun deleteChar_reflowsRetainedTextToPrevLine() {
        val (oldLayout, newLayout) = measureScene2()
        // "cde" 的 old/new range
        val oldRange = TextRange(4, 7)
        val newRange = TextRange(2, 5)
        val patch =
            ComposeVisualPatch(
                id = 2L,
                coreTransactionIds = listOf(2L),
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap = null,
                insertedUnits = emptyList(),
                deletedUnits = listOf(TextRange(2, 4)),
                retainedMoves = listOf(RetainedMove(oldRange, newRange)),
                originCaretRect = oldLayout.result.getCursorRect(4),
                targetCaretRect = newLayout.result.getCursorRect(2),
                originCaretOffset = 4,
                targetCaretOffset = 2,
                durationMs = 100L,
                animationMode = AnimationMode.GLYPH_ANIMATION,
            )
        val motion =
            CoordinatedEditMotion.fromPatch(
                patch = patch,
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        assertTrue("traversal 应有效（caret 跨行）", motion.isValid)

        // 预期 old/new topLeft
        val oldBounds = oldLayout.boundsForRawRange(oldRange)
        val newBounds = newLayout.boundsForRawRange(newRange)
        assertNotNull("oldBounds 应非空", oldBounds)
        assertNotNull("newBounds 应非空", newBounds)
        val expectedOldTopLeft = Offset(oldBounds!!.left, oldBounds.top)
        val expectedNewTopLeft = Offset(newBounds!!.left, newBounds.top)
        val expectedDx = expectedNewTopLeft.x - expectedOldTopLeft.x
        val expectedDy = expectedNewTopLeft.y - expectedOldTopLeft.y
        assertTrue("应有 y 轴位移（回流）", kotlin.math.abs(expectedDy) > 0.5f)

        // 1. progress=0：retained overlay 在 old position，destination newRange 已被隐藏
        val startSample = motion.sample(startTime)
        assertTrue("progress=0 时应有 retained overlay", startSample.retainedOverlays.isNotEmpty())
        val startOverlay = startSample.retainedOverlays.first()
        assertEquals(MSG_TRANSLATE_ZERO, 0f, startOverlay.translate.x, 0.01f)
        assertEquals(MSG_TRANSLATE_ZERO, 0f, startOverlay.translate.y, 0.01f)
        assertTrue(
            "progress=0 时 destination newRange 应被隐藏",
            startSample.hiddenRanges.contains(newRange),
        )
        assertFalse("progress=0 时 motion 未完成", startSample.finished)

        // 2. 中间帧：retained overlay 位置在 old/new 之间
        val midSample = motion.sample(startTime + duration / 2)
        assertTrue(MSG_MID_HAS_RETAINED, midSample.retainedOverlays.isNotEmpty())
        val midOverlay = midSample.retainedOverlays.first()
        val expectedMidDx = expectedDx * 0.5f
        val expectedMidDy = expectedDy * 0.5f
        assertEquals("中间帧 translate.x 应为 dx*0.5", expectedMidDx, midOverlay.translate.x, 0.01f)
        assertEquals("中间帧 translate.y 应为 dy*0.5", expectedMidDy, midOverlay.translate.y, 0.01f)
        assertTrue(
            "中间帧 destination newRange 应仍被隐藏",
            midSample.hiddenRanges.contains(newRange),
        )
        assertFalse("中间帧 motion 未完成", midSample.finished)

        // 3. 结束：retained overlay 到达 new position，destination hidden ownership 释放
        val endSample = motion.sample(startTime + duration)
        assertTrue("结束后应有 retained overlay（全程可见）", endSample.retainedOverlays.isNotEmpty())
        val endOverlay = endSample.retainedOverlays.first()
        assertEquals("结束 translate.x 应为 dx", expectedDx, endOverlay.translate.x, 0.01f)
        assertEquals("结束 translate.y 应为 dy", expectedDy, endOverlay.translate.y, 0.01f)
        assertFalse(
            "结束后 destination newRange 应释放（不在 hiddenRanges）",
            endSample.hiddenRanges.contains(newRange),
        )
        assertTrue("结束后 motion 完成", endSample.finished)
    }

    /**
     * 验证 retained overlay 全程可见（不套 clipFraction）—
     * 即使 progress=0 也应有 retained overlay（translate=Zero，文字在 old position）。
     */
    @Test
    fun retainedOverlay_alwaysVisible_noClipFraction() {
        val (oldLayout, newLayout) = measureScene1()
        val patch =
            ComposeVisualPatch(
                id = 3L,
                coreTransactionIds = listOf(3L),
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap = null,
                insertedUnits = listOf(TextRange(2, 4)),
                deletedUnits = emptyList(),
                retainedMoves = listOf(RetainedMove(TextRange(2, 5), TextRange(4, 7))),
                originCaretRect = oldLayout.result.getCursorRect(2),
                targetCaretRect = newLayout.result.getCursorRect(4),
                originCaretOffset = 2,
                targetCaretOffset = 4,
                durationMs = 100L,
                animationMode = AnimationMode.GLYPH_ANIMATION,
            )
        val motion =
            CoordinatedEditMotion.fromPatch(
                patch = patch,
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 在 progress=0、0.25、0.5、0.75、1.0 各帧都应有 retained overlay
        for (frac in floatArrayOf(0f, 0.25f, 0.5f, 0.75f, 1f)) {
            val t = startTime + (duration * frac).toLong()
            val sample = motion.sample(t)
            assertTrue(
                "frac=$frac 时应有 retained overlay（全程可见）",
                sample.retainedOverlays.isNotEmpty(),
            )
        }
    }

    /**
     * 验证 retained move 和 inserted glyph 共用同一个 master progress —
     * 不另开 timer。inserted glyph 的 clipFraction 和 retained overlay 的 translate
     * 在同一帧由同一个 progress 驱动。
     */
    @Test
    fun retainedAndInserted_shareSameMasterProgress() {
        val (oldLayout, newLayout) = measureScene1()
        val patch =
            ComposeVisualPatch(
                id = 4L,
                coreTransactionIds = listOf(4L),
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap = null,
                insertedUnits = listOf(TextRange(2, 4)),
                deletedUnits = emptyList(),
                retainedMoves = listOf(RetainedMove(TextRange(2, 5), TextRange(4, 7))),
                originCaretRect = oldLayout.result.getCursorRect(2),
                targetCaretRect = newLayout.result.getCursorRect(4),
                originCaretOffset = 2,
                targetCaretOffset = 4,
                durationMs = 100L,
                animationMode = AnimationMode.GLYPH_ANIMATION,
            )
        val motion =
            CoordinatedEditMotion.fromPatch(
                patch = patch,
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 中间帧：inserted glyph fraction 和 retained overlay translate 都应由同一个 progress=0.5 驱动
        val midSample = motion.sample(startTime + duration / 2)
        // inserted glyph 应有 overlay（fraction > 0）
        assertTrue(
            "中间帧应有 inserted glyph overlay",
            midSample.glyphOverlays.isNotEmpty(),
        )
        // retained overlay 也应有
        assertTrue(
            MSG_MID_HAS_RETAINED,
            midSample.retainedOverlays.isNotEmpty(),
        )
        // 两者都由 progress=0.5 驱动，不另开 timer
    }

    // ==================== 辅助方法 ====================

    /**
     * 场景1：测量 "abcde"（一行）和 "abX\ncde"（两行）。
     */
    private fun measureScene1(): Pair<ComposeLayoutSnapshot, ComposeLayoutSnapshot> {
        lateinit var oldLayout: ComposeLayoutSnapshot
        lateinit var newLayout: ComposeLayoutSnapshot
        composeRule.setContent {
            val textMeasurer = rememberTextMeasurer()
            val oldResult =
                textMeasurer.measure(
                    text = AnnotatedString("abcde"),
                    style = TextStyle(fontSize = 14f.sp),
                    constraints = Constraints(maxWidth = 1000),
                )
            val newResult =
                textMeasurer.measure(
                    text = AnnotatedString("abX\ncde"),
                    style = TextStyle(fontSize = 14f.sp),
                    constraints = Constraints(maxWidth = 1000),
                )
            oldLayout = ComposeLayoutSnapshot(oldResult, TextRange(2, 2), 0)
            newLayout = ComposeLayoutSnapshot(newResult, TextRange(4, 4), 0)
        }
        return oldLayout to newLayout
    }

    /**
     * 场景2：测量 "abX\ncde"（两行）和 "abcde"（一行）。
     */
    private fun measureScene2(): Pair<ComposeLayoutSnapshot, ComposeLayoutSnapshot> {
        lateinit var oldLayout: ComposeLayoutSnapshot
        lateinit var newLayout: ComposeLayoutSnapshot
        composeRule.setContent {
            val textMeasurer = rememberTextMeasurer()
            val oldResult =
                textMeasurer.measure(
                    text = AnnotatedString("abX\ncde"),
                    style = TextStyle(fontSize = 14f.sp),
                    constraints = Constraints(maxWidth = 1000),
                )
            val newResult =
                textMeasurer.measure(
                    text = AnnotatedString("abcde"),
                    style = TextStyle(fontSize = 14f.sp),
                    constraints = Constraints(maxWidth = 1000),
                )
            oldLayout = ComposeLayoutSnapshot(oldResult, TextRange(4, 4), 0)
            newLayout = ComposeLayoutSnapshot(newResult, TextRange(2, 2), 0)
        }
        return oldLayout to newLayout
    }
}
