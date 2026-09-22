package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Issue #728 评论 5762435453 复现测试 —
 * `ComposeEditorVisualState.drainPendingPatchesAtFrame` 里 motion 创建的分支条件漏洞。
 *
 * **边界场景（评论"建议固定一个场景"）**：
 * 1. parent 多字符 unit（"abc" range=[0,3)，key=10）在 100ms 动画完整吐完；
 * 2. 90ms 那帧 timeline 还持有 parent（motion 还没 finished，sample 到 90%）；
 * 3. 下一帧 110ms：motion 已数学完成（`isFinished(110ms) == true`），但 timeline 还没执行
 *    本帧 sample/收口 parent，parent unit 仍在 timeline；
 * 4. 110ms 帧先到一笔部分删除（Backspace 删 [2,3)），`applyPatch` 对 parent 做 split，
 *    算出 child 的 inherited fraction（surviving child=1、deleted child=1，因为 parent 已完整显示）；
 * 5. `drainPendingPatchesAtFrame` 创建 motion 时判断走哪条分支。
 *
 * **Bug**：原条件 `existing != null && !existing.isFinished(frameTimeNanos)` 在 110ms 帧为 false
 * （existing 已 finished），走 `forEdit()`。`forEdit()` 没有 inheritedFractions 参数，lineage 被丢弃：
 * surviving inserted child 从 0→1 重吐，deleted child 从 1→0 重吞。直观结果：已完整显示的
 * surviving child 突然变 0 再重吐一次。
 *
 * **修复**：在分支判断之前先算出 `inheritedFractionsByKey`，把条件改为
 * `existing != null && (!existing.isFinished(frameTimeNanos) || inheritedFractionsByKey.isNotEmpty())`。
 * motion finished 但 timeline 本帧仍 split/rekey 出 child 时，继续走 `existing.redirectTo(... inheritedFractionsByKey=...)`，
 * `classifyRedirectSpec` 对 `oldChannel == null` 且 `inheritedFraction != null` 的 unit 用 inherited fraction
 * 分类（FixedTerminal/Pending），不依赖 motion 是否 finished，只依赖 caller 传入的 inheritedFraction。
 *
 * 本测试在 `ComposeEditMotion` API 层复现该帧边界：构造一个已 finished 的 parent motion，
 * 然后调 `redirectTo(... inheritedFractionsByKey = mapOf(childKey to 1f))`，
 * 验证 surviving child 首帧 fraction=1（继承 parent 终点，不从 0 重吐）。
 * 同时用 `forEdit()` 复现 bug 路径，锁定分支条件 — motion finished 时不能走 forEdit 丢弃 lineage。
 *
 * 纯 API 测试，不需要 Robolectric/ComposeRule/反射。
 */
@Suppress("MaxLineLength", "LongMethod")
class Issue728Comment5762435453ReproTest {
    private val originRect = Rect(left = 10f, top = 0f, right = 12f, bottom = 20f)
    private val targetRect = Rect(left = 30f, top = 0f, right = 32f, bottom = 20f)
    private val startTime = 1_000_000_000L
    private val duration = 100_000_000L // 100ms in nanos

    // parent "abc" range=[0,3)；split 后 surviving child "ab" range=[0,2)，deleted ghost child "c" range=[2,3)
    private val parentRange = TextRange(start = 0, end = 3)
    private val survivingChildRange = TextRange(start = 0, end = 2)
    private val deletedGhostChildRange = TextRange(start = 2, end = 3)

