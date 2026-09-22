package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Issue #728 评论 5760112985：[ComposeEditMotion.redirectTo] 角色目标与统一 schedule 专项测试 —
 * 验证两个确定运行错误的修复：
 * 1. Inserted→DeletedGhost 角色变化时，目标从 currentFraction 走向当前角色目标
 *    （inserted: 1, deleted: 0），不再沿用旧 channel 的 to；Completed+角色变化不再固定旧终值，
 *    而是走新剩余动画。
 * 2. redirect 把 inserted（正文顺序）+deleted（sourceRange 反序，右往左吞）合成一条有序 traversal list，
 *    只调用一次剩余 schedule builder 统一归一化到一条 [0,1]，不再分别建 inserted/deleted 两条 schedule。
 *
 * 从 [ComposeEditMotionTest] 拆出，避免触发 detekt LargeClass（阈值 500 行）。
 */
class ComposeEditMotionRedirectRoleTest {
    private val originRect = Rect(left = 10f, top = 0f, right = 12f, bottom = 20f)
    private val targetRect = Rect(left = 30f, top = 0f, right = 32f, bottom = 20f)
    private val startTime = 1_000_000_000L
    private val duration = 100_000_000L // 100ms in nanos

    /**
     * Issue #728 评论 5760112985 问题1：Inserted→DeletedGhost 角色变化时，
     * 目标应从 currentFraction 走向 0（吞字），不再沿用旧 channel 的 to=1（继续吐字）。
     *
     * Issue #728 评论 5760741452 问题1 增强：角色反转后 glyph 与 caret 必须**同时**到达终点，
     * 不能 glyph 提前吞完。单 unit 反转时新 channel 应是 [0, 1] 全区间，
     * 新 motion 60% 时 fraction ≈ 0.16（0.4+(0-0.4)*0.6），100% 时 fraction=0；
     * caret 也在 100% 时到目标（left=10），60% 时还在 13.2。
     */
    @Test
    fun redirectTo_insertedBecomesDeletedGhost_targetsZeroNotOldToOne() {
        // 1. forInsert 创建 unit key=1（0→1，duration=100ms）
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 2. sample 到 40% 时间：unit 1 fraction≈0.4（InProgress，旧 ch.to=1）
        val midTime = startTime + duration * 2 / 5 // 40ms
        val midSample = motion1.sample(midTime)
        assertEquals(0.4f, midSample.unitClipFractions[1L]!!, 0.001f)

        // 3. redirectTo：key=1 现在是 deleted（desiredTo=0，不再用旧 ch.to=1）
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = emptyList(),
                newDeletedUnitKeys = listOf(1L),
                frameTimeNanos = midTime,
                durationNanos = duration,
            )

        // 4. redirect 后立即 sample：key=1 fraction≈0.4（从当前继续，不是从 1 开始）
        val redirectedStart = motion2.sample(midTime)
        assertEquals(0.4f, redirectedStart.unitClipFractions[1L]!!, 0.001f)

        // 5. 走一小段后 fraction 下降（往 0 走，不是往 1 走）。
        // 新 motion 50% 时间：glyphProgress=0.5，key=1 在 [0, 1] 区间（角色反转后全区间），
        // localProgress=0.5，fraction=0.4+(0-0.4)*0.5=0.2 < 0.4
        val afterHalf = motion2.sample(midTime + duration / 2)
        val fractionAfter = afterHalf.unitClipFractions[1L]!!
        // 关键断言：desiredTo=0 生效，fraction 在减少（吞字方向），不再用旧 ch.to=1（否则会往 1 增长）
        assertTrue("fraction 应下降（往 0 走），实际=$fractionAfter", fractionAfter < 0.4f)

        // 6. Issue #728 评论 5760741452 问题1 核心断言：glyph 与 caret 同步到达终点。
        // 新 motion 60% 时间：glyphProgress=0.6，key=1 在 [0, 1] 全区间，
        // localProgress=0.6，fraction=0.4+(0-0.4)*0.6=0.16（不是 0！）
        // 旧 bug 用旧方向 remaining=0.6 作 endProgress，60% 时 fraction=0（提前吞完）。
        val atSixtyPercent = motion2.sample(midTime + duration * 3 / 5)
        val fractionAtSixty = atSixtyPercent.unitClipFractions[1L]!!
        assertEquals(
            "新 motion 60% 时 fraction 应≈0.16（0.4+(0-0.4)*0.6），glyph 不应提前吞完，实际=$fractionAtSixty",
            0.16f,
            fractionAtSixty,
            0.001f,
        )
        // 60% 时 caret 还在走：origin=18（40% 时 caret.left），target=10，60% 插值=13.2
        val caretAtSixty = atSixtyPercent.caretRect
        assertEquals(
            "caret 60% 时应在 13.2（还没到目标 10），与 glyph 同步",
            13.2f,
            caretAtSixty.left,
            0.001f,
        )
        // glyph 还没到终点（fraction=0.16 > 0），caret 还没到终点（left=13.2 > 10）— 同步
        assertTrue("glyph 60% 时还没到终点（fraction=$fractionAtSixty > 0）", fractionAtSixty > 0.001f)
        assertTrue("caret 60% 时还没到终点（left=${caretAtSixty.left} > 10）", caretAtSixty.left > 10.001f)

