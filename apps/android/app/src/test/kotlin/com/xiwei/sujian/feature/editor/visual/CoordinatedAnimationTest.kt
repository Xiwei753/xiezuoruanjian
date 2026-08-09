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
 * #595 三/四/九：协同动画契约测试 — 验证 coordinated 设置和 reduceMotion 设置
 * 真正进入 AndroidTextAnimationEngine，不再只是死开关。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CoordinatedAnimationTest {
    private fun createEngine(): AndroidTextAnimationEngine {
        return AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            ChoreographerAnimationTimeSource(),
            TransactionIdSource(),
        )
    }

    @Test
    fun coordinatedEnabledByDefault() {
        val engine = createEngine()
        assertTrue("Coordinated enabled by default", engine.isCoordinatedAnimationEnabled())
    }

    @Test
    fun setCoordinatedAnimationEnabledPropagatesToEngine() {
        val engine = createEngine()
        engine.setCoordinatedAnimationEnabled(false)
        assertFalse("Engine reflects coordinated disabled", engine.isCoordinatedAnimationEnabled())
        engine.setCoordinatedAnimationEnabled(true)
        assertTrue("Engine reflects coordinated re-enabled", engine.isCoordinatedAnimationEnabled())
    }

    @Test
    fun reduceMotionDisabledByDefault() {
        val engine = createEngine()
        assertFalse("Reduce motion disabled by default", engine.isReduceMotion())
    }

    @Test
    fun setReduceMotionPropagatesToEngineAndSuppressesAnimation() {
        val engine = createEngine()
        engine.setReduceMotion(true)
        assertTrue("Engine reflects reduce motion enabled", engine.isReduceMotion())
        assertEquals(
            "Animation policy suppressed",
            TextAnimationPolicy.SYSTEM_SUPPRESSED,
            engine.getAnimationPolicy(),
        )
    }

    @Test
    fun setReduceMotionFalseDoesNotSuppressAnimation() {
        val engine = createEngine()
        engine.setReduceMotion(true)
        engine.setReduceMotion(false)
        assertFalse("Reduce motion disabled", engine.isReduceMotion())
    }

    @Test
    fun pauseWithoutActiveTransactionIsNoOp() {
        val engine = createEngine()
        engine.pause(100L)
        assertFalse("No active transaction means not paused", engine.isPaused())
    }

    /**
     * #605: 协同模式 + 有文字事务时，光标 progress 跟随主 timeline，
     * 不创建独立 cursorTimeline。
     */
    @Test
    fun coordinatedModeWithTextMotionCursorProgressFollowsMainTimeline() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        engine.setCoordinatedAnimationEnabled(true)
        engine.submit(textMotionTransaction(transactionId = 1L, durationMs = 100L), submittedAtMs = 0L)
        // 协同模式 + 有文字事务：cursorProgress = 主 timeline progress
        // 在 50ms 时，主 timeline progress = 0.5
        val cursorProgress = engine.getCursorProgress(50L)
        assertEquals(0.5f, cursorProgress!!, 0.01f)
    }

    /**
     * #605 WEAK: 协同模式 + 有文字事务时，显式锁住 textProgress == cursorProgress
     * 同一 timeline 契约 — 不只断言 cursorProgress=0.5，还断言 captureFrame().progress
     * 与 cursorProgress 相等，且都等于 0.5。
     */
    @Test
    fun coordinatedModeTextProgressEqualsCursorProgress() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        engine.setCoordinatedAnimationEnabled(true)
        engine.submit(textMotionTransaction(transactionId = 1L, durationMs = 100L), submittedAtMs = 0L)
        // 在 50ms 时，主 timeline progress = 0.5
        val frame = engine.captureFrame(50L)
        val cursorProgress = engine.getCursorProgress(50L)
        assertNotNull("Frame must be captured in coordinated mode", frame)
        assertNotNull("Cursor progress must exist in coordinated mode", cursorProgress)
        // 显式锁住同一 timeline 契约：text progress == cursor progress
        assertEquals(
            "text progress must equal cursor progress in coordinated mode",
            frame!!.progress,
            cursorProgress!!,
            0.01f,
        )
        // 锁住具体值，而非仅相等 — 防止两者同时漂移到错误值
        assertEquals("text progress must be 0.5 at 50ms", 0.5f, frame.progress, 0.01f)
        assertEquals("cursor progress must be 0.5 at 50ms", 0.5f, cursorProgress, 0.01f)
    }

    /**
     * #605: 非协同模式 + 有文字事务时，光标使用独立 cursorTimeline。
     */
    @Test
    fun nonCoordinatedModeWithTextMotionCursorProgressUsesIndependentTimeline() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        engine.setCoordinatedAnimationEnabled(false)
        engine.submit(textMotionTransaction(transactionId = 1L, durationMs = 100L), submittedAtMs = 0L)
        // 非协同模式：cursorProgress = 独立 cursorTimeline progress
        // cursorTimeline 时长 = 80ms，在 40ms 时 progress = 0.5
        val cursorProgress = engine.getCursorProgress(40L)
        assertEquals(0.5f, cursorProgress!!, 0.01f)
    }

    /**
     * #605: 协同模式 + CURSOR_ONLY 事务（无文字切片和 blockShifts）时，
     * 光标仍使用独立 cursorTimeline。
     */
    @Test
    fun coordinatedModeCursorOnlyUsesIndependentTimeline() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        engine.setCoordinatedAnimationEnabled(true)
        engine.submit(cursorOnlyTransaction(transactionId = 1L, durationMs = 80L), submittedAtMs = 0L)
        // CURSOR_ONLY：使用独立 cursorTimeline
        // 在 40ms 时，cursorTimeline progress = 0.5
        val cursorProgress = engine.getCursorProgress(40L)
        assertEquals(0.5f, cursorProgress!!, 0.01f)
    }

    /**
     * #605: 协同模式 + 有文字事务时，isCursorTimelineCompleted 跟随主 timeline。
     */
    @Test
    fun coordinatedModeWithTextMotionCursorCompletedFollowsMainTimeline() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        engine.setCoordinatedAnimationEnabled(true)
        engine.submit(textMotionTransaction(transactionId = 1L, durationMs = 100L), submittedAtMs = 0L)
        // 主 timeline 100ms，在 50ms 时未完成
        assertFalse("Cursor not completed when main timeline not finished", engine.isCursorTimelineCompleted(50L))
        // 在 100ms 时完成
        assertTrue("Cursor completed when main timeline finished", engine.isCursorTimelineCompleted(100L))
    }

    private fun textMotionTransaction(
        transactionId: Long,
        durationMs: Long,
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
                    fromX = 0f,
                    fromY = 0f,
                    fromHeight = 20f,
                    toX = 100f,
                    toY = 0f,
                    toHeight = 20f,
                    shouldAnimate = true,
                ),
            durationMs = durationMs,
            blockShifts =
                listOf(
                    PreparedVisualTransaction.BlockShift(
                        startLineIndex = 1,
                        endLineIndexExclusive = 3,
                        top = 40f,
                        bottom = 80f,
                        left = 0f,
                        right = 200f,
                        deltaY = 20f,
                    ),
                ),
        )
    }

    private fun cursorOnlyTransaction(
        transactionId: Long,
        durationMs: Long,
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
                    fromX = 0f,
                    fromY = 0f,
                    fromHeight = 20f,
                    toX = 100f,
                    toY = 0f,
                    toHeight = 20f,
                    shouldAnimate = true,
                ),
            durationMs = durationMs,
        )
    }
}
