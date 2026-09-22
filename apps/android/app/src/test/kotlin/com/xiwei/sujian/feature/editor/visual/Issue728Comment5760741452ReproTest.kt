package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Issue #728 评论 5760741452 回归测试 — 两个确定运行错误的修复验证。
 *
 * 原本是 reproduce-engineer 写的复现测试（断言当前错误行为），debug-engineer 修复后
 * 翻转为断言**正确行为**，作为正式回归测试保留。
 *
 * 两个问题：
 * 1. `buildRedirectChannels()` 角色反转后不再用旧方向 remaining 决定新区间长度，
 *    glyph 与 caret 在整个新 motion 内同步到达终点 —
 *    新 motion 60% 时 glyph fraction≈0.16（不是 0），100% 时才到 0。
 * 2. `redirectCaretTo()` 按旧 channel 的 startProgress 升序重建 specs，
 *    多字删除 selection 移动后剩余吞字顺序保持右→左 12→11→10，不会变成 12→10→11。
 *
 * 纯 API 测试，不需要 Robolectric/ComposeRule/反射。
 */
@Suppress("MaxLineLength", "LongMethod")
class Issue728Comment5760741452ReproTest {
    private val originRect = Rect(left = 10f, top = 0f, right = 12f, bottom = 20f)
    private val targetRect = Rect(left = 30f, top = 0f, right = 32f, bottom = 20f)
    private val startTime = 1_000_000_000L
    private val duration = 100_000_000L // 100ms in nanos

    // ==================== 问题1：角色反转后 glyph 与 caret 同步到达终点 ====================

    /**
     * 问题1 回归：Inserted→Deleted 角色反转后，glyph 与 caret 在整个新 motion 内同步到达终点。
     *
     * 场景推导（修复后正确行为）：
     * 1. forInsert 创建 unit key=1（from=0, to=1, startProgress=0, endProgress=1, duration=100ms）
     * 2. sample 到 40% 时间：glyphProgress=0.4，key=1 fraction=0.4
     * 3. redirectTo：key=1 变 deleted（desiredTo=0）
     *    - 角色反转（directionChanged=true），不用旧方向 remaining=0.6
     *    - 新 channel = UnitChannel(from=0.4, to=0, startProgress=0, endProgress=1)（全区间）
     * 4. sample 到新 motion 60% 时间：glyphProgress=0.6
     *    - 0.6 < endProgress=1 → localProgress=0.6 → fraction=0.4+(0-0.4)*0.6=0.16
     *    - glyph 还没消失，与 caret 同步
     * 5. caret 在 60% 时也在走（origin=18, target=10, 60% 插值=13.2，没到目标）
     * 6. 100% 时 glyph fraction=0 且 caret.left=10 — 同时到达终点
     */
    @Test
    fun regression_issue1_roleReversal_glyphSyncsWithCaret() {
        // 1. forInsert 创建 unit key=1（0→1，duration=100ms）
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 2. sample 到 40% 时间：unit 1 fraction=0.4
        val midTime = startTime + duration * 2 / 5 // 40ms
        val midSample = motion1.sample(midTime)
        assertEquals("40% 时 fraction 应为 0.4", 0.4f, midSample.unitClipFractions[1L]!!, 0.001f)

        // 3. redirectTo：key=1 变 deleted（newDeletedUnitKeys=listOf(1L)）
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = emptyList(),
                newDeletedUnitKeys = listOf(1L),
                frameTimeNanos = midTime,
                durationNanos = duration,
            )

        // 4. 新 motion 60% 时间：修复后 glyph fraction≈0.16（与 caret 同步，不是提前吞完到 0）
        val atSixtyPercent = motion2.sample(midTime + duration * 3 / 5) // midTime + 60ms
        val fractionAtSixty = atSixtyPercent.unitClipFractions[1L]!!
        assertEquals(
            "修复后：新 motion 60% 时 glyph fraction 应≈0.16（0.4+(0-0.4)*0.6），与 caret 同步，实际=$fractionAtSixty",
            0.16f,
            fractionAtSixty,
            0.001f,
        )

