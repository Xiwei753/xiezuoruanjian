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
 * Issue #728 评论 5755928697：三个确定问题的收口测试 —
 * 1. [redirectCaretTo] 保留现有 glyph channel fraction。
 * 3. 多 unit 区间映射，光标跟着吞吐。
 *
 * Issue #735 评论 5773604666 问题2：删除双 duration —
 * caret 和 glyph 共用同一个 durationNanos/progress，不再有独立的 caret/glyph 时长。
 * 删除 `sample_coordinatedFalse_caretAndGlyphIndependent`、
 * `sample_caretInstantGlyphRunning_notFinished`、`sample_glyphInstantCaretRunning_notFinished`、
 * `isFinished_requiresBothCaretAndGlyphFinished` 等依赖双 duration 的测试。
 *
 * 一笔编辑一只钟：caret 移动和文字吞吐共用同一个 progress。
 */
@Suppress("TooManyFunctions")
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
                insertedUnitKeys = listOf(1L, 2L),
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
                deletedUnitKeys = listOf(1L),
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

    /**
     * Issue #728 评论 5755928697 问题3：混合 edit 时 inserted 占前半段 [0, 0.5]，
     * deleted 占后半段 [0.5, 1]。光标先经过插入区吐字，再经过删除区吞字。
     */
    @Test
    fun forEdit_handlesBothInsertedAndDeletedUnits() {
        val motion =
            ComposeEditMotion.forEdit(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                deletedUnitKeys = listOf(2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // progress=0.25：inserted key=1 在区间 [0, 0.5] 中点，local=0.5，fraction=0.5
        val quarterSample = motion.sample(startTime + duration / 4)
        assertEquals(0.5f, quarterSample.unitClipFractions[1L]!!, 0.001f)
        // deleted key=2 还没开始它的区间 [0.5, 1]，fraction=1（完全可见）
        assertEquals(1f, quarterSample.unitClipFractions[2L]!!, 0.001f)

        // progress=0.5：inserted key=1 已走完 [0, 0.5]，fraction=1（完全吐出）
        val midSample = motion.sample(startTime + duration / 2)
        assertEquals(1f, midSample.unitClipFractions[1L]!!, 0.001f)
        // deleted key=2 刚到区间起点 [0.5, 1]，还没开始吞，fraction=1
        assertEquals(1f, midSample.unitClipFractions[2L]!!, 0.001f)

        // progress=0.75：inserted key=1 已走完，fraction=1
        val threeQuarterSample = motion.sample(startTime + 3 * duration / 4)
        assertEquals(1f, threeQuarterSample.unitClipFractions[1L]!!, 0.001f)
        // deleted key=2 在区间 [0.5, 1] 中点，local=0.5，fraction=1+(0-1)*0.5=0.5
        assertEquals(0.5f, threeQuarterSample.unitClipFractions[2L]!!, 0.001f)
    }

    /**
     * Issue #728 评论 5755928697 问题3：多个 inserted unit 依次吐字。
     * n=2，key=1 区间 [0, 0.5]，key=2 区间 [0.5, 1]。
     */
    @Test
    fun forInsert_multipleUnitsRevealSequentially() {
        val motion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L, 2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // progress=0.25：key=1 在 [0, 0.5] 中点 local=0.5 fraction=0.5；
        // key=2 还没开始 [0.5, 1] fraction=0
        val quarterSample = motion.sample(startTime + duration / 4)
        assertEquals(0.5f, quarterSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(0f, quarterSample.unitClipFractions[2L]!!, 0.001f)

        // progress=0.5：key=1 已走完 fraction=1；key=2 刚开始 fraction=0
        val midSample = motion.sample(startTime + duration / 2)
        assertEquals(1f, midSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(0f, midSample.unitClipFractions[2L]!!, 0.001f)

        // progress=0.75：key=1 已走完 fraction=1；key=2 在 [0.5, 1] 中点 fraction=0.5
        val threeQuarterSample = motion.sample(startTime + 3 * duration / 4)
        assertEquals(1f, threeQuarterSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(0.5f, threeQuarterSample.unitClipFractions[2L]!!, 0.001f)
    }

    /**
     * Issue #728 评论 5755928697 问题3：多个 deleted unit 反向吞字（先吞最右边）。
     * n=2，sorted=[1, 2]，key=1 区间 [(2-1-0)/2, (2-0)/2]=[0.5, 1]，
     * key=2 区间 [(2-1-1)/2, (2-1)/2]=[0, 0.5]。key=2 先吞（光标从右往左）。
     */
    @Test
    fun forDelete_multipleUnitsConcealReversed() {
        val motion =
            ComposeEditMotion.forDelete(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedUnitKeys = listOf(1L, 2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // progress=0.25：key=2 在 [0, 0.5] 中点 local=0.5 fraction=1+(0-1)*0.5=0.5；
        // key=1 还没开始 [0.5, 1] fraction=1
        val quarterSample = motion.sample(startTime + duration / 4)
        assertEquals(1f, quarterSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(0.5f, quarterSample.unitClipFractions[2L]!!, 0.001f)

        // progress=0.5：key=2 已走完 fraction=0；key=1 刚开始 fraction=1
        val midSample = motion.sample(startTime + duration / 2)
        assertEquals(1f, midSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(0f, midSample.unitClipFractions[2L]!!, 0.001f)

        // progress=0.75：key=2 已走完 fraction=0；key=1 在 [0.5, 1] 中点 fraction=0.5
        val threeQuarterSample = motion.sample(startTime + 3 * duration / 4)
        assertEquals(0.5f, threeQuarterSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(0f, threeQuarterSample.unitClipFractions[2L]!!, 0.001f)
    }

    /**
     * Issue #728 评论 5755928697 问题3：单个 unit 退化为 [0, 1] 全区间。
     */
    @Test
    fun forInsert_singleUnitUsesFullRange() {
        val motion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // mid progress → fraction=0.5（退化为原行为）
        val midSample = motion.sample(startTime + duration / 2)
        assertEquals(0.5f, midSample.unitClipFractions[1L]!!, 0.001f)
    }

    @Test
    fun sample_caretRectInterpolatesFromOriginToTarget() {
        val motion =
            ComposeEditMotion.forSelectionMove(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                frameTimeNanos = startTime,
                caretDurationNanos = duration,
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
                insertedUnitKeys = listOf(1L),
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
                insertedUnitKeys = listOf(1L),
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
                insertedUnitKeys = listOf(1L),
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
                insertedUnitKeys = listOf(1L),
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
                newInsertedUnitKeys = listOf(1L, 2L),
                newDeletedUnitKeys = emptyList(),
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
                insertedUnitKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val midTime = startTime + duration / 2

        // Redirect with a new deleted unit
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = listOf(1L),
                newDeletedUnitKeys = listOf(3L),
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
                insertedUnitKeys = listOf(1L),
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
                newInsertedUnitKeys = listOf(2L),
                newDeletedUnitKeys = emptyList(),
                frameTimeNanos = finishTime,
                durationNanos = duration,
            )
        // At start of redirected motion: caret should be at newOrigin (targetRect)
        val startSample = motion2.sample(finishTime)
        assertEquals(targetRect, startSample.caretRect)
    }

    /**
     * Issue #728 评论 5755928697 问题1：redirectCaretTo 保留现有 unit channel fraction，
     * 不丢弃正在吐的字。只换 caret 目标，文字继续从当前 fraction 走到原 to。
     */
    @Test
    fun redirectCaretTo_preservesExistingUnitFractions() {
        // motion1: insert unit 1 和 2，duration 100ms
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L, 2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 25ms 时：progress=0.25
        // key=1 区间 [0, 0.5]，local=0.5，fraction=0.5
        // key=2 区间 [0.5, 1]，还没开始，fraction=0
        val midTime = startTime + duration / 4
        val midSample = motion1.sample(midTime)
        assertEquals(0.5f, midSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(0f, midSample.unitClipFractions[2L]!!, 0.001f)

        // 移动光标（pending selection）— 用 redirectCaretTo，不传 newInserted/newDeleted
        val newCaretTarget = Rect(left = 5f, top = 0f, right = 7f, bottom = 20f)
        val motion2 =
            motion1.redirectCaretTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = newCaretTarget,
                frameTimeNanos = midTime,
                durationNanos = duration,
            )
        // redirect 后立即 sample：unit fraction 应该保持当前值（不丢字）
        val redirectedSample = motion2.sample(midTime)
        assertEquals(0.5f, redirectedSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(0f, redirectedSample.unitClipFractions[2L]!!, 0.001f)

        // 继续跑到终点：两个 unit 都应该到 1（文字继续吐完，没被丢弃）
        val endSample = motion2.sample(midTime + duration)
        assertEquals(1f, endSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(1f, endSample.unitClipFractions[2L]!!, 0.001f)
    }

    /**
     * Issue #728 评论 5755928697 问题1：redirectCaretTo 已 finished 时用 newOriginCaretRect。
     */
    @Test
    fun redirectCaretTo_finishedMotionUsesNewOrigin() {
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val finishTime = startTime + duration
        assertTrue(motion1.isFinished(finishTime))

        val newTarget = Rect(left = 60f, top = 0f, right = 62f, bottom = 20f)
        val motion2 =
            motion1.redirectCaretTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = newTarget,
                frameTimeNanos = finishTime,
                durationNanos = duration,
            )
        val startSample = motion2.sample(finishTime)
        assertEquals(targetRect, startSample.caretRect)
    }

    /**
     * Issue #728 评论 5755928697 问题1：redirectCaretTo 后 caret 从当前 rect 去新 target。
     */
    @Test
    fun redirectCaretTo_caretMovesFromCurrentToNewTarget() {
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 50ms：caret 在 midpoint (left=20)
        val midTime = startTime + duration / 2
        val midSample = motion1.sample(midTime)
        assertEquals(20f, midSample.caretRect.left, 0.001f)

        val newTarget = Rect(left = 60f, top = 0f, right = 62f, bottom = 20f)
        val motion2 =
            motion1.redirectCaretTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = newTarget,
                frameTimeNanos = midTime,
                durationNanos = duration,
            )
        // redirect 后立即 sample：caret 在当前 rect（midpoint left=20）
        val startSample = motion2.sample(midTime)
        assertEquals(20f, startSample.caretRect.left, 0.001f)
        // 跑到终点：caret 到新 target (left=60)
        val endSample = motion2.sample(midTime + duration)
        assertEquals(60f, endSample.caretRect.left, 0.001f)
    }

    @Test
    fun forSelectionMove_hasNoUnitChannels() {
        val motion =
            ComposeEditMotion.forSelectionMove(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                frameTimeNanos = startTime,
                caretDurationNanos = duration,
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
                insertedUnitKeys = listOf(1L),
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
