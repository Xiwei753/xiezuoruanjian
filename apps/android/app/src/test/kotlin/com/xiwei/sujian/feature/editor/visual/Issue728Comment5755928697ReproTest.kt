package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Issue #728 评论 5755928697 三个确定问题的回归测试。
 *
 * 三个问题（已由 coder 修复）：
 * 1. 文字动画还没结束时移动光标，会把正在运行的 glyph channel 全丢掉。
 *    修复：[ComposeEditMotion.redirectCaretTo] 保留现有 unit channel fraction。
 * 2. coordinated=false 时独立 smooth cursor 设置对正文编辑不生效。
 *    修复：双 duration（caretDurationNanos/glyphDurationNanos 独立）。
 * 3. 一笔里有多个 glyph unit 时所有字同时吐/吞，不是真正跟着光标。
 *    修复：[ComposeEditMotion.UnitChannel] 增加 startProgress/endProgress 区间，sample 按区间映射。
 *
 * 这些测试是 [ComposeEditMotion] 的纯 API 测试，不需要 Robolectric/ComposeRule/反射。
 * 直接构造 ComposeEditMotion、调用方法、断言。和 [ComposeEditMotionTest] 同级别，
 * 但聚焦三个问题的修复语义，命名用 Issue728Comment5755928697 前缀。
 */
@Suppress("MaxLineLength", "LongMethod")
class Issue728Comment5755928697ReproTest {
    private val originRect = Rect(left = 10f, top = 0f, right = 12f, bottom = 20f)
    private val targetRect = Rect(left = 30f, top = 0f, right = 32f, bottom = 20f)
    private val startTime = 1_000_000_000L

    /**
     * 600ms — 能被 6 整除，方便算 1/6、1/2、5/6 区间映射点（问题3 三个 unit 的区间中点）。
     * 600_000_000 / 6 = 100_000_000（精确），/ 2 = 300_000_000（精确），* 5 / 6 = 500_000_000（精确）。
     */
    private val glyphDuration = 600_000_000L

    // ==================== 问题1：redirectCaretTo 保留 glyph channel ====================