        // 5. caret 在 60% 时还没到目标（还在 13.2）— glyph 和 caret 都在 60% 进度，同步
        // 新 motion origin = currentSample.caretRect（motion1 40% 时 caret.left=10+(30-10)*0.4=18）
        // 新 motion target = originRect（left=10）
        // 60% 时 caret.left = 18 + (10 - 18) * 0.6 = 13.2
        val caretAtSixty = atSixtyPercent.caretRect
        assertEquals(
            "caret 在 60% 时应在 13.2（origin=18, target=10, 60% 插值），还没到目标（left=10）",
            13.2f,
            caretAtSixty.left,
            0.001f,
        )
        // glyph 还没到终点（fraction=0.16 > 0），caret 还没到终点（left=13.2 > 10）— 同步
        assertTrue(
            "glyph 60% 时还没到终点（fraction=$fractionAtSixty > 0），与 caret 同步",
            fractionAtSixty > 0.001f,
        )
        assertTrue(
            "caret 60% 时还没到目标（left=${caretAtSixty.left} > 10），与 glyph 同步",
            caretAtSixty.left > 10.001f,
        )

        // 6. 新 motion 100% 时 glyph 和 caret 同时到达终点
        val atFull = motion2.sample(midTime + duration)
        assertEquals(
            "新 motion 100% 时 glyph fraction 应为 0（终点）",
            0.0f,
            atFull.unitClipFractions[1L]!!,
            0.001f,
        )
        assertEquals(
            "新 motion 100% 时 caret 应到目标（left=10），与 glyph 同时到达终点",
            10.0f,
            atFull.caretRect.left,
            0.001f,
        )
    }

    /**
     * 问题1 补充：验证修复后正确行为成立 — 60% 时 fraction ≈ 0.16，不是 0。
     */
    @Test
    fun regression_issue1_correctBehaviorHeld() {
        val motion1 =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val midTime = startTime + duration * 2 / 5
        val motion2 =
            motion1.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = emptyList(),
                newDeletedUnitKeys = listOf(1L),
                frameTimeNanos = midTime,
                durationNanos = duration,
            )
        val atSixtyPercent = motion2.sample(midTime + duration * 3 / 5)
        val fractionAtSixty = atSixtyPercent.unitClipFractions[1L]!!
        // 修复后正确行为：60% 时 fraction ≈ 0.16（0.4 + (0 - 0.4) * 0.6 = 0.16）
        assertEquals(
            "修复后：60% 时 fraction 应≈0.16（glyph 与 caret 同步），实际=$fractionAtSixty",
            0.16f,
            fractionAtSixty,
            0.01f,
        )
    }

    // ==================== 问题2：redirectCaretTo traversal 顺序保持 12→11→10 ====================

    /**
     * 问题2 回归：`redirectCaretTo()` 按旧 channel 的 startProgress 升序重建 specs，
     * 多字删除 selection 移动后剩余吞字顺序保持右→左 12→11→10。
     *
     * 场景推导（修复后正确行为）：
     * 1. forDelete(deletedUnitKeys=[10,11,12])：区间反向分配
     *    - key=12: [0, 1/3]（最右，先开始吞）
     *    - key=11: [1/3, 2/3]
     *    - key=10: [2/3, 1]（最左，最后吞）
     * 2. sample 到 duration/6（glyphProgress=1/6，key=12 区间 [0,1/3] 中段）：
     *    - key=12 fraction=0.5（in-progress），key=10/11 fraction=1（未开始）
     * 3. redirectCaretTo（纯 caret 移动）：
     *    - 修复后 specs = unitChannels.entries.sortedBy { startProgress } → [12, 11, 10]
     *    - buildRedirectChannels:
     *      - key=12: InProgress(remaining=0.5) → inProgressUnits
     *      - key=11: NotStarted → pending
     *      - key=10: NotStarted → pending
     *    - 新 schedule: key=12 [0, 0.5], key=11 [0.5, 0.75], key=10 [0.75, 1.0]
     *    - traversal: 12 → 11 → 10（正确！）
     * 4. sample 到新 motion 60% 时间（glyphProgress=0.6）：
     *    - key=12: 0.6 >= 0.5 → fraction=0（已完成）
     *    - key=11: 0.5 < 0.6 < 0.75 → local=0.4 → fraction=0.6（正在吞）
     *    - key=10: 0.6 <= 0.75 → fraction=1（还没开始）
     *    - key=11 比 key=10 先开始吞 — 右→左顺序保持！
     */
    @Test
    fun regression_issue2_redirectCaretTo_preservesTraversalOrder() {
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
        val atSixtyPercent = motion2.sample(midTime + duration * 3 / 5)
        val fraction10 = atSixtyPercent.unitClipFractions[10L]!!
        val fraction11 = atSixtyPercent.unitClipFractions[11L]!!
        val fraction12 = atSixtyPercent.unitClipFractions[12L]!!

        // 修复后正确行为：key=11 先开始吞（fraction<1），key=10 还没开始（fraction==1）
        assertTrue(
            "修复后：key=11 应已开始吞（fraction<1），实际=$fraction11",
            fraction11 < 1f,
        )
        assertEquals(
            "修复后：key=10 应还没开始（fraction=1），实际=$fraction10",
            1f,
            fraction10,
            0.001f,
        )
        // key=11 fraction(0.6) < key=10 fraction(1.0) — key=11 先吞，右→左顺序保持
        assertTrue(
            "修复后：key=11 fraction=$fraction11 < key=10 fraction=$fraction10，" +
                "key=11 先于 key=10 开始吞 — traversal 顺序保持 12→11→10",
            fraction11 < fraction10,
        )

        // 5. 补充验证 key=12 已完成（fraction=0），证明 key=12 在最前继续
        assertEquals(
            "key=12 应已完成吞（fraction=0），实际=$fraction12",
            0f,
            fraction12,
            0.001f,
        )
    }

    /**
     * 问题2 补充：验证新 channel 的区间分配，直接证明 traversal 顺序是 12→11→10（正确）。
     *
     * 修复后：
     * - key=12: startProgress=0, endProgress=0.5（in-progress，最前）
     * - key=11: startProgress=0.5, endProgress=0.75（pending 第一，第二位 — 正确）
     * - key=10: startProgress=0.75, endProgress=1.0（pending 第二，最后 — 正确）
     */
    @Test
    fun regression_issue2_channelRangesShowCorrectOrder() {
        val motion1 =
            ComposeEditMotion.forDelete(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedUnitKeys = listOf(10L, 11L, 12L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val midTime = startTime + duration / 6
        val motion2 =
            motion1.redirectCaretTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                frameTimeNanos = midTime,
                durationNanos = duration,
            )

        // 通过 sample 推断区间边界（不依赖内部字段，纯 API 验证）
        // sample 到新 motion 50% 时间（glyphProgress=0.5）：key=12 刚好走完
        val atFifty = motion2.sample(midTime + duration / 2)
        // key=12 在 [0, 0.5]，0.5 >= endProgress → fraction=0
        assertEquals("50% 时 key=12 应已完成 fraction=0", 0f, atFifty.unitClipFractions[12L]!!, 0.001f)
        // key=11 在 [0.5, 0.75]，0.5 <= startProgress=0.5 → 还没开始 fraction=1
        assertEquals("50% 时 key=11 应还没开始 fraction=1", 1f, atFifty.unitClipFractions[11L]!!, 0.001f)
        // key=10 在 [0.75, 1.0]，0.5 <= 0.75 → 还没开始 fraction=1
        assertEquals("50% 时 key=10 应还没开始 fraction=1", 1f, atFifty.unitClipFractions[10L]!!, 0.001f)

        // sample 到新 motion 75% 时间（glyphProgress=0.75）：
        val atSeventyFive = motion2.sample(midTime + duration * 3 / 4)
        // key=11 在 [0.5, 0.75]，0.75 >= endProgress=0.75 → 已完成 fraction=0
        assertEquals(
            "修复后：75% 时 key=11 应已完成 fraction=0（区间 [0.5,0.75]），实际=${atSeventyFive.unitClipFractions[11L]}",
            0f,
            atSeventyFive.unitClipFractions[11L]!!,
            0.001f,
        )
        // key=10 在 [0.75, 1.0]，0.75 <= startProgress=0.75 → 还没开始 fraction=1
        assertEquals(
            "修复后：75% 时 key=10 应还没开始 fraction=1（区间 [0.75,1.0]），实际=${atSeventyFive.unitClipFractions[10L]}",
            1f,
            atSeventyFive.unitClipFractions[10L]!!,
            0.001f,
        )
        // 关键：75% 时 key=11 已完成但 key=10 还没开始 → key=11 在 key=10 之前 → 顺序正确 12→11→10
        assertTrue(
            "修复后：75% 时 key=11 已完成（fraction=0）但 key=10 还没开始（fraction=1），" +
                "key=11 排在 key=10 之前 — traversal 顺序保持 12→11→10",
            atSeventyFive.unitClipFractions[11L]!! < atSeventyFive.unitClipFractions[10L]!!,
        )
    }
}