    /**
     * 主复现：parent motion 已 finished（动画完整吐完），timeline 本帧 split 出 child，
     * `redirectTo` 传 `inheritedFractionsByKey` 时 surviving child 首帧 fraction 应继承 parent 终点（1.0），
     * 不应从 0 重吐。
     *
     * 推导（parent fraction=1，已完整显示）：
     * - revealBoundary = 0 + 1 * 3 = 3
     * - surviving child [0,2)：end=2 <= 3 → 1f（已完整显示）
     * - deleted ghost child [2,3)：end=3 <= 3 → 1f（已完整显示）
     * - `classifyRedirectSpec`：surviving child inherited=1, desiredTo=1 → FixedTerminal(1) → 首帧 fraction=1
     *
     * 修复前 bug 路径（`forEdit`）：surviving child from=0, to=1 → 首帧 fraction=0（从 0 重吐）。
     */
    @Test
    fun repro_survivingChild_motionFinished_timelineSplit_shouldInheritFullNotRestartFromZero() {
        // 1. parent motion：forInsert parent key=10（模拟 "abc" range=[0,3) 吐字）
        val parentMotion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 2. 让 motion finished：finishedTime = startTime + duration + 1
        val finishedTime = startTime + duration + 1L
        assertTrue("parent motion 在 finishedTime 应已 finished", parentMotion.isFinished(finishedTime))
        val finishedSample = parentMotion.sample(finishedTime)
        assertEquals("parent key=10 finished 时 fraction 应为 1.0", 1.0f, finishedSample.unitClipFractions[10L]!!, 0.001f)

        // 3. timeline split 后 redirectTo：surviving child key=11（inserted），deleted ghost child key=12（deleted）
        //    parent 已完整显示 → 两个 child 的 inherited fraction 都是 1f
        val expectedSurvivingFraction =
            computeRevealFractionForChild(
                parentRange = parentRange,
                parentRevealFraction = 1.0f,
                childRange = survivingChildRange,
            )
        val expectedGhostFraction =
            computeRevealFractionForChild(
                parentRange = parentRange,
                parentRevealFraction = 1.0f,
                childRange = deletedGhostChildRange,
            )
        assertEquals("expected surviving child fraction 应为 1.0", 1.0f, expectedSurvivingFraction, 0.001f)
        assertEquals("expected deleted ghost child fraction 应为 1.0", 1.0f, expectedGhostFraction, 0.001f)

        // Issue #728 评论 5762435453 修复：motion finished 但 inheritedFractionsByKey 非空 → 继续走 redirectTo
        val childMotion =
            parentMotion.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = listOf(11L),
                newDeletedUnitKeys = listOf(12L),
                frameTimeNanos = finishedTime,
                durationNanos = duration,
                inheritedFractionsByKey =
                    mapOf(
                        11L to expectedSurvivingFraction,
                        12L to expectedGhostFraction,
                    ),
            )

        // 4. 新 motion 首帧（split 那一帧）：sample(finishedTime) 时 elapsed=0 → progress=0
        val firstFrame = childMotion.sample(finishedTime)
        val survivingFraction = firstFrame.unitClipFractions[11L]!!

        // 5. 关键断言：surviving child 首帧 fraction 应继承 parent 终点（1.0），不应从 0 重吐
        assertEquals(
            "motion finished 后 timeline split，surviving child 首帧 fraction 应继承 parent 终点（1.0），" +
                "不应从 0 重吐（bug：motion finished 走 forEdit 丢弃 inheritedFraction），实际=$survivingFraction",
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
        assertTrue(
            "surviving child 首帧 fraction 应保持完整可见（1.0），实际=$survivingFraction",
            survivingFraction > 0.999f,
        )
    }

    /**
     * Bug 路径锁定：motion finished 时如果错误地走 `forEdit()` 而非 `redirectTo(... inheritedFractionsByKey=...)`，
     * surviving child 会从 0 重吐。
     *
     * 这个测试不依赖修复后的分支条件，直接调 `forEdit()` 复现 bug 行为，
     * 锁定"motion finished 时不能走 forEdit 丢弃 lineage"这一分支条件要求。
     *
     * 推导（`forEdit` 不知道 inheritedFraction）：
     * - surviving child key=11：from=0, to=1 → 首帧 fraction=0（从 0 重吐）
     * - deleted child key=12：from=1, to=0 → 首帧 fraction=1
     */
    @Test
    fun bugPath_forEdit_motionFinished_survivingChildRestartsFromZero() {
        val parentMotion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val finishedTime = startTime + duration + 1L
        assertTrue("parent motion 在 finishedTime 应已 finished", parentMotion.isFinished(finishedTime))

        // bug 路径：motion finished → 走 forEdit（不传 inheritedFractions，lineage 丢失）
        val bugMotion =
            ComposeEditMotion.forEdit(
                originCaretRect = targetRect,
                targetCaretRect = originRect,
                insertedUnitKeys = listOf(11L),
                deletedUnitKeys = listOf(12L),
                frameTimeNanos = finishedTime,
                durationNanos = duration,
            )
        val bugFrame = bugMotion.sample(finishedTime)
        val bugSurviving = bugFrame.unitClipFractions[11L]!!

        // bug 行为：surviving child 从 0 重吐
        assertEquals(
            "bug 路径（forEdit）surviving child 首帧 fraction 应为 0（从 0 重吐），实际=$bugSurviving",
            0f,
            bugSurviving,
            0.001f,
        )
        // 这就是 bug：已完整显示的 surviving child 突然变 0
        assertFalse(
            "bug 路径 surviving child 不应保持完整可见，实际=$bugSurviving",
            bugSurviving > 0.999f,
        )
    }