    /**
     * 问题1：文字动画还没结束时移动光标，[ComposeEditMotion.redirectCaretTo] 保留现有 unit channel。
     *
     * 场景：forInsert 2 个 unit {1, 2}，sample 到 25%（glyphProgress=0.25），
     * key=1 区间 [0, 0.5] 正在吐（fraction=0.5），key=2 区间 [0.5, 1] 还没开始（fraction=0）。
     * 调 redirectCaretTo 换 caret 目标，验证：
     * - 原有 unit channel 仍然存在（unitClipFractions 包含 key=1 和 key=2）。
     * - fraction 从当前值继续（key=1=0.5, key=2=0），不是被清空、不是重新从 0/1 开始。
     * - 继续跑到 glyph 终点，两个 unit 都到 1（文字继续吐完，没被丢弃）。
     */
    @Test
    fun redirectCaretTo_preservesRunningGlyphChannels() {
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L, 2L),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )
        // 25% 时：glyphProgress=0.25，key=1 区间 [0, 0.5] local=0.5 fraction=0.5，
        // key=2 区间 [0.5, 1] 还没开始 fraction=0
        val midTime = startTime + glyphDuration / 4
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
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )

        // redirect 后立即 sample：unit channel 仍然存在，fraction 保持当前值（不丢字）
        val redirectedSample = motion2.sample(midTime)
        assertNotNull("unit 1 channel 应保留（redirectCaretTo 不丢 channel）", redirectedSample.unitClipFractions[1L])
        assertNotNull("unit 2 channel 应保留（redirectCaretTo 不丢 channel）", redirectedSample.unitClipFractions[2L])
        assertEquals("unit 1 fraction 应从当前值 0.5 继续", 0.5f, redirectedSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals("unit 2 fraction 应从当前值 0 继续", 0f, redirectedSample.unitClipFractions[2L]!!, 0.001f)

        // 继续跑到 glyph 终点：两个 unit 都到 1（文字继续吐完，没被丢弃）
        val endSample = motion2.sample(midTime + glyphDuration)
        assertEquals("unit 1 应吐完到 1", 1f, endSample.unitClipFractions[1L]!!, 0.001f)
        assertEquals("unit 2 应吐完到 1", 1f, endSample.unitClipFractions[2L]!!, 0.001f)
    }

    /**
     * 问题1 对比测试：redirectTo 用空 keys 会丢 channel（旧 bug 行为），
     * redirectCaretTo 保留 channel（修复后行为）。验证两者语义不同。
     *
     * 场景：forInsert 1 个 unit {1}，sample 到 50%（fraction=0.5）。
     * - redirectTo(emptySet, emptySet)：新 motion 的 unitChannels 为空（旧 bug 行为，丢 channel）。
     * - redirectCaretTo：新 motion 的 unitChannels 保留 key=1（修复后行为）。
     *
     * 旧实现遇到 pending selection 时调 redirectTo(empty, empty) 把正在吐的字全丢掉；
     * 修复后改用 redirectCaretTo 保留 channel，文字继续吐完。
     */
    @Test
    fun redirectTo_withEmptyKeys_dropsChannels_contrastWithRedirectCaretTo() {
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )
        val midTime = startTime + glyphDuration / 2
        val midSample = motion1.sample(midTime)
        assertEquals(0.5f, midSample.unitClipFractions[1L]!!, 0.001f)

        val newCaretTarget = Rect(left = 5f, top = 0f, right = 7f, bottom = 20f)

        // 旧 bug 行为：redirectTo 用空 keys → 丢 channel
        val redirectedDropMotion =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = newCaretTarget,
                newInsertedUnitKeys = emptyList(),
                newDeletedUnitKeys = emptyList(),
                frameTimeNanos = midTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )
        val droppedSample = redirectedDropMotion.sample(midTime)
        assertTrue(
            "redirectTo 用空 keys 应丢 channel（旧 bug 行为）— unitClipFractions 应不含 key=1",
            !droppedSample.unitClipFractions.containsKey(1L),
        )

        // 修复后行为：redirectCaretTo → 保留 channel
        val preservedMotion =
            motion1.redirectCaretTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = newCaretTarget,
                frameTimeNanos = midTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )
        val preservedSample = preservedMotion.sample(midTime)
        assertNotNull(
            "redirectCaretTo 应保留 channel（修复后行为）— unitClipFractions 应含 key=1",
            preservedSample.unitClipFractions[1L],
        )
        assertEquals(
            "redirectCaretTo 保留的 channel fraction 应从当前值 0.5 继续",
            0.5f,
            preservedSample.unitClipFractions[1L]!!,
            0.001f,
        )
    }

    // ==================== 问题2：coordinated=false 独立 duration ====================

    /**
     * 问题2：coordinated=false 时 caret 和 glyph 各自持有 duration，独立计时。
     *
     * coordinated=false 在 ComposeEditMotion 层面体现为 caretDurationNanos != glyphDurationNanos。
     *
     * 场景1：caretDuration=200ms, glyphDuration=100ms。sample 到 100ms 时
     * glyph finished（100ms >= 100ms）但 caret 未 finished（100ms < 200ms）。
     * 验证 caret 不跟 glyphDuration（textDuration）走，caret 用自己的 cursorDuration。
     *
     * 场景2：caretDuration=100ms, glyphDuration=200ms。sample 到 100ms 时
     * caret finished 但 glyph 未 finished。验证 glyph 不跟 caretDuration 走。
     */
    @Test
    fun coordinatedFalse_caretUsesCursorDuration_glyphUsesTextDuration() {
        val caretDurationLong = 200_000_000L // 200ms
        val glyphDurationShort = 100_000_000L // 100ms

        // 场景1：caret 200ms, glyph 100ms — glyph 先 finished，caret 还在跑
        val motionCaretLonger =
            ComposeEditMotion.forEdit(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                deletedUnitKeys = emptyList(),
                frameTimeNanos = startTime,
                caretDurationNanos = caretDurationLong,
                glyphDurationNanos = glyphDurationShort,
            )
        // sample 到 100ms：glyph finished，caret 未 finished
        val sampleGlyphDone = motionCaretLonger.sample(startTime + glyphDurationShort)
        assertEquals(
            "glyph 应 finished（100ms >= glyphDuration 100ms）— fraction=1",
            1f,
            sampleGlyphDone.unitClipFractions[1L]!!,
            0.001f,
        )
        assertFalse(
            "caret 不应 finished（100ms < caretDuration 200ms）— 整体 finished=false",
            sampleGlyphDone.finished,
        )
        // caret 还在中间（100ms/200ms=0.5），left=10+(30-10)*0.5=20，没到 target
        assertEquals("caret 在中间（用 caretDuration 200ms 算 progress=0.5）", 20f, sampleGlyphDone.caretRect.left, 0.001f)

        // 场景2：caret 100ms, glyph 200ms — caret 先 finished，glyph 还在跑
        val motionGlyphLonger =
            ComposeEditMotion.forEdit(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                deletedUnitKeys = emptyList(),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDurationShort,
                glyphDurationNanos = caretDurationLong,
            )
        // sample 到 100ms：caret finished，glyph 未 finished
        val sampleCaretDone = motionGlyphLonger.sample(startTime + glyphDurationShort)
        assertEquals(
            "caret 应到 target（100ms >= caretDuration 100ms）",
            targetRect,
            sampleCaretDone.caretRect,
        )
        assertFalse(
            "glyph 不应 finished（100ms < glyphDuration 200ms）— 整体 finished=false",
            sampleCaretDone.finished,
        )
        // glyph 还在中间（100ms/200ms=0.5），fraction=0.5
        assertEquals(
            "glyph 在中间（用 glyphDuration 200ms 算 progress=0.5）— fraction=0.5",
            0.5f,
            sampleCaretDone.unitClipFractions[1L]!!,
            0.001f,
        )
    }

    /**
     * 问题2 对比：coordinated=true 时 caret 和 glyph 共用同一 duration，sample 到中间时两者都未 finished。
     *
     * coordinated=true 在 ComposeEditMotion 层面体现为 caretDurationNanos == glyphDurationNanos。
     * sample 到中间时 caret 和 glyph 都 progress=0.5，都未 finished；到终点时都 finished。
     */
    @Test
    fun coordinatedTrue_caretAndGlyphShareTextDuration() {
        val sharedDuration = 200_000_000L // 200ms
        val motion =
            ComposeEditMotion.forEdit(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                deletedUnitKeys = emptyList(),
                frameTimeNanos = startTime,
                caretDurationNanos = sharedDuration,
                glyphDurationNanos = sharedDuration,
            )
        // sample 到 100ms（中间）：caret 和 glyph 都 progress=0.5，都未 finished
        val midSample = motion.sample(startTime + sharedDuration / 2)
        assertEquals("caret 在中间（progress=0.5）", 20f, midSample.caretRect.left, 0.001f)
        assertEquals("glyph 在中间（fraction=0.5）", 0.5f, midSample.unitClipFractions[1L]!!, 0.001f)
        assertFalse("coordinated=true 中间时两者都未 finished", midSample.finished)

        // sample 到 200ms（终点）：两者都 finished
        val endSample = motion.sample(startTime + sharedDuration)
        assertEquals(targetRect, endSample.caretRect)
        assertEquals(1f, endSample.unitClipFractions[1L]!!, 0.001f)
        assertTrue("coordinated=true 终点时两者都 finished", endSample.finished)
    }

    // ==================== 问题3：多 unit 区间映射跟着光标 ====================

    /**
     * 问题3：多个 inserted unit 依次吐字，跟着光标。
     *
     * forInsert 3 个 unit {1, 2, 3}，sorted=[1, 2, 3]，区间（纯 inserted 第 i 个 [i/n, (i+1)/n]）：
     * - key=1: [0, 1/3]
     * - key=2: [1/3, 2/3]
     * - key=3: [2/3, 1]
     *
     * sample 到 glyphProgress=1/6（第一个区间中点）：
     * - unit1 fraction≈0.5（正在吐）
     * - unit2=0（还没开始）
     * - unit3=0（还没开始）
     *
     * sample 到 glyphProgress=1/2（第二个区间中点）：
     * - unit1=1（已吐完）
     * - unit2≈0.5（正在吐）
     * - unit3=0
     *
     * sample 到 glyphProgress=5/6（第三个区间中点）：
     * - unit1=1，unit2=1，unit3≈0.5
     *
     * 验证光标从左到右依次吐字，不是所有字同时吐。
     */
    @Test
    fun multipleInsertedUnits_revealSequentially_followingCaret() {
        val motion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L, 2L, 3L),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )

        // glyphProgress=1/6：unit1 区间 [0, 1/3] 中点 local=0.5 fraction=0.5；
        // unit2/unit3 还没开始 fraction=0
        val t1 = startTime + glyphDuration / 6 // 100ms → 100/600=1/6
        val s1 = motion.sample(t1)
        assertEquals("glyphProgress=1/6: unit1 正在吐 fraction≈0.5", 0.5f, s1.unitClipFractions[1L]!!, 0.001f)
        assertEquals("glyphProgress=1/6: unit2 还没开始 fraction=0", 0f, s1.unitClipFractions[2L]!!, 0.001f)
        assertEquals("glyphProgress=1/6: unit3 还没开始 fraction=0", 0f, s1.unitClipFractions[3L]!!, 0.001f)

        // glyphProgress=1/2：unit1 已吐完 fraction=1；unit2 区间 [1/3, 2/3] 中点 local=0.5 fraction=0.5；
        // unit3 还没开始 fraction=0
        val t2 = startTime + glyphDuration / 2 // 300ms → 300/600=1/2
        val s2 = motion.sample(t2)
        assertEquals("glyphProgress=1/2: unit1 已吐完 fraction=1", 1f, s2.unitClipFractions[1L]!!, 0.001f)
        assertEquals("glyphProgress=1/2: unit2 正在吐 fraction≈0.5", 0.5f, s2.unitClipFractions[2L]!!, 0.001f)
        assertEquals("glyphProgress=1/2: unit3 还没开始 fraction=0", 0f, s2.unitClipFractions[3L]!!, 0.001f)

        // glyphProgress=5/6：unit1/unit2 已吐完 fraction=1；unit3 区间 [2/3, 1] 中点 local=0.5 fraction=0.5
        val t3 = startTime + 5 * glyphDuration / 6 // 500ms → 500/600=5/6
        val s3 = motion.sample(t3)
        assertEquals("glyphProgress=5/6: unit1 已吐完 fraction=1", 1f, s3.unitClipFractions[1L]!!, 0.001f)
        assertEquals("glyphProgress=5/6: unit2 已吐完 fraction=1", 1f, s3.unitClipFractions[2L]!!, 0.001f)
        assertEquals("glyphProgress=5/6: unit3 正在吐 fraction≈0.5", 0.5f, s3.unitClipFractions[3L]!!, 0.001f)
    }

    /**
     * 问题3：多个 deleted unit 反向吞字，跟着光标从右往左。
     *
     * forDelete 3 个 unit {1, 2, 3}，sorted=[1, 2, 3]，反向区间（纯 deleted 第 i 个 [(n-1-i)/n, (n-i)/n]）：
     * - key=1 (i=0): [2/3, 1]（最后吞）
     * - key=2 (i=1): [1/3, 2/3]
     * - key=3 (i=2): [0, 1/3]（最先吞，最右边的 unit）
     *
     * sample 到 glyphProgress=1/6：
     * - unit3（sorted 最后一个，最右边）正在吞 fraction≈0.5
     * - unit1/unit2 fraction=1（完全可见，还没开始吞）
     *
     * sample 到 glyphProgress=1/2：
     * - unit3 已吞完 fraction=0；unit2 正在吞 fraction≈0.5；unit1 fraction=1
     *
     * sample 到 glyphProgress=5/6：
     * - unit3/unit2 已吞完 fraction=0；unit1 正在吞 fraction≈0.5
     *
     * 验证删除按 caret 实际经过顺序反向映射（光标从右往左，先吞最右边）。
     */
    @Test
    fun multipleDeletedUnits_concealReversed_followingCaret() {
        val motion =
            ComposeEditMotion.forDelete(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedUnitKeys = listOf(1L, 2L, 3L),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )

        // glyphProgress=1/6：unit3 区间 [0, 1/3] 中点 local=0.5 fraction=1+(0-1)*0.5=0.5（正在吞）；
        // unit1 区间 [2/3, 1] 还没开始 fraction=1；unit2 区间 [1/3, 2/3] 还没开始 fraction=1
        val t1 = startTime + glyphDuration / 6
        val s1 = motion.sample(t1)
        assertEquals("glyphProgress=1/6: unit1 完全可见 fraction=1（还没吞）", 1f, s1.unitClipFractions[1L]!!, 0.001f)
        assertEquals("glyphProgress=1/6: unit2 完全可见 fraction=1（还没吞）", 1f, s1.unitClipFractions[2L]!!, 0.001f)
        assertEquals("glyphProgress=1/6: unit3 正在吞 fraction≈0.5（最右边先吞）", 0.5f, s1.unitClipFractions[3L]!!, 0.001f)

        // glyphProgress=1/2：unit3 已吞完 fraction=0；unit2 区间 [1/3, 2/3] 中点 fraction=0.5；
        // unit1 还没开始 fraction=1
        val t2 = startTime + glyphDuration / 2
        val s2 = motion.sample(t2)
        assertEquals("glyphProgress=1/2: unit1 完全可见 fraction=1（还没吞）", 1f, s2.unitClipFractions[1L]!!, 0.001f)
        assertEquals("glyphProgress=1/2: unit2 正在吞 fraction≈0.5", 0.5f, s2.unitClipFractions[2L]!!, 0.001f)
        assertEquals("glyphProgress=1/2: unit3 已吞完 fraction=0", 0f, s2.unitClipFractions[3L]!!, 0.001f)

        // glyphProgress=5/6：unit3/unit2 已吞完 fraction=0；unit1 区间 [2/3, 1] 中点 fraction=0.5
        val t3 = startTime + 5 * glyphDuration / 6
        val s3 = motion.sample(t3)
        assertEquals("glyphProgress=5/6: unit1 正在吞 fraction≈0.5", 0.5f, s3.unitClipFractions[1L]!!, 0.001f)
        assertEquals("glyphProgress=5/6: unit2 已吞完 fraction=0", 0f, s3.unitClipFractions[2L]!!, 0.001f)
        assertEquals("glyphProgress=5/6: unit3 已吞完 fraction=0", 0f, s3.unitClipFractions[3L]!!, 0.001f)
    }

    /**
     * 问题3：混合 edit（1 inserted + 1 deleted）— inserted 占前半段，deleted 占后半段。
     *
     * forEdit inserted={1}, deleted={2}。区间（混合：inserted 前半段 [0, 0.5]，deleted 后半段 [0.5, 1]）：
     * - inserted key=1: [0, 0.5]
     * - deleted key=2: [0.5, 1]
     *
     * sample 到 glyphProgress=0.25：
     * - inserted key=1 区间 [0, 0.5] 中点 local=0.5 fraction=0.5（正在吐）
     * - deleted key=2 还没开始 fraction=1（还没吞）
     *
     * sample 到 glyphProgress=0.75：
     * - inserted key=1 已吐完 fraction=1
     * - deleted key=2 区间 [0.5, 1] 中点 local=0.5 fraction=1+(0-1)*0.5=0.5（正在吞）
     *
     * 验证光标先经过插入区吐字，再经过删除区吞字。
     */
    @Test
    fun mixedEdit_insertedFirstHalf_deletedSecondHalf() {
        val motion =
            ComposeEditMotion.forEdit(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                deletedUnitKeys = listOf(2L),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )

        // glyphProgress=0.25：inserted 正在吐，deleted 还没吞
        val t1 = startTime + glyphDuration / 4 // 150ms → 150/600=0.25
        val s1 = motion.sample(t1)
        assertEquals("glyphProgress=0.25: inserted 正在吐 fraction≈0.5", 0.5f, s1.unitClipFractions[1L]!!, 0.001f)
        assertEquals("glyphProgress=0.25: deleted 还没吞 fraction=1", 1f, s1.unitClipFractions[2L]!!, 0.001f)

        // glyphProgress=0.75：inserted 已吐完，deleted 正在吞
        val t2 = startTime + 3 * glyphDuration / 4 // 450ms → 450/600=0.75
        val s2 = motion.sample(t2)
        assertEquals("glyphProgress=0.75: inserted 已吐完 fraction=1", 1f, s2.unitClipFractions[1L]!!, 0.001f)
        assertEquals("glyphProgress=0.75: deleted 正在吞 fraction≈0.5", 0.5f, s2.unitClipFractions[2L]!!, 0.001f)
    }

    // ==================== 问题1：redirect 保留 master progress 相位 ====================

    /**
     * 问题1：redirect 后正在进行的 unit 应该立即继续，不应该冻结。
     *
     * 场景：3 个 unit {1, 2, 3}，区间 [0, 1/3], [1/3, 2/3], [2/3, 1]
     * 当前正在第二个 unit，fraction=0.5（中途）
     * 此时 redirectCaretTo 到新目标
     *
     * 修复前：新 motion 的 master progress 从 0 开始，第二个 unit 的 startProgress=1/3
     * 所以前 1/3 的新 duration 里这个 unit 的 localProgress=0，fraction 卡在 0.5 不动
     *
     * 修复后：第二个 unit 的 startProgress 归一化到 0，from=0.5，fraction 立即继续
     */
    @Test
    fun redirect_preservesPhase_activeUnitContinuesImmediately() {
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L, 2L, 3L),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )
        // 3 个 unit：key=1 [0, 1/3], key=2 [1/3, 2/3], key=3 [2/3, 1]
        // sample 到 glyphProgress=0.4（第二个 unit 中途）
        val midTime = startTime + (glyphDuration * 0.4f).toLong()
        val midSample = motion1.sample(midTime)
        // key=2 区间 [1/3, 2/3]，localProgress = (0.4 - 1/3) / (1/3) ≈ 0.2，fraction≈0.2
        val key2Fraction = midSample.unitClipFractions[2L]!!
        assertTrue("key=2 应在进行中 fraction>0 && <1", key2Fraction > 0f && key2Fraction < 1f)

        // redirect 到新目标
        val newCaretTarget = Rect(left = 0f, top = 0f, right = 2f, bottom = 20f)
        val motion2 =
            motion1.redirectCaretTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = newCaretTarget,
                frameTimeNanos = midTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )

        // redirect 后立即 sample：key=2 的 fraction 应该从当前值继续
        val redirectedSample = motion2.sample(midTime)
        assertEquals("key=2 fraction 应从当前值继续", key2Fraction, redirectedSample.unitClipFractions[2L]!!, 0.001f)

        // 关键验证：redirect 后过一小段时间，key=2 的 fraction 应该在增长（不是冻结）
        val shortDelay = midTime + glyphDuration / 10 // 10% 的新 duration
        val afterShortDelay = motion2.sample(shortDelay)
        val newKey2Fraction = afterShortDelay.unitClipFractions[2L]!!
        assertTrue("key=2 fraction 应继续增长（不是冻结）: $newKey2Fraction > $key2Fraction", newKey2Fraction > key2Fraction)
    }

    /**
     * 问题1：redirect 后进行中的 unit 立即继续，不应该冻结。
     *
     * 场景：3 个 unit {1, 2, 3}，区间 [0, 1/3], [1/3, 2/3], [2/3, 1]
     * 当前正在第一个 unit 中途 fraction=0.45
     * 此时 redirectTo 到新目标（保留旧 unit 1,2,3）
     *
     * 验证：
     * - key=1（进行中）：从 0.45 继续，不是从 0 开始
     */
    @Test
    fun redirectTo_activeUnitContinuesFromCurrentFraction() {
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L, 2L, 3L),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )
        // sample 到 glyphProgress=0.15（第一个 unit 中途）
        val midTime = startTime + (glyphDuration * 0.15f).toLong()
        val midSample = motion1.sample(midTime)
        // key=1 区间 [0, 1/3]，localProgress = 0.15 / (1/3) = 0.45，fraction=0.45
        assertEquals(0.45f, midSample.unitClipFractions[1L]!!, 0.001f)

        // redirectTo 到新目标，保留旧 unit 1,2,3
        val newCaretTarget = Rect(left = 0f, top = 0f, right = 2f, bottom = 20f)
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = newCaretTarget,
                newInsertedUnitKeys = listOf(1L, 2L, 3L),
                newDeletedUnitKeys = emptyList(),
                frameTimeNanos = midTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )

        // redirect 后立即 sample：key=1 从 0.45 继续
        val redirectedSample = motion2.sample(midTime)
        assertEquals("key=1 应从 0.45 继续", 0.45f, redirectedSample.unitClipFractions[1L]!!, 0.001f)

        // 关键验证：redirect 后过一小段时间，key=1 的 fraction 应该在增长（不是冻结）
        val shortDelay = midTime + glyphDuration / 10 // 10% 的新 duration
        val afterShortDelay = motion2.sample(shortDelay)
        val newKey1Fraction = afterShortDelay.unitClipFractions[1L]!!
        assertTrue("key=1 fraction 应继续增长（不是冻结）: $newKey1Fraction > 0.45", newKey1Fraction > 0.45f)
    }

    // ==================== 问题2：unit 顺序来自正文/几何 ====================

    /**
     * 问题2：unit 顺序应该来自正文位置，不是 key 编号。
     *
     * 场景：key=10 和 key=11 两个 unit
     * 如果按 key sorted()，key=10 会排在 key=11 前面
     * 但如果我们按正文位置逆序传入，应该得到不同的区间分配
     *
     * 修复：allocateEditRanges 接受有序列表，不再内部 sorted()
     * 调用方（ComposeEditorVisualState）按正文 range 排序后再传入
     *
     * 验证：通过 forInsert 创建 motion，观察不同 key 顺序导致的区间分配差异
     */
    @Test
    fun forInsert_respectsKeyOrder_notKeySorted() {
        // 场景：两个 unit，key 11 和 10（故意逆序）
        // 如果 allocateEditRanges 内部 sorted()，会按 key 10->11 分配区间
        // 如果 allocateEditRanges 按传入顺序，会按 11->10 分配区间
        // 两种顺序下 key=11 的 fraction 增长速度不同（区间位置不同）

        // 顺序 1：key=10 在前，key=11 在后
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L, 11L),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )
        // 顺序 2：key=11 在前，key=10 在后（逆序）
        val motion2 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(11L, 10L),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )

        // 在 25% 进度时：
        // motion1: key=10 区间 [0, 0.5]，key=11 区间 [0.5, 1]
        // motion2: key=11 区间 [0, 0.5]，key=10 区间 [0.5, 1]
        val t = startTime + glyphDuration / 4
        val s1 = motion1.sample(t)
        val s2 = motion2.sample(t)

        // motion1 中 key=10 正在吐（区间 [0, 0.5] 中点 fraction≈0.5）
        assertTrue("motion1 key=10 应正在吐", s1.unitClipFractions[10L]!! > 0.4f)
        // motion1 中 key=11 还没开始（区间 [0.5, 1]）
        assertEquals("motion1 key=11 应还没开始", 0f, s1.unitClipFractions[11L]!!, 0.001f)

        // motion2 中 key=11 正在吐（区间 [0, 0.5] 中点 fraction≈0.5）
        assertTrue("motion2 key=11 应正在吐", s2.unitClipFractions[11L]!! > 0.4f)
        // motion2 中 key=10 还没开始（区间 [0.5, 1]）
        assertEquals("motion2 key=10 应还没开始", 0f, s2.unitClipFractions[10L]!!, 0.001f)
    }

    /**
     * 问题1 加固：redirect 必须把相位按"旧 motion 的 duration"算，而不是新 duration。
     *
     * 连续编辑时如果用户改了 motion 设置（coordinated / duration 变化），redirect 的新 motion
     * 时长可能不同于旧 motion。如果 oldGlyphProgress 误用新 duration 算，相位分类会失真，
     * 正在吐的字会被错判成"未开始"而从 0 重新播（或冻结一段）。
     *
     * 场景：3 个 unit [0, 1/3], [1/3, 2/3], [2/3, 1]，旧 glyphDuration=100ms，
     * 在 40ms 处（glyphProgress=0.4，key=2 进行中 fraction≈0.2）redirect 到新 duration=200ms。
     * 修复后 oldGlyphProgress 仍用旧 100ms 算 = 0.4，key=2 从当前 fraction 继续。
     */
    @Test
    fun redirect_preservesPhase_whenGlyphDurationChanges() {
        val oldGlyphDuration = 100_000_000L
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L, 2L, 3L),
                frameTimeNanos = startTime,
                caretDurationNanos = oldGlyphDuration,
                glyphDurationNanos = oldGlyphDuration,
            )
        // glyphProgress=0.4：key=2 区间 [1/3, 2/3]，local=(0.4-1/3)/(1/3)≈0.2，fraction≈0.2
        val midTime = startTime + 40_000_000L
        val midSample = motion1.sample(midTime)
        val key2Fraction = midSample.unitClipFractions[2L]!!
        assertTrue("key=2 应在进行中", key2Fraction > 0f && key2Fraction < 1f)

        // redirect 到新目标，但用不同的 glyph 时长（200ms）
        val newCaretTarget = Rect(left = 0f, top = 0f, right = 2f, bottom = 20f)
        val motion2 =
            motion1.redirectCaretTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = newCaretTarget,
                frameTimeNanos = midTime,
                caretDurationNanos = 200_000_000L,
                glyphDurationNanos = 200_000_000L,
            )

        // redirect 后立即 sample：key=2 的 fraction 应该从当前值继续（不是从 0 重播）
        val redirectedSample = motion2.sample(midTime)
        assertEquals(
            "key=2 fraction 应从当前值继续（相位用旧 duration 算）",
            key2Fraction,
            redirectedSample.unitClipFractions[2L]!!,
            0.001f,
        )

        // 继续跑一小段时间：key=2 的 fraction 应该增长（不是冻结）
        val afterShortDelay = motion2.sample(midTime + 20_000_000L)
        val newKey2Fraction = afterShortDelay.unitClipFractions[2L]!!
        assertTrue("key=2 fraction 应继续增长（不是冻结）: $newKey2Fraction > $key2Fraction", newKey2Fraction > key2Fraction)
    }

    /**
     * 问题2 加固：rapid redirect 时旧未完成 unit 和新 unit 按传入（正文/几何）顺序交错排，
     * 不按创建时间排。
     *
     * 场景（评论里的例子）：右边旧 inserted unit key=10 还在吐（进行中 fraction=0.5），
     * 用户把光标移到左边再输入新字（key=11，正文里在 key=10 左边）。
     * 调用方传 newInsertedUnitKeys = [11, 10]（正文顺序：左 11 在前，右 10 在后）。
     *
     * 修复前：旧 unit 按创建时间排在前、新 unit 追加在后 → key=10 先吐，key=11 冻结等。
     * 修复后：按传入顺序交错，新 unit（光标处）先吐，旧 unit 从当前 fraction 继续。
     *
     * 验证：redirect 后一小段进度，key=11（光标处新字）在吐（fraction>0），
     * 而 key=10（右边旧字）还停在当前 fraction（它的区间在后面，还没轮到）。
     */
    @Test
    fun redirectTo_interleavesOldAndNewByTextOrder_notByCreationTime() {
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )
        // 旧 unit key=10 进行中：glyphProgress=0.5，fraction=0.5
        val midTime = startTime + glyphDuration / 2
        val midSample = motion1.sample(midTime)
        assertEquals(0.5f, midSample.unitClipFractions[10L]!!, 0.001f)

        // redirect：按正文顺序传 [11(新,左), 10(旧,右)]
        val newCaretTarget = Rect(left = 0f, top = 0f, right = 2f, bottom = 20f)
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = newCaretTarget,
                newInsertedUnitKeys = listOf(11L, 10L),
                newDeletedUnitKeys = emptyList(),
                frameTimeNanos = midTime,
                caretDurationNanos = glyphDuration,
                glyphDurationNanos = glyphDuration,
            )

        // 立即 sample：key=10 从当前 0.5 继续，key=11 从 0 开始
        val redirectedSample = motion2.sample(midTime)
        assertEquals("key=10 应从 0.5 继续", 0.5f, redirectedSample.unitClipFractions[10L]!!, 0.001f)
        assertEquals("key=11 应从 0 开始", 0f, redirectedSample.unitClipFractions[11L]!!, 0.001f)

        // 关键验证：redirect 后一小段进度，key=11（光标处新字）先吐，key=10 还停在当前 fraction
        val shortDelay = midTime + glyphDuration / 8
        val afterShort = motion2.sample(shortDelay)
        val new11 = afterShort.unitClipFractions[11L]!!
        val new10 = afterShort.unitClipFractions[10L]!!
        assertTrue("key=11（光标处新字）应先吐：fraction>0", new11 > 0f)
        // key=10 的区间是 [0.5, 1]，新 motion 进度还小，local 还没到，应仍≈0.5
        assertEquals("key=10（右边旧字）还应停在当前 fraction，没抢先吐", 0.5f, new10, 0.001f)
    }
}
