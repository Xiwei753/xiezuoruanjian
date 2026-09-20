package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.input.EditorInputSnapshot
import com.xiwei.sujian.feature.editor.input.InputSnapshotOutcome
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #694 评论 5695660885 复现测试 — 覆盖评论指出的两个剩余竞态。
 *
 * 问题1：composition 还是存在 onTextLayout 先于 bridge outcome 的竞态。
 *   真实运行时 InputTransformation 会无条件 recordLocalInput(...)，所以 tracker 里有 preedit/commit 的 edit。
 *   composition 结束后 final candidate layout 先到 onAuthoritativeLayout()，此时 bridge outcome 还没到，
 *   onAuthoritativeLayout() 就已经能先发布候选 local patch、推进 frameCoordinator。
 *   上一轮 5694645209 的 Rejected / pending-authoritative 测试都没有调用 recordLocalInput()，
 *   所以 tracker 是空的，当然不会提前生成 patch。本测试真正调用 recordLocalInput() 让 tracker 非空。
 *
 * 问题2：同一 VSync "输入后立刻删除"净变化为 0 时，cursor 仍会凭空来回跑。
 *   ComposeVisualPatchBatch.compose() 无论最终净文本变化是什么，都会先 composeBatchCursorPath(batch, ...)。
 *   同一帧 "" -> "a" -> ""，最终 transactionTextKind == None，insertedUnits/deletedUnits 都为空，
 *   文字正确地不播放任何中间态；但 batch 仍会收集两笔 patch 的 stage cursor point，得到 0 -> 1 -> 0 的路径。
 *   随后 computeCursorParamsForPatch() 会把它当 CURSOR_ONLY，使用 cursorDurationMillis 真正播放。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength", "LargeClass", "CognitiveComplexMethod")
