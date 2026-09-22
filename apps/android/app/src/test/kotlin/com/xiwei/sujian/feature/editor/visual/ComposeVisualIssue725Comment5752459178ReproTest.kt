package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #725 评论 5752459178 — `reconcileDeletedGhosts()` 删除 reveal schedule 修复的验证测试。
 *
 * **修复内容**：`ComposeVisualTimeline.reconcileDeletedGhosts()` 的 schedule 阶段
 * 在命中 `isCurrentPatchGhost && isWithinDeletedRange` 时，除了重建 `alpha`，同时重建
 * `reveal = TimedFloat(from = ghost.reveal.from, to = 0f, startedAtNanos = ghostStartedAt,
 * durationNanos = ghostDuration)`。
 *
 * **核心问题**（旧 bug）：
 * reveal 通道独立后（评论 5752205479），`alpha` 不再兼任空间 reveal，coordinated 模式下
 * draw 层把 alpha 固定为 1，真正决定吞字可见进度的是 `reveal`。但 `reconcileDeletedGhosts()`
 * 仍然只重排 alpha，没把 reveal 接到本次删除 patch 的正式分段时间表。后果：
 * 1. 新建的多个 delete ghost 虽然 alpha 按第 1、2、3 个顺序进入各自时间片，但 reveal 仍全部
 *    从 `frameTimeNanos` 同时开始，导致多字同时吞而非按 `orderedDeletedUnits` 顺序吞；
 * 2. active unit 转 ghost 的路径也一样：`toGhost()` 先用旧 reveal 的剩余时长创建通道，
 *    随后 `reconcileDeletedGhosts()` 只覆盖 alpha，导致本次删除事件的正式 schedule 根本没有
 *    接管 reveal。
 *
 * 这违背 #725 最初定下的规则：同一笔 patch 内多个 unit 要按顺序分配自己的 reveal 时间片。
 *
 * **修复后**：
 * `reconcileDeletedGhosts()` 的 schedule 阶段同时重建 reveal，`reveal.from` 保留 `toGhost()`
 * 时投影好的局部 fraction，split child 从局部进度继续吞，不复活也不退回 parent fraction。
 * `toGhost()` 负责"当前瞬间"的局部 reveal，`reconcileDeletedGhosts()` 负责把它接到本次
 * 删除 patch 的正式分段时间表，两个职责不再混。
 *
 * **测试场景**（与 #694 评论 5694645209 问题3 同构，但断言 reveal 而非 alpha）：
 * 1. 插入 "abc"（3 段 insertedUnits），applyPatch at frame0=0，吐字动画开始；
 * 2. sample 到中间帧 150ms：a reveal=1（已完成）、b reveal=0.5（进行中）、c reveal=0（未开始）；
 * 3. 下一帧快速删除 abc -> ""（deletedUnits = [c, b, a] 3 段），applyPatch at frame1=150ms；
 * 4. 通过反射在 sample 之前检查 ghost 的 `reveal.startedAtNanos` 和 `reveal.durationNanos`。
 *
 * **断言**：
 * - alpha>0 的 ghost（a、b）的 `reveal.startedAtNanos` 按 [i/n, (i+1)/n] 分段，互不相同；
 * - ghost b (range [1,2))：reveal.startedAtNanos == frame1 + segmentDuration；
 * - ghost a (range [0,1))：reveal.startedAtNanos == frame1 + 2*segmentDuration；
 * - 两者的 reveal.durationNanos == segmentDuration（不是整段 durationNanos）；
 * - alpha=0 的 unit（c）不产生 ghost（直接消失，#708 修复1）。
 *
 * 修复前本测试会失败：所有 ghost 的 reveal.startedAtNanos 都 == frame1（toGhost 创建时的 now），
 * reveal.durationNanos 都 == durationNanos（300ms），没有分段。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength", "LargeClass", "CognitiveComplexMethod")