        // 7. 新 motion 100% 时 glyph 和 caret 同时到达终点
        val atFull = motion2.sample(midTime + duration)
        assertEquals(
            "新 motion 100% 时 glyph fraction 应为 0（终点）",
            0f,
            atFull.unitClipFractions[1L]!!,
            0.001f,
        )
        assertEquals(
            "新 motion 100% 时 caret 应到目标（left=10），与 glyph 同时到达",
            10f,
            atFull.caretRect.left,
            0.001f,
        )
    }

    /**
     * Issue #728 评论 5760112985 问题1：Completed inserted→deleted 角色变化时，
     * 不再固定旧终值 1，而是从 1 走到 0（新剩余动画）。
     */
    @Test
    fun redirectTo_completedInsertedBecomesDeleted_reAnimatesToZero() {
        // 1. forInsert 创建 unit key=1（0→1，duration=100ms）
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 2. sample 到 100% 时间（Completed，fraction=1.0）
        val finishTime = startTime + duration
        val finishSample = motion1.sample(finishTime)
        assertEquals(1f, finishSample.unitClipFractions[1L]!!, 0.001f)
        assertTrue(motion1.isFinished(finishTime))

        // 3. redirectTo：key=1 现在是 deleted
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = emptyList(),
                newDeletedUnitKeys = listOf(1L),
                frameTimeNanos = finishTime,
                durationNanos = duration,
            )

        // 4. redirect 后立即 sample：key=1 fraction=1.0（从当前开始）
        val redirectedStart = motion2.sample(finishTime)
        assertEquals(1f, redirectedStart.unitClipFractions[1L]!!, 0.001f)

        // 5. 走一段后 fraction 下降（新剩余动画，不是固定在 1）
        // 50% 时间：glyphProgress=0.5，key=1 在 [0, 1] 区间，fraction=1+(0-1)*0.5=0.5
        val afterHalf = motion2.sample(finishTime + duration / 2)
        val fractionHalf = afterHalf.unitClipFractions[1L]!!
        assertTrue("Completed+角色变化应重新动画，fraction 应下降，实际=$fractionHalf", fractionHalf < 1f)
        assertEquals(0.5f, fractionHalf, 0.001f)

        // 6. 走到终点 fraction=0（从 1 走到 0，不再固定旧终值 1）
        val afterFull = motion2.sample(finishTime + duration)
        assertEquals(0f, afterFull.unitClipFractions[1L]!!, 0.001f)
    }

    /**
     * Issue #728 评论 5760112985 问题2：redirect 后 deleted 保持右往左吞顺序，
     * 不被翻成左往右。三个 deleted unit keys=10,11,12（sourceRange.start 升序），
     * 最右边的 key=12 应先开始吞，最左边的 key=10 应最后。
     */
    @Test
    fun redirectTo_deletedUnitsKeepRightToLeftOrder() {
        // 1. 三个 deleted unit keys=10,11,12（sourceRange.start 升序，即正文位置左→右）
        // 2. forDelete 创建 motion，验证反向分配：
        //    key=12（最右）区间 [0, 1/3]，key=11 区间 [1/3, 2/3]，key=10（最左）区间 [2/3, 1]
        val motion1 =
            ComposeEditMotion.forDelete(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedUnitKeys = listOf(10L, 11L, 12L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // sample 到 duration/6 时间（glyphProgress=1/6，在 key=12 区间 [0, 1/3] 中段）：
        // key=12 localProgress=0.5 fraction=0.5，key=10/11 fraction=1（未开始）
        val midTime = startTime + duration / 6
        val midSample = motion1.sample(midTime)
        assertEquals(0.5f, midSample.unitClipFractions[12L]!!, 0.001f)
        assertEquals(1f, midSample.unitClipFractions[11L]!!, 0.001f)
        assertEquals(1f, midSample.unitClipFractions[10L]!!, 0.001f)

        // 3. redirectTo：newDeletedUnitKeys=listOf(10L,11L,12L)（升序，模拟 activeEditUnits 返回）
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = emptyList(),
                newDeletedUnitKeys = listOf(10L, 11L, 12L),
                frameTimeNanos = midTime,
                durationNanos = duration,
            )

        // 4. redirect 后 sample 到新 motion duration/6 时间：
        // 最右边的 key=12 应该先开始吞（fraction≈1/3 < 1），最左边的 key=10 应该还没开始（fraction=1）
        val redirectedSample = motion2.sample(midTime + duration / 6)
        val fraction12 = redirectedSample.unitClipFractions[12L]!!
        val fraction10 = redirectedSample.unitClipFractions[10L]!!
        assertTrue("最右 key=12 应先开始吞（fraction<1），实际=$fraction12", fraction12 < 1f)
        assertEquals("最左 key=10 应还没开始（fraction=1），实际=$fraction10", 1f, fraction10, 0.001f)
        // 证明 redirect 后 deleted 仍是右往左，没被翻成左往右
        assertTrue("key=12 应比 key=10 先吞（fraction 更小）", fraction12 < fraction10)
    }

    /**
     * Issue #728 评论 5760112985 问题2：mixed edit redirect 后 inserted/deleted 在一条 schedule 上，
     * 不再各自从 0 同时开始（不重叠）。inserted key=1 在前半段先动，deleted key=2 在后半段后动。
     */
    @Test
    fun redirectTo_mixedEdit_singleScheduleNotOverlapping() {
        // 1. forEdit 创建 motion：inserted key=1, deleted key=2
        val motion1 =
            ComposeEditMotion.forEdit(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                deletedUnitKeys = listOf(2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 2. sample 到 25% 时间（glyphProgress=0.25）：
        // key=1 在 [0, 0.5] 中点 fraction=0.5，key=2 还没开始 fraction=1
        val midTime = startTime + duration / 4
        val midSample = motion1.sample(midTime)
        assertEquals(0.5f, midSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals(1f, midSample.unitClipFractions[2L]!!, 0.001f)

        // 3. redirectTo：inserted key=1 + deleted key=2，合成一条 traversal list
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = listOf(1L),
                newDeletedUnitKeys = listOf(2L),
                frameTimeNanos = midTime,
                durationNanos = duration,
            )

        // 4. redirect 后两条 unit 在一条 schedule 上：
        // key=1 前半段 [0, 0.5] 继续吐字，key=2 后半段 [0.5, 1] 接着吞字
        // sample 到新 motion 25% 时间：key=1 fraction 在增长（0.5→0.75），key=2 fraction=1（还没开始吞）
        val quarterSample = motion2.sample(midTime + duration / 4)
        val fraction1Quarter = quarterSample.unitClipFractions[1L]!!
        val fraction2Quarter = quarterSample.unitClipFractions[2L]!!
        assertTrue("key=1 应在增长（>0.5），实际=$fraction1Quarter", fraction1Quarter > 0.5f)
        assertEquals("key=2 应还没开始吞（fraction=1），实际=$fraction2Quarter", 1f, fraction2Quarter, 0.001f)

        // 5. sample 到新 motion 75% 时间：key=1 fraction=1（已完成），key=2 fraction 在下降（1→0.5）
        val threeQuarterSample = motion2.sample(midTime + 3 * duration / 4)
        val fraction1ThreeQuarter = threeQuarterSample.unitClipFractions[1L]!!
        val fraction2ThreeQuarter = threeQuarterSample.unitClipFractions[2L]!!
        assertEquals("key=1 应已完成（fraction=1），实际=$fraction1ThreeQuarter", 1f, fraction1ThreeQuarter, 0.001f)
        assertTrue("key=2 应在下降（<1），实际=$fraction2ThreeQuarter", fraction2ThreeQuarter < 1f)

        // 6. 断言：两组不再各自从 0 同时开始（不重叠）—
        // 25% 时 key=2 还没开始（fraction=1），75% 时 key=1 已完成（fraction=1），
        // 两者在不同区间，不是各自归一化到 [0,1] 同时播放。
        assertTrue("不重叠：key=2 在 key=1 之后才动", fraction2Quarter > fraction2ThreeQuarter)
    }

    /**
     * Issue #728 评论 5760741452 问题2：redirectCaretTo 后 traversal 顺序必须保持原 motion 的真实顺序。
     *
     * 场景：forDelete([10, 11, 12])，运行到 key=12 中途，redirectCaretTo()（纯 caret 移动），
     * 后续必须是 key=12 继续 → key11 → key10，不能变成 key12 → key10 → key11。
     *
     * forDelete 反向分配区间：key=12 [0, 1/3]（最右先吞），key=11 [1/3, 2/3]，key=10 [2/3, 1]（最左后吞）。
     * unitChannels 插入顺序是 [10, 11, 12]（正文左→右），但实际 traversal 顺序是 12→11→10。
     * 旧 bug 用 map 迭代顺序建 specs，pending 按 [10, 11] 排，traversal 变成 12→10→11（错误）。
     * 修复后按 startProgress 升序建 specs，pending 按 [11, 10] 排，traversal 保持 12→11→10（正确）。
     */
    @Test
    fun redirectCaretTo_preservesOriginalTraversalOrder_rightToLeft() {
        // 1. forDelete 三个 deleted unit keys=10,11,12（sourceRange.start 升序，正文左→右）
        val motion1 =
            ComposeEditMotion.forDelete(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedUnitKeys = listOf(10L, 11L, 12L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 2. sample 到 duration/6（glyphProgress=1/6，key=12 区间 [0,1/3] 中段）
        //    key=12 fraction=0.5（in-progress），key=10/11 fraction=1（未开始）
        val midTime = startTime + duration / 6
        val midSample = motion1.sample(midTime)
        assertEquals("key=12 应在 [0,1/3] 中段 fraction=0.5", 0.5f, midSample.unitClipFractions[12L]!!, 0.001f)
        assertEquals("key=11 还没开始 fraction=1", 1f, midSample.unitClipFractions[11L]!!, 0.001f)
        assertEquals("key=10 还没开始 fraction=1", 1f, midSample.unitClipFractions[10L]!!, 0.001f)

        // 3. redirectCaretTo（纯 caret 移动，模拟用户按方向键/鼠标移动 selection）
        val motion2 =
            motion1.redirectCaretTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                frameTimeNanos = midTime,
                durationNanos = duration,
            )

        // 4. sample 到新 motion 60% 时间（glyphProgress=0.6）
        //    修复后区间：key=12 [0, 0.5]（in-progress 继续），key=11 [0.5, 0.75]，key=10 [0.75, 1.0]
        //    key=12: 0.6 >= 0.5 → fraction=0（已完成）
        //    key=11: 0.5 < 0.6 < 0.75 → local=0.4 → fraction=0.6（正在吞）
        //    key=10: 0.6 <= 0.75 → fraction=1（还没开始）
        //    key=11 比 key=10 先开始吞 — 右→左顺序保持！
        val atSixtyPercent = motion2.sample(midTime + duration * 3 / 5)
        val fraction10 = atSixtyPercent.unitClipFractions[10L]!!
        val fraction11 = atSixtyPercent.unitClipFractions[11L]!!
        val fraction12 = atSixtyPercent.unitClipFractions[12L]!!

        // key=12 已完成（最前继续）
        assertEquals("key=12 应已完成吞（fraction=0），实际=$fraction12", 0f, fraction12, 0.001f)
        // key=11 先开始吞（fraction<1），key=10 还没开始（fraction==1）— 右→左顺序保持
        assertTrue("key=11 应已开始吞（fraction<1），实际=$fraction11", fraction11 < 1f)
        assertEquals("key=10 应还没开始（fraction=1），实际=$fraction10", 1f, fraction10, 0.001f)
        // 关键断言：key=11 fraction < key=10 fraction — key=11 先于 key=10 开始吞
        assertTrue(
            "traversal 顺序应保持 12→11→10：key=11 fraction=$fraction11 < key=10 fraction=$fraction10" +
                "（key=11 先于 key=10 开始吞）",
            fraction11 < fraction10,
        )

        // 5. sample 到新 motion 75% 时间（glyphProgress=0.75）
        //    key=11: 0.75 >= 0.75 → fraction=0（已完成）
        //    key=10: 0.75 <= 0.75 → fraction=1（刚好开始）
        val atSeventyFive = motion2.sample(midTime + duration * 3 / 4)
        assertEquals(
            "75% 时 key=11 应已完成 fraction=0（区间 [0.5,0.75]），实际=${atSeventyFive.unitClipFractions[11L]}",
            0f,
            atSeventyFive.unitClipFractions[11L]!!,
            0.001f,
        )
        // key=10 在 key=11 之后才开始 — 顺序正确（旧 bug 这里 key=10 已完成、key=11 还没开始）
        assertTrue(
            "75% 时 key=10 应还没完成（fraction>0），key=11 已完成 — key=10 在 key=11 之后，" +
                "实际 key=10=${atSeventyFive.unitClipFractions[10L]}, key=11=${atSeventyFive.unitClipFractions[11L]}",
            atSeventyFive.unitClipFractions[10L]!! > 0f,
        )
    }
}
