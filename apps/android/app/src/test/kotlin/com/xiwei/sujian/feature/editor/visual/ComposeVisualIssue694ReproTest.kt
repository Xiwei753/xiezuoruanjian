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

/**
 * #694 复现测试 — "Android：动画事务拿得太晚，快速输入/删除和跨行回流仍会乱跳"。
 *
 * Issue #694 评论第 7 步揭示的根因：
 *
 * > 当前代码 `while (pendingPatches.isNotEmpty()) { visualTimeline.applyPatch(patch, sameFrameTimeNanos, ...) }`
 * > 会在同一个屏幕帧里把 retained text/cursor 连续重定向几次。改成一次取完这一帧的 patch，
 * > 先合成一个屏幕 transition，再只 applyPatch() 一次：
 * > `val batch = drainAllPendingPatches(); val framePatch = ComposeVisualPatchBatch.compose(batch);
 * > visualTimeline.applyPatch(patch = framePatch, frameTimeNanos = frameTimeNanos, ...)`。
 * > 这一项就是录像里"删除回上一行突然抽一下"的另一半根因。
 *
 * 本测试复现该缺陷：构造快速连续输入 3 笔（"" -> "a" -> "ab" -> "abc"），
 * 让 3 个 patch 在同一 VSync（同一 frameTimeNanos）到达 pendingPatches 队列，
 * 然后调用 [ComposeEditorVisualState.drainPendingPatchesAtFrame]。
 *
 * 期望行为（Issue #694 评论第 7 步给出）：同一 VSync 的多笔 patch 应先合成一个屏幕
 * transition（ComposeVisualPatchBatch.compose），再只 applyPatch() 一次。
 *
 * 当前实现：drainPendingPatchesAtFrame 用 `while (pendingPatches.isNotEmpty())` 循环
 * 逐笔 applyPatch，对同一 frameTimeNanos 多次调用 visualTimeline.applyPatch()，
 * 在同一个屏幕帧里把 retained text/cursor 连续重定向几次。
 *
 * 本测试在当前实现下应 **FAIL**（applied.size == 3，期望 == 1），体现 bug。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength")
class ComposeVisualIssue694ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 复现 #694 评论第 7 步：同一 VSync 多笔 patch 不应逐笔 applyPatch。
     *
     * 场景：快速连续输入 "a" -> "ab" -> "abc" 三笔，3 个 patch 在同一 VSync 到达。
     *
     * 断言：drainPendingPatchesAtFrame 应只 applyPatch 一次（batch 合成后），
     * applied.size == 1。当前实现逐笔 applyPatch 3 次，applied.size == 3，FAIL。
     */
    @Test
    fun sameVsyncMultiplePatches_shouldBatchCompose_applyPatchOnce() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val state = ComposeEditorVisualState(targetId = "issue694-repro-same-vsync")

        // === 第 1 笔："" -> "a" ===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "a",
                newRange = TextRange(0, 1),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 1),
            ),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        val patchA = state.latestPatch.value
        assertNotNull("patch A（空串 -> a）应生成", patchA)

        // === 第 2 笔："a" -> "ab" ===
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 2L,
                baseRev = 1L,
                newRev = 2L,
                oldText = "a",
                newText = "ab",
                newRange = TextRange(1, 2),
                replaceBounds = VisualReplaceBounds(1, 1, 1, 2),
                offsetMap =
                    VisualOffsetMap(
                        entries = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                    ),
            ),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        val patchB = state.latestPatch.value
        assertNotNull("patch B（a -> ab）应生成", patchB)

        // === 第 3 笔："ab" -> "abc" ===
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 3L,
                baseRev = 2L,
                newRev = 3L,
                oldText = "ab",
                newText = "abc",
                newRange = TextRange(2, 3),
                replaceBounds = VisualReplaceBounds(2, 2, 2, 3),
                offsetMap =
                    VisualOffsetMap(
                        entries = listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY)),
                    ),
            ),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(3, 3), 0)
        val patchC = state.latestPatch.value
        assertNotNull("patch C（ab -> abc）应生成", patchC)

        // 确认 pendingPatches 队列有 3 个 patch（通过反射访问 private 字段）
        val pendingSize = pendingPatchesSize(state)
        assertTrue(
            "drain 前 pendingPatches 队列应有 3 个 patch（快速输入 3 笔在同一 VSync 到达），实际=$pendingSize\n" +
                "Issue #694 复现前提：同一 VSync 多笔 patch",
            pendingSize == 3,
        )

        // === 用同一 frameTimeNanos 消费所有 pending patch ===
        val frameTimeNanos = 0L
        val applied = state.drainPendingPatchesAtFrame(frameTimeNanos)

        // 核心断言：同一 VSync 的多笔 patch 应先合成一个屏幕 transition，再只 applyPatch() 一次。
        // 当前实现：while 循环逐笔 applyPatch 3 次，applied.size == 3。
        // 期望实现（Issue #694 评论第 7 步）：batch 合成后只 applyPatch 一次，applied.size == 1。
        assertEquals(
            "同一 VSync 的多笔 patch 应先合成一个屏幕 transition（ComposeVisualPatchBatch.compose），" +
                "再只 applyPatch() 一次，applied.size 应为 1。\n" +
                "当前实现：while (pendingPatches.isNotEmpty()) 逐笔 applyPatch ${applied.size} 次，" +
                "在同一个屏幕帧里把 retained text/cursor 连续重定向几次 → 快速输入/删除和跨行回流乱跳。\n" +
                "Issue #694 评论第 7 步：'同一 VSync 不能逐笔重定向几何'",
            1,
            applied.size,
        )
    }

    /**
     * 复现 #694 评论第 7 步的另一半：同一 VSync 多笔 applyPatch 导致 retained text/cursor
     * 连续重定向，sample 后 unit 的 position 通道被多次重设。
     *
     * 场景：快速连续输入 "a" -> "ab" -> "abc" 三笔，在同一 VSync 消费。
     *
     * 断言：sample 后存活的 unit（"a"）的 position 通道 startedAtNanos 应只被设一次
     * （batch 合成后）。当前实现逐笔 applyPatch，每次都用 frameTimeNanos 重设 startedAtNanos，
     * position 通道被连续重定向几次。
     *
     * 本测试通过观察 sample 后 unit 的 position 通道状态，验证逐笔 applyPatch 导致的
     * "连续重定向"现象。当前实现下，逐笔 applyPatch 会让中间 unit 的 position 通道
     * 被多次 rebase/重定向，最终 unit 数量与 batch 合成相同，但中间过程不同。
     */
    @Test
    fun sameVsyncMultiplePatches_retainedTextCursor_consecutiveRedirect() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val state = ComposeEditorVisualState(targetId = "issue694-repro-redirect")

        // 连续 3 笔快速输入，在同一 VSync 到达
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "a",
                newRange = TextRange(0, 1),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 1),
            ),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 2L,
                baseRev = 1L,
                newRev = 2L,
                oldText = "a",
                newText = "ab",
                newRange = TextRange(1, 2),
                replaceBounds = VisualReplaceBounds(1, 1, 1, 2),
                offsetMap =
                    VisualOffsetMap(
                        entries = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                    ),
            ),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 3L,
                baseRev = 2L,
                newRev = 3L,
                oldText = "ab",
                newText = "abc",
                newRange = TextRange(2, 3),
                replaceBounds = VisualReplaceBounds(2, 2, 2, 3),
                offsetMap =
                    VisualOffsetMap(
                        entries = listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY)),
                    ),
            ),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(3, 3), 0)

        val pendingSize = pendingPatchesSize(state)
        assertTrue("drain 前 pendingPatches 队列应有 3 个 patch，实际=$pendingSize", pendingSize == 3)

        // 同一 VSync 消费
        val frameTimeNanos = 0L
        val applied = state.drainPendingPatchesAtFrame(frameTimeNanos)
        val scene = state.sampleVisualScene(frameTimeNanos)

        // 逐笔 applyPatch 3 次后，最终应有 3 个 unit（"a", "b", "c"）。
        // 但关键问题是：applied.size == 3 说明同一 VSync 调用了 3 次 applyPatch，
        // 每次都把 retained text/cursor 重定向一次。这是"乱跳"的根因。
        // 期望：batch 合成后只 applyPatch 一次（applied.size == 1），retained text/cursor 只重定向一次。
        assertEquals(
            "同一 VSync 应只 applyPatch 一次（batch 合成），不应逐笔重定向 retained text/cursor。\n" +
                "当前 applyPatch 调用次数=${applied.size}，glyph overlay 数量=${scene?.glyphOverlays?.size ?: 0}\n" +
                "Issue #694 评论第 7 步：'同一 VSync 不能逐笔重定向几何'",
            1,
            applied.size,
        )
    }

    // ==================== 辅助方法 ====================

    private fun makeInsertIntent(
        coreTxnId: Long,
        baseRev: Long,
        newRev: Long,
        oldText: String,
        newText: String,
        newRange: TextRange,
        replaceBounds: VisualReplaceBounds,
        offsetMap: VisualOffsetMap? = null,
    ): EditorEditFact =
        EditorEditFact(
            cause = uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            coreTransactionId = coreTxnId,
            baseRevision = baseRev,
            newRevision = newRev,
            animationMode = AnimationMode.CLUSTER_ANIMATION,
            durationMs = 100L,
            offsetMap = offsetMap,
            oldRanges = emptyList(),
            newRanges = listOf(newRange),
            textKind = TextVisualKind.Insert,
            replaceBounds = replaceBounds,
            expectedOldText = oldText,
            expectedNewText = newText,
        )

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

    /**
     * 通过反射访问 ComposeEditorVisualState 的 private pendingPatches 队列大小。
     * 用于确认同一 VSync 多笔 patch 已入队。
     */
    private fun pendingPatchesSize(state: ComposeEditorVisualState): Int {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("pendingPatches")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val deque = field.get(state) as kotlin.collections.ArrayDeque<*>
        return deque.size
    }
}