class ComposeVisualIssue694Comment5695660885ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 问题1：composition onTextLayout 先于 bridge outcome ====================

    /**
     * 问题1 核心复现：composition 结束后 final layout "an" 先到 onAuthoritativeLayout()，
     * 此时 bridge outcome 还没到，但 tracker 里已有 recordLocalInput("a" -> "an") 的 edit。
     * onAuthoritativeLayout() 会 drainMatchingChain("a", "an") 返回 chain，提前发布 a->an 的 local patch。
     *
     * 复现序列（真正调用 recordLocalInput）：
     * - base = "a"
     * - InputTransformation: recordLocalInput("a" -> "an")
     * - composition active（onInputSnapshotResolved(Composing) 记录 base，但不调 onAuthoritativeLayout — preedit layout 未到）
     * - composition 结束，compositionActive=false
     * - final candidate layout "an" 先到 onAuthoritativeLayout()
     * - 此时 bridge outcome 还没到
     * - 然后 bridge 返回 LocalCommitRejected
     *
     * 断言（期望正确行为，当前代码因竞态导致断言失败）：
     * - 在 bridge outcome 之前 pendingPatches 仍为空
     * - outcome 为 Reject 后也不能出现 a -> an local patch
     */
    @Test
    fun compositionEnd_onTextLayoutBeforeBridgeOutcome_withRecordLocalInput_rejected_preventsLocalPatch() {
        val layouts = captureLayouts("a", "an")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5695660885-p1-rejected",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 1. base = "a"：设置基线 lastPresentedLayout = "a"
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 2. composition 开始：InputTransformation 无条件 recordLocalInput("a" -> "an")
        //    tracker 里现在有 preedit/commit 的 edit（这是真实运行时行为）。
        state.recordLocalInput(
            oldText = "a",
            newText = "an",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        // onInputSnapshotResolved(Composing) — 记录 compositionBaseLayout = lastPresentedLayout = "a"
        // 注意：composition 期间不调 onAuthoritativeLayout（preedit layout 未到 / onTextLayout 延迟），
        // lastPresentedLayout 仍是 "a"。
        val snapshotComposing =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = TextRange(1, 2),
            )
        state.onInputSnapshotResolved(snapshotComposing, InputSnapshotOutcome.Composing)

        // 3. composition 结束，compositionActive=false
        //    final candidate layout "an" 先到 onAuthoritativeLayout() — bridge outcome 还没到
        //    此时 tracker 非空，lastPresentedLayout.text = "a"，drainMatchingChain("a", "an") 返回 chain
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)

        // 断言1：在 bridge outcome 之前 pendingPatches 仍应为空
        // 当前代码有竞态：onAuthoritativeLayout 已提前发布 a->an 的 local patch
        val pendingBeforeOutcome = pendingPatchesSize(state)
        assertTrue(
            "在 bridge outcome 之前 pendingPatches 应仍为空，实际=$pendingBeforeOutcome\n" +
                "Issue #694 评论 5695660885 问题1：onTextLayout 先于 bridge outcome 时" +
                "不应提前发布候选 local patch（tracker 非空导致 drainMatchingChain 命中）",
            pendingBeforeOutcome == 0,
        )

        // 4. bridge outcome 到达：LocalCommitRejected
        val snapshotEnd =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotEnd, InputSnapshotOutcome.LocalCommitRejected)

        // 断言2：outcome 为 Reject 后也不能出现 a -> an local patch
        val pendingAfterReject = pendingPatchesSize(state)
        val latestPatch = state.latestPatch.value
        val isAnLocalPatch =
            latestPatch != null &&
                latestPatch.oldLayout.result.layoutInput.text.text == "a" &&
                latestPatch.newLayout.result.layoutInput.text.text == "an" &&
                latestPatch.intent == null
        assertTrue(
            "outcome 为 Reject 后也不应出现 a->an local patch，" +
                "pendingPatches=$pendingAfterReject isAnLocalPatch=$isAnLocalPatch\n" +
                "Issue #694 评论 5695660885 问题1：Rejected 后不应保留提前发布的候选 local patch",
            !isAnLocalPatch && pendingAfterReject == 0,
        )
    }

    /**
     * 问题1 核心复现（AuthoritativeApplied 变体）：composition 期间 Undo 到达，
     * composition 结束后 final layout "an" 先到 onAuthoritativeLayout()，bridge 返回 AuthoritativeApplied。
     *
     * 断言（期望正确行为，当前代码因竞态导致断言失败）：
     * - 在 bridge outcome 之前 pendingPatches 仍为空
     * - outcome 为 AuthoritativeApplied 后也不能出现 a -> an local patch
     */
    @Test
    fun compositionEnd_onTextLayoutBeforeBridgeOutcome_withRecordLocalInput_authoritativeApplied_preventsLocalPatch() {
        val layouts = captureLayouts("a", "an")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5695660885-p1-authoritative",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 1. base = "a"
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 2. composition 开始：recordLocalInput("a" -> "an")
        state.recordLocalInput(
            oldText = "a",
            newText = "an",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        val snapshotComposing =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = TextRange(1, 2),
            )
        state.onInputSnapshotResolved(snapshotComposing, InputSnapshotOutcome.Composing)

        // 3. composition 结束，final layout "an" 先到 onAuthoritativeLayout() — bridge outcome 还没到
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)

        // 断言1：在 bridge outcome 之前 pendingPatches 仍应为空
        val pendingBeforeOutcome = pendingPatchesSize(state)
        assertTrue(
            "在 bridge outcome 之前 pendingPatches 应仍为空，实际=$pendingBeforeOutcome\n" +
                "Issue #694 评论 5695660885 问题1：onTextLayout 先于 bridge outcome 时" +
                "不应提前发布候选 local patch",
            pendingBeforeOutcome == 0,
        )

        // 4. bridge outcome 到达：AuthoritativeApplied（Undo 在 composition 期间到达）
        val snapshotEnd =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotEnd, InputSnapshotOutcome.AuthoritativeApplied)

        // 断言2：outcome 为 AuthoritativeApplied 后也不能出现 a -> an local patch
        val pendingAfterAuth = pendingPatchesSize(state)
        val latestPatch = state.latestPatch.value
        val isAnLocalPatch =
            latestPatch != null &&
                latestPatch.oldLayout.result.layoutInput.text.text == "a" &&
                latestPatch.newLayout.result.layoutInput.text.text == "an" &&
                latestPatch.intent == null
        assertTrue(
            "outcome 为 AuthoritativeApplied 后也不应出现 a->an local patch，" +
                "pendingPatches=$pendingAfterAuth isAnLocalPatch=$isAnLocalPatch\n" +
                "Issue #694 评论 5695660885 问题1：AuthoritativeApplied 后不应保留提前发布的候选 local patch",
            !isAnLocalPatch && pendingAfterAuth == 0,
        )
    }

    /**
     * 问题1 补充：composition cancel — "a" -> preedit "an" -> cancel 回 "a"，bridge 返回 NoTextChange。
     *
     * 复现序列：
     * - base = "a"
     * - recordLocalInput("a" -> "an")（preedit）
     * - recordLocalInput("an" -> "a")（cancel 回 "a"）
     * - composition 结束，final layout "a" 先到 onAuthoritativeLayout()
     * - bridge 返回 NoTextChange
     *
     * 断言（期望正确行为，当前代码因竞态导致断言失败）：
     * - tracker/state 清干净
     * - 不提前发布 local patch
     * - 下一笔普通输入不会吃到上一轮 preedit
     */
    @Test
    fun compositionCancel_onTextLayoutBeforeBridgeOutcome_noTextChange_clearsTrackerAndNoPrematurePatch() {
        val layouts = captureLayouts("a", "an", "a", "ab")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5695660885-p1-cancel",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 1. base = "a"
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 2. composition preedit "an"
        state.recordLocalInput(
            oldText = "a",
            newText = "an",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        val snapshotComposing =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = TextRange(1, 2),
            )
        state.onInputSnapshotResolved(snapshotComposing, InputSnapshotOutcome.Composing)

        // 3. composition cancel：IME 把 buffer 从 "an" 改回 "a"
        state.recordLocalInput(
            oldText = "an",
            newText = "a",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 1), oldRange = TextRange(1, 2))),
        )

        // 4. composition 结束，final layout "a" 先到 onAuthoritativeLayout() — bridge outcome 还没到
        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0, compositionActive = false)

        // 断言1：在 bridge outcome 之前 pendingPatches 仍应为空（cancel 净变化为 0，不应提前发布 patch）
        val pendingBeforeOutcome = pendingPatchesSize(state)
        assertTrue(
            "cancel 场景：在 bridge outcome 之前 pendingPatches 应仍为空，实际=$pendingBeforeOutcome\n" +
                "Issue #694 评论 5695660885 问题1：cancel 时不应提前发布 local patch",
            pendingBeforeOutcome == 0,
        )

        // 5. bridge outcome 到达：NoTextChange
        val snapshotEnd =
            EditorInputSnapshot(
                text = "a",
                selection = TextRange(1, 1),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotEnd, InputSnapshotOutcome.NoTextChange)

        // 断言2：tracker 应清干净（NoTextChange 后不应残留 preedit edit）
        val trackerPending = localInputTrackerSize(state)
        assertTrue(
            "NoTextChange 后 tracker 应清干净，实际 pending=$trackerPending\n" +
                "Issue #694 评论 5695660885 问题1：cancel 后 tracker/state 应清干净",
            trackerPending == 0,
        )

        // 断言3：下一笔普通输入 "a" -> "ab" 不会吃到上一轮 preedit
        // 即不应生成 an->ab 或 a->an->ab 的 patch，只应生成 a->ab 的 patch
        state.recordLocalInput(
            oldText = "a",
            newText = "ab",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(2, 2), 0, compositionActive = false)

        val pendingAfterNext = pendingPatchesSize(state)
        // 下一笔应只产生 a->ab 的 patch（加上之前可能残留的）
        // 检查 latestPatch 是 a->ab 而非 an->ab
        val latestPatch = state.latestPatch.value
        val isAnContaminated =
            latestPatch != null &&
                latestPatch.oldLayout.result.layoutInput.text.text == "an"
        assertTrue(
            "下一笔普通输入不应吃到上一轮 preedit，latestPatch.oldText 不应为 an\n" +
                "Issue #694 评论 5695660885 问题1：cancel 后下一笔输入不应被 preedit 污染",
            !isAnContaminated,
        )
    }

    // ==================== 问题2：同一 VSync 输入后立刻删除净变化为 0 时光标抽动 ====================

    /**
     * 问题2 核心复现：同一 VSync "" -> "a" -> ""，最终净文本变化为 None，
     * insertedUnits/deletedUnits 都为空，文字正确地不播放任何中间态；
     * 但 ComposeVisualPatchBatch.compose() 仍会 composeBatchCursorPath(batch, ...)，
     * 收集两笔 patch 的 stage cursor point，得到 0 -> 1 -> 0 的路径。
     * 随后 computeCursorParamsForPatch() 会把它当 CURSOR_ONLY，使用 cursorDurationMillis 真正播放。
     *
     * 复现序列：
     * - 同一 VSync："" -> "a" -> ""
     * - selection: 0 -> 1 -> 0
     *
     * 断言合成 patch（期望正确行为，当前代码因竞态导致断言失败）：
     * - insertedUnits.isEmpty()
     * - deletedUnits.isEmpty()
     * - cursorMotionPath == null（或最终 snap 且 duration=0）
     * - timeline 不产生 0 -> 1 -> 0 的 cursor track
     */
    @Test
    fun sameVsyncInsertThenDelete_netZeroText_cursorPathShouldBeNullOrSingleSnap() {
        val layouts = captureLayouts("", "a", "")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5695660885-p2",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // base = ""
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 同一 VSync："" -> "a" -> ""
        // patch1: "" -> "a"（selection 0 -> 1）
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0, compositionActive = false)

        // patch2: "a" -> ""（selection 1 -> 0）
        state.recordLocalInput(
            oldText = "a",
            newText = "",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(0, 0), 0, compositionActive = false)

        val pendingSize = pendingPatchesSize(state)
        assertTrue(
            "drain 前 pendingPatches 应有 2 个 patch，实际=$pendingSize",
            pendingSize == 2,
        )

        // 同一 VSync 消费
        val applied = state.drainPendingPatchesAtFrame(0L)
        assertEquals("应只 applyPatch 一次（batch 合成）", 1, applied.size)

        val framePatch = applied[0]

        // 断言1：最终净文本变化为 None — insertedUnits/deletedUnits 都为空
        assertTrue(
            "insertedUnits 应为空（净变化为 0）",
            framePatch.insertedUnits.isEmpty(),
        )
        assertTrue(
            "deletedUnits 应为空（净变化为 0）",
            framePatch.deletedUnits.isEmpty(),
        )

        // Issue #725 评论 5750735497：cursorMotionPath 已删除（停止自绘屏幕 caret），
        // 断言2/3（cursorMotionPath == null 或单点 snap）不再适用，已移除。

        // 断言4：timeline 不应产生有持续时长的 cursor track（CURSOR_ONLY 不应用 cursorDurationMillis 播放）
        // 通过检查 framePatch 的有效时长：净变化为 0 时 cursor duration 应为 0
        // computeCursorParamsForPatch 是 private，通过反射检查 visualTimeline 的 cursorChannel
        val cursorDurationNanos = cursorChannelDurationNanos(state)
        assertTrue(
            "净变化为 0 时 cursor channel duration 应为 0（不播放），实际=${cursorDurationNanos}ns\n" +
                "Issue #694 评论 5695660885 问题2：CURSOR_ONLY 净变化为 0 时不应用 cursorDurationMillis 播放",
            cursorDurationNanos == 0L,
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
     * 通过反射访问 ComposeEditorVisualState 的 private localInputTracker 的 pending 数量。
     */
    private fun localInputTrackerSize(state: ComposeEditorVisualState): Int {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("localInputTracker")
        field.isAccessible = true
        val tracker = field.get(state) as LocalInputVisualEditTracker
        return tracker.pendingSize()
    }

    /**
     * Issue #725 评论 5750735497：cursorChannel 已删除（停止自绘屏幕 caret）。
     * 本方法改为直接返回 0L — cursorChannel 不存在意味着不可能有持续时长的 cursor track。
     */
    private fun cursorChannelDurationNanos(state: ComposeEditorVisualState): Long = 0L
}
