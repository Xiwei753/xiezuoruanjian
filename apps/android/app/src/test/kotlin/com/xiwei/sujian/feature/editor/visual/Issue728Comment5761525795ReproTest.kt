package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Issue #728 评论 5761525795 复现测试 —
 * timeline split/rebase 换 child key 后，ComposeEditMotion 没有继承 parent 的当前 fraction/phase。
 *
 * 多字符 unit 被部分删除时，child 会被当成全新 unit 从 0/1 重启，产生闪烁/重影。
 *
 * 场景（对应 issue 描述的链路）：
 * 1. 一个正在吐的 parent unit：parent key=10, range=[0,3), current motion fraction=0.4
 * 2. 用户动画没结束就 Backspace 删除最后一个字符 [2,3)
 * 3. ComposeVisualTimeline.mapSurvivingUnits() 对 parent 做 split：
 *    - isSplit == true → childKey = nextUnitKey++
 *    - surviving child key=11 range=[0,2)，deleted ghost child key=12 range=[2,3)
 *    - parent key=10 从 timeline 退出
 * 4. ComposeEditorVisualState.drainPendingPatchesAtFrame() 调用 activeEditUnits() 拿到新 key 11/12
 * 5. 调用旧 activeEditMotion.redirectTo(inserted=[11], deleted=[12])
 * 6. 但旧 motion 的 unitChannels 只有 parent key=10
 * 7. redirectTo() 查 unitChannels[11]/unitChannels[12] 都是 null → 当成全新 unit：
 *    inserted child 从 0→1 重吐，deleted child 从 1→0 重吞
 *
 * 正确行为（由 [computeRevealFractionForChild] 投影）：
 * - surviving child 首帧 fraction = computeRevealFractionForChild(parentRange, parentFraction, childRange)
 * - deleted ghost child 首帧 fraction = computeRevealFractionForChild(parentRange, parentFraction, childRange)
 * 不应该从 0 重吐 / 补满到 1 重吞。
 *
 * 本测试直接在 ComposeEditMotion API 层复现：构造 parent motion（只有 parent key），
 * 再 redirectTo 传入 split 后的 child key（不在旧 channels 里），验证首帧 fraction。
 * 纯 API 测试，不需要 Robolectric/ComposeRule/反射。
 */
@Suppress("MaxLineLength", "LongMethod")
class Issue728Comment5761525795ReproTest {
    private val originRect = Rect(left = 10f, top = 0f, right = 12f, bottom = 20f)
    private val targetRect = Rect(left = 30f, top = 0f, right = 32f, bottom = 20f)
    private val startTime = 1_000_000_000L
    private val duration = 100_000_000L // 100ms in nanos

    // parent "abc" range=[0,3)；split 后 surviving child "ab" range=[0,2)，deleted ghost child "c" range=[2,3)
    private val parentRange = TextRange(start = 0, end = 3)
    private val survivingChildRange = TextRange(start = 0, end = 2)
    private val deletedGhostChildRange = TextRange(start = 2, end = 3)

    /**
     * 主复现：parent fraction=0.4 时 split，surviving child 首帧 fraction 应继承 parent 可见状态（0.6），
     * 不应该从 0 重吐。
     *
     * 推导：
     * - parent fraction=0.4 → revealBoundary = 0 + 0.4 * 3 = 1.2
     * - surviving child [0,2)：end=2 > 1.2, start=0 < 1.2 → (1.2-0)/2 = 0.6
     * - 当前 bug：childKey=11 不在旧 unitChannels（只有 parent key=10）→ NewUnitPending
     *   → from=0, to=1 → 首帧 fraction=0（从 0 重吐）
     */
    @Test
    fun repro_survivingChild_shouldInheritParentFraction_notRestartFromZero() {
        // 1. parent motion：forInsert parent key=10（模拟 "abc" range=[0,3) 吐字）
        val parentMotion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 2. sample 到 40% 时间：parent key=10 fraction=0.4
        val midTime = startTime + duration * 2 / 5 // 40ms
        val midSample = parentMotion.sample(midTime)
        assertEquals("parent key=10 在 40% 时 fraction 应为 0.4", 0.4f, midSample.unitClipFractions[10L]!!, 0.001f)

        // 3. split 后 redirectTo：surviving child key=11（inserted），deleted ghost child key=12（deleted）
        //    旧 motion 的 unitChannels 只有 parent key=10，11/12 都是新 key
        //    Issue #728 评论 5761525795：传入 inheritedFractionsByKey — split child 从 parent 投影后的
        //    fraction 继续，不从 0/1 重启。
        val childMotion =
            parentMotion.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = listOf(11L),
                newDeletedUnitKeys = listOf(12L),
                frameTimeNanos = midTime,
                durationNanos = duration,
                inheritedFractionsByKey =
                    mapOf(
                        11L to computeRevealFractionForChild(parentRange, 0.4f, survivingChildRange),
                        12L to computeRevealFractionForChild(parentRange, 0.4f, deletedGhostChildRange),
                    ),
            )

