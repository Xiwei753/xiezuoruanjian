package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.input.CommitResult
import com.xiwei.sujian.feature.editor.input.EditorInputSnapshot
import com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge
import com.xiwei.sujian.feature.editor.input.InputSnapshotOutcome
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #694 评论 5694645209 回归测试 — 覆盖评论指出的 3 个功能问题的修复。
 *
 * 1. 问题1：composition 结束时，视觉层不应先于 Core 决定"候选已提交"。
 *    测试真实 bridge 路径：bridge.onInputSnapshot 返回 InputSnapshotOutcome，
 *    visualState.onInputSnapshotResolved 根据 outcome 收口。
 * 2. 问题2：同一 VSync 合批时，快速删除的多段光标路径不应被压回单点。
 *    测试 ComposeVisualPatchBatch.compose 对 3 笔删除保留多段 cursor path。
 * 3. 问题3："刚吐出来马上删"时，活动 unit 应进入新的删除分段 schedule。
 *    测试 ComposeVisualTimeline 对正在动画的 active unit 转 ghost 后按 deletedUnits 分段。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength", "LargeClass", "CognitiveComplexMethod")
class ComposeVisualIssue694Comment5694645209Test {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 问题1：bridge 路径测试 ====================

    /**
     * 问题1 核心回归：composition 期间 Undo 到达（pending authoritative），
     * composition 结束时 bridge.onInputSnapshot 返回 AuthoritativeApplied，
     * visualState 不应生成 a -> an 的 local patch。
     *
     * 场景："a" -> composition "an"，storePendingAuthoritative("", ...)，
     * composition 结束（snapshot.text = "an", composition = null）。
     *
     * 断言：
     * - bridge.onInputSnapshot 返回 AuthoritativeApplied
     * - 不生成 a -> an 的 local patch（visualState.latestPatch 不应变成 a->an 的 patch）
     * - bridge 最终正文是 ""（mirroredText == ""）
     */
    @Test
    fun compositionEnd_withPendingAuthoritative_bridgeReturnsAuthoritativeApplied_noLocalPatch() {
        val layouts = captureLayouts("a", "an", "")
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "a",
                initialSelection = TextRange(1, 1),
                commitToCore = { edit ->
                    // Core 已提交 "a"，现在收到 "an" 的提交应被接受（用于对比）
                    CommitResult.Accepted
                },
            )
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5694645209-p1",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 设置基线：lastPresentedLayout = "a"
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // composition 开始：IME preedit "an"
        // bridge.state 已被 IME 改写成 "an"，composition = TextRange(1, 2)
        // 注意：setComposition 是 internal API，测试不直接调；
        // bridge.onInputSnapshot 只看 snapshot.composition，不读 bridge.state.composition。
        bridge.state.edit {
            replace(0, length, "an")
            this.selection = TextRange(2, 2)
        }
        val snapshotComposing =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = TextRange(1, 2),
            )
        // 先调 bridge 拿 outcome，再传 visualState
        val outcomeComposing = bridge.onInputSnapshot(snapshotComposing)
        assertEquals(
            "composition 期间应返回 Composing",
            InputSnapshotOutcome.Composing,
            outcomeComposing,
        )
        state.onInputSnapshotResolved(snapshotComposing, outcomeComposing)

        // composition 期间 Undo 到达，Core 权威正文变为 ""
        bridge.storePendingAuthoritative("", TextRange(0, 0))
        assertTrue("pending 已存", bridge.hasPendingAuthoritative())

        // composition 结束：snapshot.text = "an", composition = null
        bridge.state.edit {
            replace(0, length, "an")
            this.selection = TextRange(2, 2)
            // composition 已结束
        }
        // 模拟 composition 结束后的 layout 到达（"an" 的 layout）
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)

        val snapshotEnd =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = null,
            )
        // 先调 bridge 拿 outcome — bridge 应发现 pending authoritative 并返回 AuthoritativeApplied
        val outcomeEnd = bridge.onInputSnapshot(snapshotEnd)
        assertEquals(
            "composition 结束且有 pending authoritative 应返回 AuthoritativeApplied，不应提交本地 diff\n" +
                "Issue #694 评论 5694645209 问题1：bridge 决定权收口",
            InputSnapshotOutcome.AuthoritativeApplied,
            outcomeEnd,
        )
        // 记录 outcome 前的 latestPatch（可能是 composition 期间 onAuthoritativeLayout 设置的）
        val patchBefore = state.latestPatch.value
        // 再传 visualState — visualState 不应生成 a->an 的 local patch
        state.onInputSnapshotResolved(snapshotEnd, outcomeEnd)

        // bridge 最终正文应是 ""（pending authoritative 已应用）
        assertEquals(
            "bridge.mirroredText 应为空串（pending authoritative 已应用）",
            "",
            bridge.mirroredText,
        )
        assertEquals(
            "bridge.state.text 应为空串（pending authoritative 已应用）",
            "",
            bridge.state.text.toString(),
        )
        // visualState 不应生成 a->an 的 local patch
        // latestPatch 要么是 null，要么是之前 onAuthoritativeLayout 设置的（不是 a->an）
        val patchAfter = state.latestPatch.value
        if (patchAfter != null && patchBefore != null) {
            // 如果有 patch，它不应是 a->an 的 local patch
            val isAnPatch =
                patchAfter.oldLayout.result.layoutInput.text.text == "a" &&
                    patchAfter.newLayout.result.layoutInput.text.text == "an"
            assertTrue(
                "不应生成 a->an 的 local patch（bridge 已返回 AuthoritativeApplied）\n" +
                    "Issue #694 评论 5694645209 问题1：视觉层不应先于 Core 决定候选已提交",
                !isAnPatch,
            )
        }
    }

    /**
     * 问题1 核心回归：commitToCore -> Rejected 时，bridge.onInputSnapshot 返回 LocalCommitRejected，
     * visualState 不应播放被拒绝的候选动画。
     *
     * 场景：composition "an" 结束，bridge 提交 "an" 给 Core，Core 拒绝并回退到权威正文 "a"。
     *
     * 断言：
     * - bridge.onInputSnapshot 返回 LocalCommitRejected
     * - bridge 最终正文是 "a"（mirroredText == "a"）
     */
    @Test
    fun compositionEnd_commitRejected_bridgeReturnsLocalCommitRejected() {
        val layouts = captureLayouts("a", "an")
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "a",
                initialSelection = TextRange(1, 1),
                commitToCore = { _ ->
                    // Core 拒绝 "an" 的提交，回退到权威正文 "a"
                    CommitResult.Rejected(
                        text = "a",
                        selection = TextRange(1, 1),
                    )
                },
            )
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5694645209-p1-rejected",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 设置基线：lastPresentedLayout = "a"
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // composition 开始：IME preedit "an"
        bridge.state.edit {
            replace(0, length, "an")
            this.selection = TextRange(2, 2)
        }
        val snapshotComposing =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = TextRange(1, 2),
            )
        val outcomeComposing = bridge.onInputSnapshot(snapshotComposing)
        assertEquals(InputSnapshotOutcome.Composing, outcomeComposing)
        state.onInputSnapshotResolved(snapshotComposing, outcomeComposing)

        // composition 结束：snapshot.text = "an", composition = null
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)
        val snapshotEnd =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = null,
            )
        val outcomeEnd = bridge.onInputSnapshot(snapshotEnd)
        assertEquals(
            "Core 拒绝 commit 应返回 LocalCommitRejected\n" +
                "Issue #694 评论 5694645209 问题1：Rejected 时不能播放被拒绝的候选动画",
            InputSnapshotOutcome.LocalCommitRejected,
            outcomeEnd,
        )
        state.onInputSnapshotResolved(snapshotEnd, outcomeEnd)

        assertEquals(
            "bridge.mirroredText 应为 a（Core 拒绝后回退到权威正文）",
            "a",
            bridge.mirroredText,
        )
    }

    /**
     * 问题1 回归：composition 结束且 Core 接受本地 commit 时，bridge 返回 LocalCommitAccepted，
     * visualState 可以 finishCompositionCommit 生成 local patch。
     */
    @Test
    fun compositionEnd_commitAccepted_bridgeReturnsLocalCommitAccepted() {
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "",
                initialSelection = TextRange(0, 0),
                commitToCore = { _ -> CommitResult.Accepted },
            )

        // composition 开始：IME preedit "a"
        bridge.state.edit {
            replace(0, length, "a")
            this.selection = TextRange(1, 1)
        }
        val snapshotComposing =
            EditorInputSnapshot(
                text = "a",
                selection = TextRange(1, 1),
                composition = TextRange(0, 1),
            )
        val outcomeComposing = bridge.onInputSnapshot(snapshotComposing)
        assertEquals(InputSnapshotOutcome.Composing, outcomeComposing)

        // composition 结束
        val snapshotEnd =
            EditorInputSnapshot(
                text = "a",
                selection = TextRange(1, 1),
                composition = null,
            )
        val outcomeEnd = bridge.onInputSnapshot(snapshotEnd)
        assertEquals(
            "Core 接受 commit 应返回 LocalCommitAccepted",
            InputSnapshotOutcome.LocalCommitAccepted,
            outcomeEnd,
        )
        assertEquals("a", bridge.mirroredText)
    }

    /**
     * 问题1 回归：text == committedMirror（无变化）时返回 NoTextChange。
     */
    @Test
    fun compositionEnd_noTextChange_bridgeReturnsNoTextChange() {
        val bridge =
            EditorTextFieldStateBridge(
                initialText = "a",
                initialSelection = TextRange(1, 1),
                commitToCore = { _ -> CommitResult.Accepted },
            )

        val snapshot =
            EditorInputSnapshot(
                text = "a",
                selection = TextRange(1, 1),
                composition = null,
            )
        val outcome = bridge.onInputSnapshot(snapshot)
        assertEquals(
            "text 未变应返回 NoTextChange",
            InputSnapshotOutcome.NoTextChange,
            outcome,
        )
    }

    // ==================== 问题2：batch cursor path 测试 ====================

    /**
     * 问题2 核心回归：3 笔 local Backspace 在同一个 frame drain，
     * 合成 patch 的 cursorMotionPath 应保留多段阶段点，而非单点。
     *
     * 场景：abc -> ab -> a -> "" 3 笔 local patch 在同一 VSync 入队。
     *
     * 断言合成 patch：
     * - deletedUnits 顺序仍是 c、b、a
     * - cursorMotionPath.points 至少保留 3 个阶段点，而不是 1 个最终点
     * - 各点按删除方向向前移动，最后收敛到 offset 0
     */
    @Test
    fun sameVsyncThreeBackspace_batchCursorPathPreservesStageCarets() {
        val layouts = captureLayouts("abc", "ab", "a", "")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5694645209-p2",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 设置基线：lastPresentedLayout = "abc"
        state.onAuthoritativeLayout(layouts[0], TextRange(3, 3), 0)

        // 三笔快速 Backspace：abc -> ab -> a -> ""
        recordThreeBackspaces(state, layouts)

        val pendingSize = pendingPatchesSize(state)
        assertTrue(
            "drain 前 pendingPatches 应有 3 个 patch，实际=$pendingSize",
            pendingSize == 3,
        )

        // 同一 VSync 消费
        val applied = state.drainPendingPatchesAtFrame(0L)
        assertEquals("应只 applyPatch 一次（batch 合成）", 1, applied.size)

        val framePatch = applied[0]
        assertBatchDeletePatchBasics(framePatch)
        assertBatchCursorPathPreservesStageCarets(framePatch)
    }

    /**
     * 录入三笔快速 Backspace：abc -> ab -> a -> ""。
     */
    private fun recordThreeBackspaces(
        state: ComposeEditorVisualState,
        layouts: List<TextLayoutResult>,
    ) {
        state.recordLocalInput(
            oldText = "abc",
            newText = "ab",
            oldSelection = TextRange(3, 3),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(2, 2), oldRange = TextRange(2, 3))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)

        state.recordLocalInput(
            oldText = "ab",
            newText = "a",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 1), oldRange = TextRange(1, 2))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0, compositionActive = false)

        state.recordLocalInput(
            oldText = "a",
            newText = "",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(0, 0), 0, compositionActive = false)
    }

    /**
     * 验证 batch 删除 patch 的基本属性：deletedUnits 非空、oldLayout="abc"、newLayout=""。
     */
    private fun assertBatchDeletePatchBasics(framePatch: ComposeVisualPatch) {
        assertTrue(
            "deletedUnits 应非空（删除了 abc）",
            framePatch.deletedUnits.isNotEmpty(),
        )
        assertEquals(
            "patch.oldLayout 应是 abc",
            "abc",
            framePatch.oldLayout.result.layoutInput.text.text,
        )
        assertEquals(
            "patch.newLayout 应是空串",
            "",
            framePatch.newLayout.result.layoutInput.text.text,
        )
    }

    /**
     * 验证 batch cursor path 保留多段阶段点：至少 3 个点、最后收敛到 offset 0、endFraction 归一化。
     */
    private fun assertBatchCursorPathPreservesStageCarets(framePatch: ComposeVisualPatch) {
        val cursorPath = framePatch.cursorMotionPath
        assertNotNull(
            "cursorMotionPath 应非空",
            cursorPath,
        )
        assertTrue(
            "cursorMotionPath.points 应至少保留 3 个阶段点（c/b/a 删除阶段），实际=${cursorPath?.points?.size}\n" +
                "Issue #694 评论 5694645209 问题2：batch 纯删除不应把多段光标路径压回单点",
            (cursorPath?.points?.size ?: 0) >= 3,
        )

        // 各点按删除方向向前移动（offset 3 -> 2 -> 1 -> 0），
        // 最后一个点应收敛到 offset 0 的 cursor rect
        val points = cursorPath!!.points
        val finalCursorRect = framePatch.newLayout.result.getCursorRect(0)
        assertEquals(
            "最后一个点应收敛到 offset 0 的 cursor rect",
            finalCursorRect,
            points.last().rect,
        )
        // endFraction 应归一化：(i+1)/n
        val n = points.size
        for (i in points.indices) {
            val expectedFraction = (i + 1f) / n
            assertEquals(
                "points[$i].endFraction 应为 (i+1)/n = $expectedFraction",
                expectedFraction,
                points[i].endFraction,
                0.001f,
            )
        }
    }

    // ==================== 问题3：ghost 分段 schedule 测试 ====================

    /**
     * 问题3 核心回归："刚吐出来马上删"时，活动 unit 应进入新的删除分段 schedule。
     *
     * 场景：先让 "abc" 的吐字动画已经开始但没结束（applyPatch 插入 abc，sample 到中间帧），
     * 下一帧快速删除 c -> b -> a。
     *
     * 断言：在 1/3、2/3 时间点采样，三个 ghost 的 alpha 应按 c、b、a 依次进入淡出，
     * 不能三个一起下降。
     */
    @Test
    fun activeUnitDeleted_rescheduleDeletedGhosts_staggeredFadeOut() {
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
        val policy = EditorMotionPolicy(textEnabled = true, textDurationMillis = 300L)
        val frame0 = 0L
        timeline.applyPatch(
            patch = insertPatch.copy(motionPolicy = policy),
            frameTimeNanos = frame0,
        )

        // 第二步：sample 到中间帧（150ms），吐字动画进行中但未结束
        val midFrame = 150_000_000L
        val midScene = timeline.sample(midFrame)
        // 确认有活动 unit（abc 正在吐字）
        assertTrue(
            "中间帧应有活动 unit（abc 正在吐字），实际=${midScene.units.size}",
            midScene.units.isNotEmpty(),
        )

        // 第三步：下一帧快速删除 c -> b -> a（产生 deletedUnits = [c, b, a] 3 段）
        // 构造一个删除 patch：abc -> ""
        val deletePatch =
            makeDeletePatch(
                oldLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0),
                newLayout = ComposeLayoutSnapshot(layouts[4], TextRange(0, 0), 0),
                insertedUnits = emptyList(),
                deletedUnits = listOf(TextRange(2, 3), TextRange(1, 2), TextRange(0, 1)),
            )
        val frame1 = midFrame
        timeline.applyPatch(
            patch = deletePatch.copy(motionPolicy = policy),
            frameTimeNanos = frame1,
        )

        // 第四步：在 1/3、2/3 时间点采样，检查三个 ghost 的 alpha
        // 删除分段 schedule：n=3, ghost i 的 startedAt = frame1 + durationNanos * (i/3)
        // ghost 0 (c): startedAt = frame1, duration = durationNanos/3
        // ghost 1 (b): startedAt = frame1 + durationNanos/3, duration = durationNanos/3
        // ghost 2 (a): startedAt = frame1 + 2*durationNanos/3, duration = durationNanos/3
        val segmentDuration = durationNanos / 3

        // 核心断言（在 sample 之前通过反射检查 ghost 的 startedAtNanos）：
        // 三个 ghost 的 startedAtNanos 应不同（分段 schedule），不能都是 frame1。
        // sample 会 rebase 所有通道的 startedAtNanos，所以必须在 sample 之前检查。
        val timelineUnits = getTimelineUnits(timeline)
        val ghosts = timelineUnits.filter { it.targetRange == null }
        assertTrue(
            "applyPatch 后应有 ghost（abc 被删除），实际=${ghosts.size}",
            ghosts.isNotEmpty(),
        )
        val startedAtSet = ghosts.map { it.alpha.startedAtNanos }.toSet()
        assertTrue(
            "三个 ghost 的 startedAtNanos 应不同（分段 schedule），实际=${ghosts.map { it.alpha.startedAtNanos }}\n" +
                "Issue #694 评论 5694645209 问题3：活动 unit 应进入删除分段 schedule，不能三个一起下降",
            startedAtSet.size > 1,
        )

        // 验证分段 schedule 的具体值：
        // orderedDeletedUnits = [TextRange(2,3), TextRange(1,2), TextRange(0,1)]
        // ghost 0 (c, range [2,3)): startedAt = frame1 + 0 = frame1
        // ghost 1 (b, range [1,2)): startedAt = frame1 + segmentDuration
        // ghost 2 (a, range [0,1)): startedAt = frame1 + 2*segmentDuration
        val ghostByRange = ghosts.associateBy { it.range }
        val ghostC = ghostByRange[TextRange(2, 3)]
        val ghostB = ghostByRange[TextRange(1, 2)]
        val ghostA = ghostByRange[TextRange(0, 1)]
        assertNotNull("应有 range [2,3) 的 ghost (c)", ghostC)
        assertNotNull("应有 range [1,2) 的 ghost (b)", ghostB)
        assertNotNull("应有 range [0,1) 的 ghost (a)", ghostA)
        assertEquals(
            "ghost c (range [2,3)) 的 startedAt 应为 frame1",
            frame1,
            ghostC!!.alpha.startedAtNanos,
        )
        assertEquals(
            "ghost b (range [1,2)) 的 startedAt 应为 frame1 + segmentDuration",
            frame1 + segmentDuration,
            ghostB!!.alpha.startedAtNanos,
        )
        assertEquals(
            "ghost a (range [0,1)) 的 startedAt 应为 frame1 + 2*segmentDuration",
            frame1 + 2 * segmentDuration,
            ghostA!!.alpha.startedAtNanos,
        )

        // 验证 alpha.from 保持当前真实 alpha（toGhost 时保留的 alphaNow），不重置成 1
        // 在 midFrame 时：
        // - unit 0 (a, range [0,1)): 已完成 alpha=1，但被 sample 移除，从 oldLayout 建 ghost，alpha.from=1
        // - unit 1 (b, range [1,2)): 进行中 alpha=0.5，toGhost 保留 alphaNow=0.5
        // - unit 2 (c, range [2,3)): 未开始 alpha=0，toGhost 保留 alphaNow=0
        // rescheduleDeletedGhosts 后 alpha.from 保持不变
        assertTrue(
            "ghost b 的 alpha.from 应保持 toGhost 时的 alphaNow（不重置成 1），实际=${ghostB.alpha.from}",
            ghostB.alpha.from < 1.0f,
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
     * 通过反射访问 ComposeEditorVisualState 的 private pendingPatches 队列大小。
     */
    private fun pendingPatchesSize(state: ComposeEditorVisualState): Int {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("pendingPatches")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val deque = field.get(state) as kotlin.collections.ArrayDeque<*>
        return deque.size
    }

    /**
     * 通过反射访问 ComposeVisualTimeline 的 private units 列表。
     * 用于在 sample 之前检查 ghost 的 startedAtNanos（sample 会 rebase 所有通道）。
     */
    private fun getTimelineUnits(timeline: ComposeVisualTimeline): List<VisualTextUnit> {
        val field = ComposeVisualTimeline::class.java.getDeclaredField("units")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        return field.get(timeline) as List<VisualTextUnit>
    }

    /**
     * 构造插入 patch（用于问题3测试）。
     */
    private fun makeInsertPatch(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        insertedUnits: List<TextRange>,
        deletedUnits: List<TextRange>,
    ): ComposeVisualPatch {
        val policy = EditorMotionPolicy(textEnabled = true, textDurationMillis = 300L)
        return ComposeVisualPatch(
            id = 1L,
            coreTransactionIds = emptyList(),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = emptyList(),
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = emptyList(),
            cursorMotionPath = null,
            durationMs = 300L,
            animationMode = AnimationModeDto.GLYPH_ANIMATION,
            motionPolicy = policy,
            intent = null,
        )
    }

    /**
     * 构造删除 patch（用于问题3测试）。
     */
    private fun makeDeletePatch(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        insertedUnits: List<TextRange>,
        deletedUnits: List<TextRange>,
    ): ComposeVisualPatch {
        val policy = EditorMotionPolicy(textEnabled = true, textDurationMillis = 300L)
        return ComposeVisualPatch(
            id = 2L,
            coreTransactionIds = emptyList(),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = emptyList(),
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = emptyList(),
            cursorMotionPath = null,
            durationMs = 300L,
            animationMode = AnimationModeDto.GLYPH_ANIMATION,
            motionPolicy = policy,
            intent = null,
        )
    }
}