    /**
     * 分支条件验证：motion finished 且 `inheritedFractionsByKey` 非空时，
     * `redirectTo` 路径让 surviving child 保持 inherited fraction（1.0），
     * `forEdit` 路径让 surviving child 从 0 重吐。
     *
     * 两条路径首帧 fraction 不同，证明 motion finished 时分支选择决定行为，
     * 修复后的条件 `!isFinished || inheritedFractionsByKey.isNotEmpty()` 必须选 redirectTo。
     */
    @Test
    fun branchCondition_motionFinished_withInheritedFractions_redirectPreservesLineage() {
        val parentMotion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val finishedTime = startTime + duration + 1L
        assertTrue(parentMotion.isFinished(finishedTime))

        val inheritedFractionsByKey = mapOf(11L to 1.0f, 12L to 1.0f)

        // 修复路径：redirectTo + inheritedFractionsByKey
        val fixedMotion =
            parentMotion.redirectTo(
                newOriginCaretRect = targetRect,
                newTargetCaretRect = originRect,
                newInsertedUnitKeys = listOf(11L),
                newDeletedUnitKeys = listOf(12L),
                frameTimeNanos = finishedTime,
                durationNanos = duration,
                inheritedFractionsByKey = inheritedFractionsByKey,
            )
        // bug 路径：forEdit（motion finished 时错误走的分支）
        val bugMotion =
            ComposeEditMotion.forEdit(
                originCaretRect = targetRect,
                targetCaretRect = originRect,
                insertedUnitKeys = listOf(11L),
                deletedUnitKeys = listOf(12L),
                frameTimeNanos = finishedTime,
                durationNanos = duration,
            )

        val fixedSurviving = fixedMotion.sample(finishedTime).unitClipFractions[11L]!!
        val bugSurviving = bugMotion.sample(finishedTime).unitClipFractions[11L]!!

        // 修复路径保持 inherited fraction=1，bug 路径从 0 重吐
        assertEquals("修复路径 surviving child 应为 1.0", 1.0f, fixedSurviving, 0.001f)
        assertEquals("bug 路径 surviving child 应为 0", 0f, bugSurviving, 0.001f)
        assertNotEquals(
            "motion finished 时 redirectTo 和 forEdit 路径行为必须不同（否则分支条件无意义）",
            fixedSurviving,
            bugSurviving,
            0.001f,
        )
    }