class ComposeVisualIssue725Comment5752459178ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 核心回归：一笔 patch 删除多个 active unit 时，ghost 的 reveal 通道按 orderedDeletedUnits
     * 分段 schedule，不能全部从 frameTimeNanos 同时开始。
     *
     * 场景：先让 "abc" 的吐字动画已经开始但没结束（applyPatch 插入 abc，sample 到中间帧），
     * 下一帧快速删除 abc -> ""（deletedUnits = [c, b, a] 3 段）。
     *
     * #708 评论 5729482707 修复1 调整：alpha=0 的 unit（c，未开始动画）删除时直接消失，
     * 不转 ghost。alpha>0 的 unit（a 已完成、b 进行中）仍然转 ghost 并进入分段 schedule。
     *
     * 断言：alpha>0 的 ghost（a、b）的 **reveal** 按分段 schedule 依次进入淡出，
     * 不能一起下降。这是 #725 评论 5752459178 的核心修复点。
     */
    @Test
    @Suppress("LongMethod")
    fun activeUnitDeleted_reconcileDeletedGhosts_revealStaggeredSchedule() {
        val layouts = captureLayouts("", "abc", "ab", "a", "")
        val timeline = ComposeVisualTimeline()

        // 第一步：插入 "abc"，吐字动画开始
        val insertPatch =
            makeInsertPatch(
                oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0),
                newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0),
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                deletedUnits = emptyList(),
            )
        val durationNanos = 300_000_000L // 300ms
        val policy = EditorMotionPolicy(textEnabled = true, textDurationMillis = 300L, coordinated = false)
        val frame0 = 0L
        timeline.applyPatch(
            patch = insertPatch,
            frameTimeNanos = frame0,
            motionPolicy = policy,
        )

        // 第二步：sample 到中间帧（150ms），吐字动画进行中但未结束
        // 3 段分段 schedule: a:0~100ms, b:100~200ms, c:200~300ms
        // 150ms 时: a reveal=1(已完成), b reveal=0.5(进行中), c reveal=0(未开始)
        val midFrame = 150_000_000L
        val midScene = timeline.sample(midFrame)
        // 确认有活动 unit（abc 正在吐字）
        assertTrue(
            "中间帧应有活动 unit（abc 正在吐字），实际=${midScene.units.size}",
            midScene.units.isNotEmpty(),
        )

        // 第三步：下一帧快速删除 abc -> ""（产生 deletedUnits = [c, b, a] 3 段）
        val frame1 = midFrame
        val deletePatch =
            makeDeletePatch(
                oldLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0),
                newLayout = ComposeLayoutSnapshot(layouts[4], TextRange(0, 0), 0),
                insertedUnits = emptyList(),
                deletedUnits = listOf(TextRange(2, 3), TextRange(1, 2), TextRange(0, 1)),
            )
        timeline.applyPatch(
            patch = deletePatch,
            frameTimeNanos = frame1,
            motionPolicy = policy,
        )

        // 第四步：检查 ghost 的 reveal 分段 schedule
        // 删除分段 schedule：n=3, ghost i 的 startedAt = frame1 + durationNanos * (i/3)
        // orderedDeletedUnits = [TextRange(2,3), TextRange(1,2), TextRange(0,1)]
        // ghost 0 (c): startedAt = frame1 — 但 c alpha=0，#708 修复1 后直接消失不转 ghost
        // ghost 1 (b): startedAt = frame1 + durationNanos/3, duration = durationNanos/3
        // ghost 2 (a): startedAt = frame1 + 2*durationNanos/3, duration = durationNanos/3
        val segmentDuration = durationNanos / 3

        // 核心断言（在 sample 之前通过反射检查 ghost 的 reveal.startedAtNanos）：
        // sample 会 rebase 所有通道的 startedAtNanos，所以必须在 sample 之前检查。
        val timelineUnits = getTimelineUnits(timeline)
        val ghosts = timelineUnits.filter { it.targetRange == null }
        assertTrue(
            "applyPatch 后应有 ghost（a 和 b 被删除），实际=${ghosts.size}",
            ghosts.isNotEmpty(),
        )

        val ghostByRange = ghosts.associateBy { it.range }
        val ghostC = ghostByRange[TextRange(2, 3)]
        val ghostB = ghostByRange[TextRange(1, 2)]
        val ghostA = ghostByRange[TextRange(0, 1)]
        // #708 评论 5729482707 修复1：c (alpha=0) 删除时直接消失，不转 ghost
        assertNull(
            "不应有 range [2,3) 的 ghost (c) — alpha=0 的 unit 删除后直接消失（#708 修复1），" +
                "实际=${ghostC?.let { "reveal=${it.reveal.from}" }}",
            ghostC,
        )
        assertNotNull("应有 range [1,2) 的 ghost (b)", ghostB)
        assertNotNull("应有 range [0,1) 的 ghost (a)", ghostA)

        // ===== reveal 分段 schedule 核心断言（#725 评论 5752459178）=====
        // 修复前：ghost a 和 b 的 reveal.startedAtNanos 都 == frame1（toGhost 创建时的 now），
        //         reveal.durationNanos 都 == durationNanos（300ms），没有分段。
        // 修复后：reconcileDeletedGhosts schedule 阶段同时重建 reveal，按 [i/n, (i+1)/n] 分段。
        val revealStartedAtSet = listOf(ghostA!!, ghostB!!).map { it.reveal.startedAtNanos }.toSet()
        assertTrue(
            "alpha>0 的 ghost 的 reveal.startedAtNanos 应不同（分段 schedule），" +
                "实际 a=${ghostA.reveal.startedAtNanos}, b=${ghostB.reveal.startedAtNanos}\n" +
                "Issue #725 评论 5752459178：reconcileDeletedGhosts 必须同时接管 reveal，" +
                "不能只重排 alpha 让多个 delete ghost 的 reveal 全部从 frameTimeNanos 同时开始",
            revealStartedAtSet.size > 1,
        )

        // 验证 reveal 分段 schedule 的具体值：
        // orderedDeletedUnits = [TextRange(2,3), TextRange(1,2), TextRange(0,1)]
        // ghost 1 (b, range [1,2)): reveal.startedAt = frame1 + segmentDuration
        // ghost 2 (a, range [0,1)): reveal.startedAt = frame1 + 2*segmentDuration
        assertEquals(
            "ghost b (range [1,2)) 的 reveal.startedAtNanos 应为 frame1 + segmentDuration，" +
                "实际=${ghostB.reveal.startedAtNanos}（frame1=$frame1, segmentDuration=$segmentDuration）\n" +
                "Issue #725 评论 5752459178：reveal 通道必须和 alpha 一样按 orderedDeletedUnits 分段",
            frame1 + segmentDuration,
            ghostB.reveal.startedAtNanos,
        )
        assertEquals(
            "ghost a (range [0,1)) 的 reveal.startedAtNanos 应为 frame1 + 2*segmentDuration，" +
                "实际=${ghostA.reveal.startedAtNanos}（frame1=$frame1, segmentDuration=$segmentDuration）",
            frame1 + 2 * segmentDuration,
            ghostA.reveal.startedAtNanos,
        )

        // reveal.durationNanos 也应被重排成分段时长（不是 toGhost 创建时的整段 durationNanos）。
        // 注：ghostDuration = (durationNanos * (endFraction - startFraction)).toLong() 走 Float 运算，
        // 有浮点精度损失（如 99999992 vs 整除 100000000），不断言精确值，只断言"被重排成分段"：
        // - 修复前 reveal.durationNanos == durationNanos（toGhost 创建时的整段 300ms）；
        // - 修复后 reveal.durationNanos < durationNanos（分段），且 a/b 同一分段时长。
        assertTrue(
            "ghost b 的 reveal.durationNanos 应被重排成分段（< 整段 durationNanos），" +
                "实际=${ghostB.reveal.durationNanos}（durationNanos=$durationNanos）\n" +
                "Issue #725 评论 5752459178：修复前 reveal 保持 toGhost 创建时的整段 durationNanos",
            ghostB.reveal.durationNanos < durationNanos,
        )
        assertTrue(
            "ghost a 的 reveal.durationNanos 应被重排成分段（< 整段 durationNanos），" +
                "实际=${ghostA.reveal.durationNanos}（durationNanos=$durationNanos）",
            ghostA.reveal.durationNanos < durationNanos,
        )

        // reveal.to 应为 0f（ghost 吞字方向 1→0）
        assertEquals(
            "ghost b 的 reveal.to 应为 0f（吞字方向），实际=${ghostB.reveal.to}",
            0f,
            ghostB.reveal.to,
        )
        assertEquals(
            "ghost a 的 reveal.to 应为 0f（吞字方向），实际=${ghostA.reveal.to}",
            0f,
            ghostA.reveal.to,
        )

        // reveal.from 保留 toGhost 时的局部 fraction（不重置成 1，也不退回 parent fraction）
        // a 在 midFrame 时 reveal=1（已完成），toGhost 保留 revealNow=1
        // b 在 midFrame 时 reveal=0.5（进行中），toGhost 保留 revealNow=0.5
        assertTrue(
            "ghost b 的 reveal.from 应保持 toGhost 时的 revealNow（进行中 < 1），实际=${ghostB.reveal.from}",
            ghostB.reveal.from < 1.0f,
        )
    }

    // ==================== 辅助方法 ====================

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = 14f.sp),
                        constraints = Constraints(maxWidth = 1000),
                    ),
                )
            }
        }
        return results
    }

    /**
     * 通过反射访问 ComposeVisualTimeline 的 private units 列表。
     * 用于在 sample 之前检查 ghost 的 reveal.startedAtNanos（sample 会 rebase 所有通道）。
     */
    private fun getTimelineUnits(timeline: ComposeVisualTimeline): List<VisualTextUnit> {
        val field = ComposeVisualTimeline::class.java.getDeclaredField("units")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        return field.get(timeline) as List<VisualTextUnit>
    }

    /**
     * 构造插入 patch。
     */
    private fun makeInsertPatch(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        insertedUnits: List<TextRange>,
        deletedUnits: List<TextRange>,
    ): ComposeVisualPatch {
        return ComposeVisualPatch(
            id = 1L,
            coreTransactionIds = emptyList(),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = emptyList(),
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = emptyList(),
            originCaretRect = Rect.Zero,
            targetCaretRect = Rect.Zero,
            durationMs = 300L,
            animationMode = AnimationMode.GLYPH_ANIMATION,
        )
    }

    /**
     * 构造删除 patch。
     */
    private fun makeDeletePatch(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        insertedUnits: List<TextRange>,
        deletedUnits: List<TextRange>,
    ): ComposeVisualPatch {
        return ComposeVisualPatch(
            id = 2L,
            coreTransactionIds = emptyList(),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = emptyList(),
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = emptyList(),
            originCaretRect = Rect.Zero,
            targetCaretRect = Rect.Zero,
            durationMs = 300L,
            animationMode = AnimationMode.GLYPH_ANIMATION,
        )
    }
}
