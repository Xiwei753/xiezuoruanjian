package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Issue #728 评论 5754045689：[ComposeEditMotion] 单元测试 —
 * 验证统一编辑 motion 的 sample、redirectTo、isFinished 语义。
 *
 * 一笔编辑一只钟：caret 移动和文字吞吐共用同一个 progress。
 */
class ComposeEditMotionTest {
    private val originRect = Rect(left = 10f, top = 0f, right = 12f, bottom = 20f)
    private val targetRect = Rect(left = 30f, top = 0f, right = 32f, bottom = 20f)
    private val startTime = 1_000_000_000L
    private val duration = 100_000_000L // 100ms in nanos

    @Test
    fun forInsert_createsChannelsFromZeroToOne() {
        val motion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = setOf(1L, 2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // At start (progress=0): fraction should be 0 (invisible)
        val startSample = motion.sample(startTime)
        assertEquals(0f, startSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(0f, startSample.unitClipFractions[2L]!!, 0.001f)
        assertFalse(startSample.finished)

        // At end (progress=1): fraction should be 1 (fully visible)
        val endSample = motion.sample(startTime + duration)
        assertEquals(1f, endSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(1f, endSample.unitClipFractions[2L]!!, 0.001f)
        assertTrue(endSample.finished)
    }

    @Test
    fun forDelete_createsChannelsFromOneToZero() {
        val motion =
            ComposeEditMotion.forDelete(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedUnitKeys = setOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // At start (progress=0): fraction should be 1 (fully visible)
        val startSample = motion.sample(startTime)
        assertEquals(1f, startSample.unitClipFractions[1L]!!, 0.001f)

        // At end (progress=1): fraction should be 0 (invisible)
        val endSample = motion.sample(startTime + duration)
        assertEquals(0f, endSample.unitClipFractions[1L]!!, 0.001f)
        assertTrue(endSample.finished)
    }

    @Test
    fun forEdit_handlesBothInsertedAndDeletedUnits() {
        val motion =
            ComposeEditMotion.forEdit(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = setOf(1L),
                deletedUnitKeys = setOf(2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val midSample = motion.sample(startTime + duration / 2)
        // Inserted unit: 0 -> 1, at mid = 0.5
        assertEquals(0.5f, midSample.unitClipFractions[1L]!!, 0.001f)
        // Deleted unit: 1 -> 0, at mid = 0.5
        assertEquals(0.5f, midSample.unitClipFractions[2L]!!, 0.001f)
    }

    @Test
    fun sample_caretRectInterpolatesFromOriginToTarget() {
        val motion =
            ComposeEditMotion.forSelectionMove(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // At start: caret at origin
        val startSample = motion.sample(startTime)
        assertEquals(originRect, startSample.caretRect)

        // At end: caret at target
        val endSample = motion.sample(startTime + duration)
        assertEquals(targetRect, endSample.caretRect)

        // At mid: caret at midpoint
        val midSample = motion.sample(startTime + duration / 2)
        assertEquals(20f, midSample.caretRect.left, 0.001f) // (10+30)/2
    }

    @Test
    fun sample_instantDurationCompletesImmediately() {
        val motion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = setOf(1L),
                frameTimeNanos = startTime,
                durationNanos = 0L,
            )
        val sample = motion.sample(startTime)
        assertTrue(sample.finished)
        assertEquals(targetRect, sample.caretRect)
        assertEquals(1f, sample.unitClipFractions[1L]!!, 0.001f)
    }

    @Test
    fun isFinished_matchesSampleFinishedSemantics() {
        val motion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = setOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        assertFalse(motion.isFinished(startTime))
        assertFalse(motion.isFinished(startTime + duration / 2))
        assertTrue(motion.isFinished(startTime + duration))
    }

    @Test
    fun isFinished_instantDurationAlwaysTrue() {
        val motion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = setOf(1L),
                frameTimeNanos = startTime,
                durationNanos = 0L,
            )
        assertTrue(motion.isFinished(startTime))
    }

    @Test
    fun redirectTo_existingUnitContinuesFromCurrentFraction() {
        // First motion: insert unit 1, 0→1
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = setOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // Sample at 50% — unit 1 fraction = 0.5
        val midTime = startTime + duration / 2
        val midSample = motion1.sample(midTime)
        assertEquals(0.5f, midSample.unitClipFractions[1L]!!, 0.001f)

        // Redirect: new insert includes unit 1 again + new unit 2
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = Rect(left = 50f, top = 0f, right = 52f, bottom = 20f),
                newInsertedUnitKeys = setOf(1L, 2L),
                newDeletedUnitKeys = emptySet(),
                frameTimeNanos = midTime,
                durationNanos = duration,
            )
        // Unit 1 should continue from 0.5 (not restart from 0)
        val redirectedSample = motion2.sample(midTime)
        assertEquals(0.5f, redirectedSample.unitClipFractions[1L]!!, 0.001f)
        // Unit 2 is new, starts from 0
        assertEquals(0f, redirectedSample.unitClipFractions[2L]!!, 0.001f)
    }

    @Test
    fun redirectTo_newUnitStartsFromZeroOrOne() {
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = setOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val midTime = startTime + duration / 2

        // Redirect with a new deleted unit
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = setOf(1L),
                newDeletedUnitKeys = setOf(3L),
                frameTimeNanos = midTime,
                durationNanos = duration,
            )
        val redirectedSample = motion2.sample(midTime)
        // New deleted unit 3 starts from 1 (fully visible, will be swallowed)
        assertNotNull(redirectedSample.unitClipFractions[3L])
        assertEquals(1f, redirectedSample.unitClipFractions[3L]!!, 0.001f)
    }

    @Test
    fun redirectTo_finishedMotionUsesNewOrigin() {
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = setOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // Let motion1 finish
        val finishTime = startTime + duration
        assertTrue(motion1.isFinished(finishTime))

        // Redirect after finish — should use newOriginCaretRect
        val newTarget = Rect(left = 60f, top = 0f, right = 62f, bottom = 20f)
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = newTarget,
                newInsertedUnitKeys = setOf(2L),
                newDeletedUnitKeys = emptySet(),
                frameTimeNanos = finishTime,
                durationNanos = duration,
            )
        // At start of redirected motion: caret should be at newOrigin (targetRect)
        val startSample = motion2.sample(finishTime)
        assertEquals(targetRect, startSample.caretRect)
    }

    @Test
    fun forSelectionMove_hasNoUnitChannels() {
        val motion =
            ComposeEditMotion.forSelectionMove(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val sample = motion.sample(startTime + duration / 2)
        assertTrue(sample.unitClipFractions.isEmpty())
        // Caret still interpolates
        assertEquals(20f, sample.caretRect.left, 0.001f)
    }

    @Test
    fun sample_beforeStartTimeReturnsOriginFractions() {
        val motion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = setOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // Sample before start time — should return origin values
        val beforeSample = motion.sample(startTime - 1000L)
        assertEquals(originRect, beforeSample.caretRect)
        assertEquals(0f, beforeSample.unitClipFractions[1L]!!, 0.001f)
        assertFalse(beforeSample.finished)
    }
}