    /**
     * 向后兼容：motion finished 且 `inheritedFractionsByKey` 为空（真正本笔新插入/新建 deleted ghost，
     * 无 parent lineage）时，应走 `forEdit` 路径，child 从 0/1 开始。
     *
     * 这保证修复后的条件 `!isFinished || inheritedFractionsByKey.isNotEmpty()` 在无继承信息时
     * 仍走 forEdit（向后兼容），不会因为 motion finished 就强行 redirectTo 旧 motion（旧 motion 已无意义）。
     *
     * 推导（`inheritedFractionsByKey` 为空 → 条件为 false → forEdit）：
     * - surviving child key=11：from=0, to=1 → 首帧 fraction=0（本笔新插入，从 0 开始吐）
     * - deleted child key=12：from=1, to=0 → 首帧 fraction=1（本笔新建 ghost，从 1 开始吞）
     */
    @Test
    fun backwardCompatible_motionFinished_noInheritedFractions_goesForEdit() {
        val parentMotion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val finishedTime = startTime + duration + 1L
        assertTrue(parentMotion.isFinished(finishedTime))

        // 无继承信息 → forEdit 路径（向后兼容）
        val motion =
            ComposeEditMotion.forEdit(
                originCaretRect = targetRect,
                targetCaretRect = originRect,
                insertedUnitKeys = listOf(11L),
                deletedUnitKeys = listOf(12L),
                frameTimeNanos = finishedTime,
                durationNanos = duration,
            )
        val frame = motion.sample(finishedTime)
        val survivingFraction = frame.unitClipFractions[11L]!!
        val ghostFraction = frame.unitClipFractions[12L]!!

        assertEquals(
            "无继承信息时 surviving child 应从 0 开始（本笔新插入），实际=$survivingFraction",
            0f,
            survivingFraction,
            0.001f,
        )
        assertEquals(
            "无继承信息时 deleted child 应从 1 开始（本笔新建 ghost），实际=$ghostFraction",
            1f,
            ghostFraction,
            0.001f,
        )
    }

    /**
     * 中间帧验证：parent motion 未 finished（90ms，sample 到 90%），timeline split 出 child，
     * `redirectTo` 传 `inheritedFractionsByKey` 时 surviving child 首帧 fraction 应继承 parent（1.0）。
     *
     * 这覆盖原条件 `!existing.isFinished(frameTimeNanos)` 为 true 的路径（已有 5761525795 测试覆盖），
     * 与本 issue 的 motion finished 路径形成对照，确认两条路径在 inherited fraction 下行为一致。
     *
     * 推导（parent fraction=0.9）：
     * - revealBoundary = 0 + 0.9 * 3 = 2.7
     * - surviving child [0,2)：end=2 <= 2.7 → 1f（已完整显示）
     * - deleted ghost child [2,3)：end=3 > 2.7, start=2 < 2.7 → (2.7-2)/1 = 0.7
     */
    @Test
    fun contrast_motionNotFinished_timelineSplit_survivingChildInheritsFraction() {
        val parentMotion =
            ComposeEditMotion.forInsert(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedUnitKeys = listOf(10L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 90ms：parent fraction=0.9，motion 未 finished
        val midTime = startTime + duration * 9 / 10
        assertFalse("parent motion 在 90ms 应未 finished", parentMotion.isFinished(midTime))
        val midSample = parentMotion.sample(midTime)
        assertEquals("parent key=10 在 90% 时 fraction 应为 0.9", 0.9f, midSample.unitClipFractions[10L]!!, 0.001f)

        val expectedSurvivingFraction =
            computeRevealFractionForChild(
                parentRange = parentRange,
                parentRevealFraction = 0.9f,
                childRange = survivingChildRange,
            )
        val expectedGhostFraction =
            computeRevealFractionForChild(
                parentRange = parentRange,
                parentRevealFraction = 0.9f,
                childRange = deletedGhostChildRange,
            )
        assertEquals("expected surviving child fraction 应为 1.0", 1.0f, expectedSurvivingFraction, 0.001f)
        assertEquals("expected deleted ghost child fraction 应为 0.7", 0.7f, expectedGhostFraction, 0.001f)

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
                        11L to expectedSurvivingFraction,
                        12L to expectedGhostFraction,
                    ),
            )

        val firstFrame = childMotion.sample(midTime)
        val survivingFraction = firstFrame.unitClipFractions[11L]!!
        val ghostFraction = firstFrame.unitClipFractions[12L]!!

        assertEquals(
            "motion 未 finished 时 surviving child 首帧 fraction 应继承 parent（1.0），实际=$survivingFraction",
            expectedSurvivingFraction,
            survivingFraction,
            0.001f,
        )
        assertEquals(
            "motion 未 finished 时 deleted ghost child 首帧 fraction 应继承 parent（0.7），实际=$ghostFraction",
            expectedGhostFraction,
            ghostFraction,
            0.001f,
        )
    }
}