        // 4. 新 motion 首帧（split 那一帧）：sample(midTime) 时 elapsed=0 → progress=0
        val firstFrame = childMotion.sample(midTime)
        val survivingFraction = firstFrame.unitClipFractions[11L]!!

        // 5. 期望：surviving child 首帧 fraction = computeRevealFractionForChild([0,3), 0.4, [0,2)) = 0.6
        val expectedSurvivingFraction =
            computeRevealFractionForChild(
                parentRange = parentRange,
                parentRevealFraction = 0.4f,
                childRange = survivingChildRange,
            )
        assertEquals("expected surviving child fraction 应为 0.6", 0.6f, expectedSurvivingFraction, 0.001f)

        // 关键断言：surviving child 首帧 fraction 应继承 parent（0.6），不应从 0 重吐
        assertEquals(
            "split 后 surviving child 首帧 fraction 应继承 parent 可见状态（0.6），" +
                "不应从 0 重吐（bug：childKey 不在旧 unitChannels → NewUnitPending → from=0），实际=$survivingFraction",
            expectedSurvivingFraction,
            survivingFraction,
            0.001f,
        )
        // 显式断言不是 0（重吐）
        assertNotEquals(
            "surviving child 首帧 fraction 不应是 0（从 0 重吐的 bug 行为），实际=$survivingFraction",
            0f,
            survivingFraction,
        )
    }

    /**
     * 主复现：parent fraction=0.4 时 split，deleted ghost child 首帧 fraction 应继承 parent 可见状态（0.0，
     * 因为 reveal 边界 1.2 落在 [2,3) 之前，[2,3) 还没吐出来），不应该补满到 1 重吞。
     *
     * 推导：
     * - parent fraction=0.4 → revealBoundary = 1.2
     * - deleted ghost child [2,3)：start=2 >= 1.2 → 0f（还没吐出来，本来不可见）
     * - 当前 bug：childKey=12 不在旧 unitChannels → NewUnitPending
     *   → from=1, to=0 → 首帧 fraction=1（补满到 1 重吞）
     *
     * deleted ghost 从 0 开始吞本就不可见的字没有视觉问题，但 bug 行为补满到 1 会让
     * "本来还没吐出来的字"突然冒出来再吞，产生重影。
     */
    @Test
    fun repro_deletedGhostChild_shouldInheritParentFraction_notRefillToOne() {
        val parentMotion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val midTime = startTime + duration * 2 / 5
        val midSample = parentMotion.sample(midTime)
        assertEquals("parent key=10 在 40% 时 fraction 应为 0.4", 0.4f, midSample.unitClipFractions[10L]!!, 0.001f)

        val childMotion =
            parentMotion.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = listOf(11L),
                newDeletedUnitKeys = listOf(12L),
                frameTimeNanos = midTime,
                durationNanos = duration,
                inheritedFractionsByKey =
                    mapOf(
                        11L to computeRevealFractionForChild(parentRange, 0.4f, survivingChildRange),
                        12L to computeRevealFractionForChild(parentRange, 0.4f, deletedGhostChildRange),
                    ),
            )

        val firstFrame = childMotion.sample(midTime)
        val ghostFraction = firstFrame.unitClipFractions[12L]!!

        val expectedGhostFraction =
            computeRevealFractionForChild(
                parentRange = parentRange,
                parentRevealFraction = 0.4f,
                childRange = deletedGhostChildRange,
            )
        assertEquals("expected deleted ghost child fraction 应为 0.0", 0.0f, expectedGhostFraction, 0.001f)

        // 关键断言：deleted ghost child 首帧 fraction 应继承 parent（0.0），不应补满到 1 重吞
        assertEquals(
            "split 后 deleted ghost child 首帧 fraction 应继承 parent 可见状态（0.0），" +
                "不应补满到 1 重吞（bug：childKey 不在旧 unitChannels → NewUnitPending → from=1），实际=$ghostFraction",
            expectedGhostFraction,
            ghostFraction,
            0.001f,
        )
        // 显式断言不是 1（重吞）
        assertNotEquals(
            "deleted ghost child 首帧 fraction 不应是 1（补满到 1 重吞的 bug 行为），实际=$ghostFraction",
            1f,
            ghostFraction,
        )
    }

    /**
     * 补充复现：parent fraction=0.8 时 split，reveal 边界落在 deleted ghost child [2,3) 内，
     * deleted ghost child 首帧 fraction 应继承 parent 可见状态（0.4），不应补满到 1 重吞。
     *
     * 推导：
     * - parent fraction=0.8 → revealBoundary = 0 + 0.8 * 3 = 2.4
     * - surviving child [0,2)：end=2 <= 2.4 → 1f（已完整显示）
     * - deleted ghost child [2,3)：end=3 > 2.4, start=2 < 2.4 → (2.4-2)/1 = 0.4
     * - 当前 bug：deleted ghost child from=1 → 首帧 fraction=1（补满到 1 重吞）
     *
     * 这个场景更直观：parent 已经吐到 [2,3) 的 40%（0.4 可见），split 后 deleted ghost
     * 应从 0.4 继续吞到 0，不应补满到 1 再吞。
     */
    @Test
    fun repro_deletedGhostChild_boundaryInsideChild_shouldInheritPartialFraction() {
        val parentMotion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 80% 时间：parent fraction=0.8
        val midTime = startTime + duration * 4 / 5 // 80ms
        val midSample = parentMotion.sample(midTime)
        assertEquals("parent key=10 在 80% 时 fraction 应为 0.8", 0.8f, midSample.unitClipFractions[10L]!!, 0.001f)

        val childMotion =
            parentMotion.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = listOf(11L),
                newDeletedUnitKeys = listOf(12L),
                frameTimeNanos = midTime,
                durationNanos = duration,
                inheritedFractionsByKey =
                    mapOf(
                        11L to computeRevealFractionForChild(parentRange, 0.8f, survivingChildRange),
                        12L to computeRevealFractionForChild(parentRange, 0.8f, deletedGhostChildRange),
                    ),
            )

        val firstFrame = childMotion.sample(midTime)
        val ghostFraction = firstFrame.unitClipFractions[12L]!!
        val survivingFraction = firstFrame.unitClipFractions[11L]!!

        // 期望：deleted ghost child = 0.4，surviving child = 1.0
        val expectedGhostFraction =
            computeRevealFractionForChild(
                parentRange = parentRange,
                parentRevealFraction = 0.8f,
                childRange = deletedGhostChildRange,
            )
        val expectedSurvivingFraction =
            computeRevealFractionForChild(
                parentRange = parentRange,
                parentRevealFraction = 0.8f,
                childRange = survivingChildRange,
            )
        assertEquals("expected deleted ghost child fraction 应为 0.4", 0.4f, expectedGhostFraction, 0.001f)
        assertEquals("expected surviving child fraction 应为 1.0", 1.0f, expectedSurvivingFraction, 0.001f)

        // deleted ghost child 应从 0.4 继续吞，不应补满到 1
        assertEquals(
            "parent 吐到 80% 时 [2,3) 已显示 40%，deleted ghost child 首帧 fraction 应为 0.4，" +
                "不应补满到 1 重吞，实际=$ghostFraction",
            expectedGhostFraction,
            ghostFraction,
            0.001f,
        )
        assertTrue(
            "deleted ghost child 首帧 fraction 不应是 1（补满到 1 重吞的 bug 行为），实际=$ghostFraction",
            ghostFraction < 0.999f,
        )
    }

    /**
     * 补充复现：parent fraction=0.8 时 split，surviving child [0,2) 已完整显示，
     * 首帧 fraction 应为 1.0，不应从 0 重吐。
     *
     * 推导：
     * - parent fraction=0.8 → revealBoundary = 2.4
     * - surviving child [0,2)：end=2 <= 2.4 → 1f（已完整显示）
     * - 当前 bug：surviving child from=0 → 首帧 fraction=0（从 0 重吐）
     *
     * parent 已经把 "ab" 全吐出来了，split 后 surviving child 应保持完整可见（1.0），
     * 不应突然消失再从 0 重吐。
     */
    @Test
    fun repro_survivingChild_fullyRevealed_shouldStayFullNotRestartFromZero() {
        val parentMotion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val midTime = startTime + duration * 4 / 5
        val midSample = parentMotion.sample(midTime)
        assertEquals("parent key=10 在 80% 时 fraction 应为 0.8", 0.8f, midSample.unitClipFractions[10L]!!, 0.001f)

        val childMotion =
            parentMotion.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = listOf(11L),
                newDeletedUnitKeys = listOf(12L),
                frameTimeNanos = midTime,
                durationNanos = duration,
                inheritedFractionsByKey =
                    mapOf(
                        11L to computeRevealFractionForChild(parentRange, 0.8f, survivingChildRange),
                        12L to computeRevealFractionForChild(parentRange, 0.8f, deletedGhostChildRange),
                    ),
            )

        val firstFrame = childMotion.sample(midTime)
        val survivingFraction = firstFrame.unitClipFractions[11L]!!

        val expectedSurvivingFraction =
            computeRevealFractionForChild(
                parentRange = parentRange,
                parentRevealFraction = 0.8f,
                childRange = survivingChildRange,
            )
        assertEquals("expected surviving child fraction 应为 1.0", 1.0f, expectedSurvivingFraction, 0.001f)

        assertEquals(
            "parent 吐到 80% 时 [0,2) 已完整显示，surviving child 首帧 fraction 应为 1.0，" +
                "不应从 0 重吐，实际=$survivingFraction",
            expectedSurvivingFraction,
            survivingFraction,
            0.001f,
        )
        assertTrue(
            "surviving child 首帧 fraction 不应是 0（从 0 重吐的 bug 行为），实际=$survivingFraction",
            survivingFraction > 0.999f,
        )
    }

    /**
     * 链路断言：确认 bug 的根因与修复 — split 后 child key 不在旧 motion 的 unitChannels 里，
     * redirectTo 用 inheritedFraction 区分"split/rekey child"和"真正全新 unit"。
     *
     * Issue #728 评论 5761525795 修复后：
     * - 不传 inheritedFractionsByKey（默认空 map）：child 无继承信息，按全新 unit 处理 —
     *   inserted child from=0（首帧 0），deleted child from=1（首帧 1）。
     *   这是向后兼容行为，保证真正本笔新插入/新建 deleted ghost 仍从 0/1 开始。
     * - 传 inheritedFractionsByKey：child 有继承信息，从 inheritedFraction 继续 —
     *   surviving child 首帧=0.6（继承 parent），deleted ghost child 首帧=0.0（继承 parent）。
     *   这是修复后的正确行为，避免闪烁/重影。
     */
    @Test
    fun repro_bugRootCause_childKeyNotInOldUnitChannels_treatedAsNewUnit() {
        val parentMotion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val midTime = startTime + duration * 2 / 5
        val midSample = parentMotion.sample(midTime)
        // parent key=10 在旧 motion 里，fraction=0.4
        assertEquals("parent key=10 fraction 应为 0.4", 0.4f, midSample.unitClipFractions[10L]!!, 0.001f)

        // 期望继承 parent fraction（surviving=0.6, ghost=0.0）
        val expectedSurviving =
            computeRevealFractionForChild(parentRange, 0.4f, survivingChildRange)
        val expectedGhost =
            computeRevealFractionForChild(parentRange, 0.4f, deletedGhostChildRange)

        // 第一部分：不传 inheritedFractionsByKey（默认空 map）—
        // child 无继承信息，redirectTo 按全新 unit 处理（NewUnitPending）：
        // inserted child from=0 → 首帧 fraction=0，deleted child from=1 → 首帧 fraction=1。
        // 这是向后兼容行为，保证真正本笔新插入/新建 deleted ghost 仍从 0/1 开始。
        val bugMotion =
            parentMotion.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = listOf(11L),
                newDeletedUnitKeys = listOf(12L),
                frameTimeNanos = midTime,
                durationNanos = duration,
            )
        val bugFrame = bugMotion.sample(midTime)
        val bugSurviving = bugFrame.unitClipFractions[11L]!!
        val bugGhost = bugFrame.unitClipFractions[12L]!!
        assertEquals(
            "无继承信息时 inserted child 应按全新 unit 从 0 开始（向后兼容），实际=$bugSurviving",
            0f,
            bugSurviving,
            0.001f,
        )
        assertEquals(
            "无继承信息时 deleted child 应按全新 unit 从 1 开始（向后兼容），实际=$bugGhost",
            1f,
            bugGhost,
            0.001f,
        )

        // 第二部分：传 inheritedFractionsByKey —
        // child 有继承信息，redirectTo 从 inheritedFraction 继续（不当作 NewUnitPending）：
        // surviving child 首帧=0.6，deleted ghost child 首帧=0.0。
        // 这是修复后的正确行为，避免闪烁/重影。
        val childMotion =
            parentMotion.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = listOf(11L),
                newDeletedUnitKeys = listOf(12L),
                frameTimeNanos = midTime,
                durationNanos = duration,
                inheritedFractionsByKey =
                    mapOf(
                        11L to expectedSurviving,
                        12L to expectedGhost,
                    ),
            )
        val firstFrame = childMotion.sample(midTime)
        val survivingFraction = firstFrame.unitClipFractions[11L]!!
        val ghostFraction = firstFrame.unitClipFractions[12L]!!

        assertEquals(
            "有继承信息时 surviving child 首帧 fraction 应继承 parent（0.6），实际=$survivingFraction",
            expectedSurviving,
            survivingFraction,
            0.001f,
        )
        assertEquals(
            "有继承信息时 deleted ghost child 首帧 fraction 应继承 parent（0.0），实际=$ghostFraction",
            expectedGhost,
            ghostFraction,
            0.001f,
        )
    }
}
