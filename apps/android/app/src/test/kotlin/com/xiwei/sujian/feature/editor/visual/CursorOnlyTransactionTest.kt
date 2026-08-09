package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 四：CursorOnly 事务行为测试 — textEnabled=false + cursorEnabled=true 时
 * 文字静态更新、光标仍由同一 FrameClock 平滑移动。
 *
 * 结构契约（方法存在性）已移入
 * [com.xiwei.sujian.arch.CursorOnlyTransactionArchitectureTest]；本文件只保留运行时行为：
 * - 无事务时 hasActiveAnimation 返回 false；
 * - reduceMotion + cursor off → 纯静态更新。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CursorOnlyTransactionTest {
    private fun createEngine(): AndroidTextAnimationEngine {
        return AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            ChoreographerAnimationTimeSource(),
            TransactionIdSource(),
        )
    }

    @Test
    fun hasActiveAnimationReturnsFalseWhenNoTransaction() {
        val engine = createEngine()
        assertFalse("No active transaction", engine.hasActiveAnimation())
    }

    @Test
    fun hasActiveAnimationChecksCursorTimelineNotJustText() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        assertFalse("No transaction → not active", engine.hasActiveAnimation())
    }

    @Test
    fun textSuppressedAndCursorDisabledProducesStaticUpdate() {
        val engine = createEngine()
        engine.setReduceMotion(true)
        engine.setSmoothCursor(false, 80L)
        assertFalse("Static update → no active animation", engine.hasActiveAnimation())
    }

    @Test
    fun reduceMotionAndCursorDisabledProducesStaticUpdate() {
        val engine = createEngine()
        engine.setReduceMotion(true)
        engine.setSmoothCursor(false, 80L)
        assertFalse("Reduce-motion + cursor off → no active animation", engine.hasActiveAnimation())
    }

    /**
     * #605: CURSOR_ONLY 事务使用独立 cursorTimeline，光标是 fromX/fromY -> toX/toY
     * 直线插值，不经过 TextRevealSpec。
     */
    @Test
    fun cursorOnlyTransactionUsesIndependentCursorTimelineWithLinearInterpolation() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 100L)
        engine.setCoordinatedAnimationEnabled(true)
        val tx =
            cursorOnlyTransaction(
                transactionId = 1L,
                durationMs = 100L,
                fromX = 10f,
                toX = 110f,
            )
        engine.submit(tx, submittedAtMs = 0L)
        // 在 50ms 时，cursorTimeline progress = 0.5
        val cursorProgress = engine.getCursorProgress(50L)
        assertEquals(0.5f, cursorProgress!!, 0.01f)
        // captureFrame 中的 cursorRect 应该是直线插值：x = 10 + (110-10)*0.5 = 60
        val frame = engine.captureFrame(50L)
        assertNotNull("Frame must be captured", frame)
        val cursorRect = frame!!.cursorRect
        assertNotNull("Cursor rect must exist", cursorRect)
        assertEquals(60f, cursorRect!!.left, 0.01f)
        assertEquals(5f, cursorRect.top, 0.01f)
    }

    /**
     * #605: CURSOR_ONLY 事务不经过 TextRevealSpec — 没有 animatedSlices，
     * sliceVisualStates 为空，没有 revealFraction。
     */
    @Test
    fun cursorOnlyTransactionDoesNotUseRevealSpec() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 100L)
        val tx =
            cursorOnlyTransaction(
                transactionId = 1L,
                durationMs = 100L,
                fromX = 0f,
                toX = 100f,
            )
        engine.submit(tx, submittedAtMs = 0L)
        val frame = engine.captureFrame(50L)
        assertNotNull("Frame must be captured", frame)
        // CURSOR_ONLY 事务没有 animatedSlices，所以 sliceVisualStates 为空
        assertTrue("No slices in cursor-only transaction", frame!!.sliceVisualStates.isEmpty())
    }

    /**
     * #605: CURSOR_ONLY 事务在协同模式下仍使用独立 cursorTimeline
     * （因为没有文字事务，hasTextMotion=false）。
     */
    @Test
    fun cursorOnlyInCoordinatedModeStillUsesIndependentTimeline() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        engine.setCoordinatedAnimationEnabled(true)
        val tx =
            cursorOnlyTransaction(
                transactionId = 1L,
                durationMs = 80L,
                fromX = 0f,
                toX = 80f,
            )
        engine.submit(tx, submittedAtMs = 0L)
        // CURSOR_ONLY：使用独立 cursorTimeline（80ms），在 40ms 时 progress = 0.5
        val cursorProgress = engine.getCursorProgress(40L)
        assertEquals(0.5f, cursorProgress!!, 0.01f)
    }

    private fun cursorOnlyTransaction(
        transactionId: Long,
        durationMs: Long,
        fromX: Float = 0f,
        toX: Float = 100f,
    ): PreparedVisualTransaction {
        return PreparedVisualTransaction(
            transactionId = transactionId,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = emptyList(),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition =
                PreparedVisualTransaction.CursorTransition(
                    fromX = fromX,
                    fromY = 5f,
                    fromHeight = 20f,
                    toX = toX,
                    toY = 5f,
                    toHeight = 20f,
                    shouldAnimate = true,
                ),
            durationMs = durationMs,
        )
    }
}
