package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #684 评论 5666730754 复现测试 — 两个 active ownership bug：
 *
 * 问题1：suppressed/未画文字的事务仍会在下一笔被 materializeStartFrame() 重新画出来。
 *   B 是 SYSTEM_SUPPRESSED 时，B 自己 startFrame=null、hiddenRanges=[]，系统直接显示 B 的最终正文；
 *   但 coordinator 仍然 active = transaction，overlay 仍按 durationMs 让这笔事务存活。
 *   若 B 还没被 completeTransaction() 清掉时马上来了下一笔可动画事务 C，
 *   C 会调用 materializeStartFrame() 物化 B 的文字 slice（因为 materializeStartFrame 只看
 *   prev.textKind/oldRanges/newRanges/retainedMoves，不知道 B 的正文从未被 overlay 动画过）。
 *   结果：B 已经由系统直接落到终点，C 开始时又把 B 当"半途动画"物化出来，闪回/重影一帧。
 *
 * 问题2：光标也有同一个问题——新事务会从"上一笔虚构的中间位置"起跑。
 *   当前 interruptedCursorRect 只检查新事务 firstCursor.animate，没检查上一笔 activeTx 的光标
 *   到底有没有被 overlay 动画。当 B 是 SYSTEM_SUPPRESSED（cursor should_animate=false），
 *   屏幕光标已直接在 B.end，但 B 因 durationMs 仍挂在 active。马上输入 C（cursor animate=true），
 *   代码会把 B.start→B.end 按 masterProgress 插值，拿一个屏幕上从未出现过的中间 rect 当 C 的起点，
 *   C 一开始光标先向后跳再向前动画。
 *
 * 两个测试在当前代码下都应失败（证明 bug 存在）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualActiveOwnershipReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 问题1 测试 ====================

    /**
     * 问题1 复现：SYSTEM_SUPPRESSED B 的正文不应被下一笔 C 的 materializeStartFrame 重新画出。
     *
     * 场景：
     * - 事务 A：CLUSTER_ANIMATION Insert "abc"（active=A，A 在 progress=0.5）。
     * - 事务 B：SYSTEM_SUPPRESSED "abc" → "abcd"。
     *   B.startFrame=null、B.suppressedCurrentRanges=[]、B.textKind=Insert、B.newRanges=[3,4)。
     *   active=B，overlay 不画 B 的正文（textEnabled=false），系统直接显示 "abcd"。
     * - reportProgress(0.5) — B 的 master progress 到一半（但 overlay 没动画过 B 的正文）。
     * - 事务 C：CLUSTER_ANIMATION "abcd" → "abcde"。
     *   C.startFrame = materializeStartFrame(B, 0.5, ...)。
     *
     * 正确行为：C.startFrame 应为 null（或 slices 为空）— B 的正文从未被 overlay 动画过，
     *   屏幕已经在 B 的最终正文，C 不应把 B 当"半途动画"物化出来。
     *
     * 当前代码：materializeStartFrame 只看 prev.textKind/oldRanges/newRanges/retainedMoves，
     *   不知道 B 的正文从未被 overlay 动画过。collectCurrentSlicesAsRebased(B, 0.5) 会返回
     *   B.newRanges=[3,4) 的 surviving slice，C.startFrame.slices 非空，闪回/重影一帧。
     */
    @Test
    fun systemSuppressed_b_text_not_rematerialized_by_next_c_bug1() {
        val layouts = captureLayouts("", "abc", "abcd", "abcde")
        val state = ComposeEditorVisualState(targetId = "test-target-active-ownership-1")

        // === 生成事务 A（CLUSTER_ANIMATION Insert "abc"）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        val intentA =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 3),
                expectedOldText = "",
                expectedNewText = "abc",
            )
        state.onVisualIntent(
            intentA,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val txA = state.activeTransaction.value
        assertNotNull("事务 A 应生成", txA)

        // A 跑到 progress=0.5。
        state.reportProgress(0.5f)

        // === 生成事务 B（SYSTEM_SUPPRESSED "abc" → "abcd"）===
        val intentB =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(3, 4)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 3, oldEnd = 3, newStart = 3, newEnd = 4),
                expectedOldText = "abc",
                expectedNewText = "abcd",
            )
        state.onVisualIntent(
            intentB,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(4, 4), 0)

        val txB = state.activeTransaction.value
        assertNotNull("事务 B 应生成", txB)
        // 确认 B 是 SYSTEM_SUPPRESSED：startFrame=null、suppressedCurrentRanges=空。
        assertTrue(
            "B 应是 SYSTEM_SUPPRESSED：startFrame 应为 null，实际=${txB?.startFrame}",
            txB?.startFrame == null,
        )
        assertTrue(
            "B 应是 SYSTEM_SUPPRESSED：suppressedCurrentRanges 应为空，实际=${txB?.suppressedCurrentRanges}",
            txB?.suppressedCurrentRanges.isNullOrEmpty(),
        )

        // B 的 master progress 到一半（overlay 不画 B 的正文，但 active 仍指向 B）。
        state.reportProgress(0.5f)

        // === 生成事务 C（CLUSTER_ANIMATION "abcd" → "abcde"）===
        val intentC =
            EditorVisualIntent(
                coreTransactionId = 3L,
                baseRevision = 2L,
                newRevision = 3L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                VisualOffsetMapEntry(0, 0, 4, VisualOffsetMapKind.IDENTITY), // "abcd"
                            ),
                    ),
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(4, 5)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 4, oldEnd = 4, newStart = 4, newEnd = 5),
                expectedOldText = "abcd",
                expectedNewText = "abcde",
            )
        state.onVisualIntent(
            intentC,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(5, 5), 0)

        val txC = state.activeTransaction.value
        assertNotNull("事务 C 应生成", txC)

        val startFrameC = txC?.startFrame
        val slicesC = startFrameC?.slices ?: emptyList()

        // 核心断言：C.startFrame 不应包含从 B 物化出的正文 slice。
        // B 是 SYSTEM_SUPPRESSED，overlay 从未动画过 B 的正文，屏幕已经在 B 的最终正文 "abcd"。
        // C 开始时不应把 B 当"半途动画"物化出来。
        // 当前代码：materializeStartFrame 只看 prev.textKind/oldRanges/newRanges/retainedMoves，
        // collectCurrentSlicesAsRebased(B, 0.5) 返回 B.newRanges=[3,4) 的 surviving slice，
        // C.startFrame.slices 非空 → 闪回/重影一帧。
        assertTrue(
            "C.startFrame 不应包含从 SYSTEM_SUPPRESSED B 物化出的正文 slice，" +
                "实际 slices targetRanges=${slicesC.map { it.targetRange }}\n" +
                "#684 评论 5666730754 问题1：B 的正文从未被 overlay 动画过（SYSTEM_SUPPRESSED），" +
                "屏幕已在 B 最终正文，C 不应把 B 当半途动画物化出来",
            slicesC.isEmpty(),
        )
    }

    // ==================== 问题2 测试 ====================

    /**
     * 问题2 复现：光标不应从上一笔 SYSTEM_SUPPRESSED B 的虚构中间位置起跑。
     *
     * 场景：
     * - 事务 B：SYSTEM_SUPPRESSED "abc" → "abc\nd"，cursor oldEndUtf16=3, newEndUtf16=5, animate=false。
     *   B.cursorStartRect = "abc" layout getCursorRect(3)（第一行末尾）。
     *   B.cursorEndRect = "abc\nd" layout getCursorRect(5)（第二行末尾）。
     *   overlay 不画 B 的光标动画（cursor.animate=false），屏幕光标已直接在 B.end（第二行末尾）。
     *   active=B，B 因 durationMs 仍挂在 active。
     * - reportProgress(0.5) — B 的 master progress 到一半。
     * - 事务 C：CLUSTER_ANIMATION "abc\nd" → "abc\nde"，cursor oldEndUtf16=5, newEndUtf16=6, animate=true。
     *
     * 正确行为：C.cursorStartRect 应等于 B.cursorEndRect（屏幕真实光标位置，第二行末尾），
     *   即 C 第一笔 old cursor 在 consumed.layout 的 rect。
     *
     * 当前代码：interruptedCursorRect 只检查 firstCursor.animate==true（C 的 cursor），
     *   没检查上一笔 activeTx 的光标到底有没有被 overlay 动画。
     *   把 B.cursorStartRect→B.cursorEndRect 按 masterProgress=0.5 插值，
     *   拿一个屏幕上从未出现过的中间 rect（top 在第一行和第二行之间）当 C 的起点，
     *   C 一开始光标先向后跳再向前动画。
     */
    @Test
    fun cursor_not_from_suppressed_b_midpoint_bug2() {
        // 用 "\n" 构造多行文本，确保 cursor rect 的 top 不同。
        val layouts = captureLayouts("abc", "abc\nd", "abc\nde")
        val oldLayoutB = layouts[0] // "abc" — 单行
        val newLayoutB = layouts[1] // "abc\nd" — 两行
        val newLayoutC = layouts[2] // "abc\nde" — 两行

        // 确认 "abc\nd" 跨多行（硬换行），cursor rect 的 top 会不同。
        assertTrue(
            "newLayoutB 应跨多行（硬换行），实际 lineCount=${newLayoutB.lineCount}",
            newLayoutB.lineCount >= 2,
        )

        val state = ComposeEditorVisualState(targetId = "test-target-active-ownership-2")

        // === 基线 layout "abc" 到达，cursor 在末尾 (offset=3) ===
        state.onAuthoritativeLayout(oldLayoutB, TextRange(3, 3), 0)

        // === 生成事务 B（SYSTEM_SUPPRESSED "abc" → "abc\nd"，cursor animate=false）===
        val intentB =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY), // "abc"
                            ),
                    ),
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(3, 5)), // 插入的 "\nd"
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 3, newEndUtf16 = 5, animate = false),
                replaceBounds = VisualReplaceBounds(oldStart = 3, oldEnd = 3, newStart = 3, newEnd = 5),
                expectedOldText = "abc",
                expectedNewText = "abc\nd",
            )
        state.onVisualIntent(
            intentB,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(newLayoutB, TextRange(5, 5), 0)

        val txB = state.activeTransaction.value
        assertNotNull("事务 B 应生成", txB)

        val bCursorStartRect = txB?.cursorStartRect
        val bCursorEndRect = txB?.cursorEndRect
        assertNotNull("B.cursorStartRect 应非 null", bCursorStartRect)
        assertNotNull("B.cursorEndRect 应非 null", bCursorEndRect)

        // 确认 B.cursorStartRect 和 B.cursorEndRect 的 top 不同（多行布局）。
        val bStartTop = bCursorStartRect!!.top
        val bEndTop = bCursorEndRect!!.top
        assertTrue(
            "B.cursorStartRect.top 应不同于 B.cursorEndRect.top（多行布局），" +
                "实际 startTop=$bStartTop, endTop=$bEndTop",
            kotlin.math.abs(bStartTop - bEndTop) > 1f,
        )

        // B 的 master progress 到一半（overlay 不画 B 的光标动画，屏幕光标已直接在 B.end）。
        state.reportProgress(0.5f)

        // === 生成事务 C（CLUSTER_ANIMATION "abc\nd" → "abc\nde"，cursor animate=true）===
        val intentC =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                VisualOffsetMapEntry(0, 0, 5, VisualOffsetMapKind.IDENTITY), // "abc\nd"
                            ),
                    ),
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(5, 6)), // 插入的 "e"
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 5, newEndUtf16 = 6, animate = true),
                replaceBounds = VisualReplaceBounds(oldStart = 5, oldEnd = 5, newStart = 5, newEnd = 6),
                expectedOldText = "abc\nd",
                expectedNewText = "abc\nde",
            )
        state.onVisualIntent(
            intentC,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(newLayoutC, TextRange(6, 6), 0)

        val txC = state.activeTransaction.value
        assertNotNull("事务 C 应生成", txC)

        val cCursorStartRect = txC?.cursorStartRect
        assertNotNull("C.cursorStartRect 应非 null", cCursorStartRect)

        // 正确起点：C 第一笔 old cursor 在 consumed.layout 的 rect = "abc\nd" getCursorRect(5) = B.cursorEndRect。
        val correctStartRect = newLayoutB.getCursorRect(5)
        // 错误起点：B.cursorStartRect → B.cursorEndRect 按 masterProgress=0.5 插值（中间 top）。
        val wrongStartRect =
            Rect(
                left = (bCursorStartRect.left + bCursorEndRect.left) / 2f,
                top = (bCursorStartRect.top + bCursorEndRect.top) / 2f,
                right = (bCursorStartRect.right + bCursorEndRect.right) / 2f,
                bottom = (bCursorStartRect.bottom + bCursorEndRect.bottom) / 2f,
            )

        val startRect = cCursorStartRect!!
        val matchesCorrect = kotlin.math.abs(startRect.top - correctStartRect.top) < 1f
        val matchesWrong = kotlin.math.abs(startRect.top - wrongStartRect.top) < 1f

        // 核心断言：C.cursorStartRect 应等于 B.cursorEndRect（屏幕真实光标位置，第二行末尾），
        // 而非 B.cursorStartRect/cursorEndRect 的中间插值。
        // 当前代码：interruptedCursorRect 只检查 firstCursor.animate==true（C 的 cursor），
        // 没检查上一笔 activeTx 的光标到底有没有被 overlay 动画，
        // 把 B.start→B.end 按 masterProgress=0.5 插值拿一个屏幕上从未出现过的中间 rect 当 C 的起点。
        assertTrue(
            "C.cursorStartRect 应等于 B.cursorEndRect（屏幕真实光标位置），而非 B.start/end 的中间插值\n" +
                "实际 C.cursorStartRect.top=${startRect.top}\n" +
                "正确（B.cursorEndRect.top）=${correctStartRect.top}\n" +
                "错误（中间插值 top）=${wrongStartRect.top}\n" +
                "matchesCorrect=$matchesCorrect, matchesWrong=$matchesWrong\n" +
                "#684 评论 5666730754 问题2：B 是 SYSTEM_SUPPRESSED 且 cursor.animate=false，" +
                "屏幕光标已直接在 B.end，但 interruptedCursorRect 只检查新事务 firstCursor.animate，" +
                "没检查上一笔 activeTx 的光标到底有没有被 overlay 动画，" +
                "C 一开始光标先向后跳再向前动画",
            matchesCorrect && !matchesWrong,
        )
    }

    // ==================== 辅助方法 ====================

    /**
     * 用 [rememberTextMeasurer] 在 Compose 测试环境里构造真实 [TextLayoutResult]。
     * 一次 setContent 构造多份 layout，供测试里多代 rebase 使用。
     * maxWidth=1000 避免折行，让 bounds 计算确定；硬换行 `\n` 一定产生多行布局。
     */
    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> =
        captureLayoutsWithWidth(texts, maxWidth = 1000)

    private fun captureLayoutsWithWidth(
        texts: Array<out String>,
        maxWidth: Int,
        fontSizeSp: Float = 14f,
    ): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = fontSizeSp.sp),
                        constraints = Constraints(maxWidth = maxWidth),
                    ),
                )
            }
        }
        return results
    }
}
