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
 * #694 评论 5696394554 复现测试 — 覆盖评论指出的 composition 端态问题。
 *
 * 问题：`compositionVisualPhase = Composing` 现在只在 `onInputSnapshotResolved()` 收到
 * `snapshot.composition != null` 时设置。状态机假设 `snapshotFlow` 一定会先把 composition active
 * 这个中间状态送到 visualState。这个假设不成立。Compose `snapshotFlow` 会 conflate 中间状态，
 * 观察者允许跳过中间状态。`BasicTextField.onTextLayout` 和 `snapshotFlow.collect` 没有先后顺序保证。
 *
 * 存在这个真实顺序：
 * 1. `InputTransformation` 已经 `recordLocalInput("a" -> "an")`；
 * 2. `onAuthoritativeLayout(... compositionActive = true)` 先到，但当前不会把
 *    `compositionVisualPhase` 设成 `Composing`，只会在后面的 composition 分支更新
 *    `lastPresentedLayout/wasCompositionActive`；
 * 3. IME 很快结束 composition，`snapshotFlow` 可能直接跳过 active emission，只看到最终
 *    `composition = null`；
 * 4. 最终 `onAuthoritativeLayout(... compositionActive = false)` 如果又先于 collector 到达，
 *    此时 phase 仍然是 `Idle`，拦截条件不生效；
 * 5. 于是它仍然可能走普通 local-input 路径，或者至少把 coordinator baseline 推到候选文本。
 *
 * 修复后 `onAuthoritativeLayout` 自己也能独立武装 composition phase，
 * `onInputSnapshotResolved` 的 LocalCommitAccepted / AuthoritativeApplied / LocalCommitRejected
 * 以 `compositionVisualPhase` 作为真值收口。
 *
 * 关键：本测试**不先调用 `onInputSnapshotResolved(Composing)`**，模拟 onTextLayout 比 snapshotFlow 快、
 * snapshotFlow conflate 跳过 active emission 的真实顺序。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength", "LargeClass", "CognitiveComplexMethod")
class ComposeVisualIssue694Comment5696394554ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 测试1：LocalCommitRejected 变体 —
     * 模拟 onTextLayout 比 snapshotFlow 快，snapshotFlow conflate 跳过 active emission。
     *
     * 复现序列（不先调 onInputSnapshotResolved(Composing)）：
     * - base = "a"
     * - recordLocalInput("a" -> "an")
     * - onAuthoritativeLayout("an", compositionActive = true) 先到（onTextLayout 比 snapshotFlow 快）
     * - onAuthoritativeLayout("an", compositionActive = false) — composition 结束
     * - bridge 返回 LocalCommitRejected
     *
     * 断言（修复后正确行为）：
     * - outcome 到达前 pendingPatches == 0
     * - frameCoordinator.lastConsumed.text 仍为 "a"（没被推到候选文本 "an"）
     * - outcome 为 Reject 后也不出现 a -> an local patch
     */
    @Test
    fun onTextLayoutBeforeSnapshotFlow_conflateSkipsActive_rejected_preventsLocalPatchAndCoordinatorBaseline() {
        val layouts = captureLayouts("a", "an")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5696394554-rejected",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 1. base = "a"：设置基线 lastPresentedLayout = "a"，frameCoordinator.lastConsumed = "a"
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 2. InputTransformation 已经 recordLocalInput("a" -> "an")（真实运行时无条件 record）
        state.recordLocalInput(
            oldText = "a",
            newText = "an",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )

        // 3. onAuthoritativeLayout("an", compositionActive = true) 先到 —
        //    模拟 onTextLayout 比 snapshotFlow 快，不发送 Composing outcome。
        //    修复后：onAuthoritativeLayout 自己独立武装 composition phase，
        //    compositionBaseLayout = "a"，compositionVisualPhase = Composing。
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = true)

        // 4. IME 很快结束 composition，snapshotFlow conflate 跳过 active emission，
        //    onAuthoritativeLayout("an", compositionActive = false) 先于 collector 到达。
        //    修复后：phase == Composing，拦截条件生效，进入 AwaitingBridgeResolution，不发布 patch。
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)

        // 断言1：outcome 到达前 pendingPatches 应仍为空
        val pendingBeforeOutcome = pendingPatchesSize(state)
        assertTrue(
            "outcome 到达前 pendingPatches 应仍为空，实际=$pendingBeforeOutcome\n" +
                "Issue #694 评论 5696394554：onTextLayout 比 snapshotFlow 快时" +
                "不应提前发布候选 local patch",
            pendingBeforeOutcome == 0,
        )

        // 断言2：frameCoordinator.lastConsumed.text 仍为 "a"（没被推到候选文本 "an"）
        val coordinatorText = frameCoordinatorLastConsumedText(state)
        assertTrue(
            "frameCoordinator.lastConsumed.text 应仍为 \"a\"，实际=\"$coordinatorText\"\n" +
                "Issue #694 评论 5696394554：composition 期间不应把 coordinator baseline 推到候选文本",
            coordinatorText == "a",
        )

        // 5. bridge outcome 到达：LocalCommitRejected（snapshotEnd.composition = null）
        val snapshotEnd =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotEnd, InputSnapshotOutcome.LocalCommitRejected)

        // 断言3：outcome 为 Reject 后也不出现 a -> an local patch
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
                "Issue #694 评论 5696394554：Rejected 后不应保留提前发布的候选 local patch",
            !isAnLocalPatch && pendingAfterReject == 0,
        )
    }

    /**
     * 测试2：AuthoritativeApplied 变体 —
     * 同测试1序列，但最后送 AuthoritativeApplied（composition 期间 Undo 到达）。
     *
     * 断言：始终没有 a -> an local patch。
     */
    @Test
    fun onTextLayoutBeforeSnapshotFlow_conflateSkipsActive_authoritativeApplied_preventsLocalPatch() {
        val layouts = captureLayouts("a", "an")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5696394554-authoritative",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 1. base = "a"
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 2. recordLocalInput("a" -> "an")
        state.recordLocalInput(
            oldText = "a",
            newText = "an",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )

        // 3. onAuthoritativeLayout("an", compositionActive = true) 先到
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = true)

        // 4. onAuthoritativeLayout("an", compositionActive = false) — composition 结束
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)

        // 断言1：outcome 到达前 pendingPatches 应仍为空
        val pendingBeforeOutcome = pendingPatchesSize(state)
        assertTrue(
            "outcome 到达前 pendingPatches 应仍为空，实际=$pendingBeforeOutcome\n" +
                "Issue #694 评论 5696394554：onTextLayout 比 snapshotFlow 快时" +
                "不应提前发布候选 local patch",
            pendingBeforeOutcome == 0,
        )

        // 断言2：frameCoordinator.lastConsumed.text 仍为 "a"
        val coordinatorText = frameCoordinatorLastConsumedText(state)
        assertTrue(
            "frameCoordinator.lastConsumed.text 应仍为 \"a\"，实际=\"$coordinatorText\"\n" +
                "Issue #694 评论 5696394554：composition 期间不应把 coordinator baseline 推到候选文本",
            coordinatorText == "a",
        )

        // 5. bridge outcome 到达：AuthoritativeApplied
        val snapshotEnd =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotEnd, InputSnapshotOutcome.AuthoritativeApplied)

        // 断言3：outcome 为 AuthoritativeApplied 后也不出现 a -> an local patch
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
                "Issue #694 评论 5696394554：AuthoritativeApplied 后不应保留提前发布的候选 local patch",
            !isAnLocalPatch && pendingAfterAuth == 0,
        )
    }

    /**
     * 测试3：LocalCommitAccepted 变体 —
     * 同测试1序列，但最后送 LocalCommitAccepted（Core 接受本地 commit）。
     *
     * 断言（修复后正确行为）：
     * - outcome 到达前 pendingPatches == 0
     * - outcome 为 Accepted 后只收口一次并正常生成最终 a->an local patch
     *   （pendingPatches == 1，latestPatch 是 a->an 且 intent == null）
     */
    @Test
    fun onTextLayoutBeforeSnapshotFlow_conflateSkipsActive_localCommitAccepted_producesFinalLocalPatch() {
        val layouts = captureLayouts("a", "an")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5696394554-accepted",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 1. base = "a"
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 2. recordLocalInput("a" -> "an")
        state.recordLocalInput(
            oldText = "a",
            newText = "an",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )

        // 3. onAuthoritativeLayout("an", compositionActive = true) 先到
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = true)

        // 4. onAuthoritativeLayout("an", compositionActive = false) — composition 结束
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)

        // 断言1：outcome 到达前 pendingPatches 应仍为空
        val pendingBeforeOutcome = pendingPatchesSize(state)
        assertTrue(
            "outcome 到达前 pendingPatches 应仍为空，实际=$pendingBeforeOutcome\n" +
                "Issue #694 评论 5696394554：onTextLayout 比 snapshotFlow 快时" +
                "不应提前发布候选 local patch",
            pendingBeforeOutcome == 0,
        )

        // 5. bridge outcome 到达：LocalCommitAccepted（snapshotEnd.composition = null, text = "an"）
        //    修复后：compositionPhaseActive = true（phase == AwaitingBridgeResolution），
        //    即使 wasCompositionActiveForSnapshot == false 也进入收口，finishCompositionCommit 生成 a->an patch。
        val snapshotEnd =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotEnd, InputSnapshotOutcome.LocalCommitAccepted)

        // 断言2：只收口一次并正常生成最终 a->an local patch
        val pendingAfterAccept = pendingPatchesSize(state)
        assertEquals(
            "LocalCommitAccepted 后应只生成 1 个 a->an local patch，实际=$pendingAfterAccept\n" +
                "Issue #694 评论 5696394554：Accepted 后应正常收口生成最终 local patch",
            1,
            pendingAfterAccept,
        )

        val latestPatch = state.latestPatch.value
        val isAnLocalPatch =
            latestPatch != null &&
                latestPatch.oldLayout.result.layoutInput.text.text == "a" &&
                latestPatch.newLayout.result.layoutInput.text.text == "an" &&
                latestPatch.intent == null
        assertTrue(
            "latestPatch 应为 a->an 且 intent == null，" +
                "latestPatch=$latestPatch isAnLocalPatch=$isAnLocalPatch\n" +
                "Issue #694 评论 5696394554：Accepted 后应生成正确的 a->an local patch",
            isAnLocalPatch,
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
     * 通过反射访问 ComposeEditorVisualState 的 private frameCoordinator 的 lastConsumed.text。
     *
     * 用于检查 composition 期间 frameCoordinator baseline 是否被推到候选文本。
     * frameCoordinator: ComposeVisualFrameCoordinator（private val）
     * lastConsumed: PresentedLayout?（private var）
     * text: String（PresentedLayout 的 val，PresentedLayout 是 private data class）
     */
    private fun frameCoordinatorLastConsumedText(state: ComposeEditorVisualState): String? {
        val coordinatorField = ComposeEditorVisualState::class.java.getDeclaredField("frameCoordinator")
        coordinatorField.isAccessible = true
        val coordinator = coordinatorField.get(state)
        val lastConsumedField = ComposeVisualFrameCoordinator::class.java.getDeclaredField("lastConsumed")
        lastConsumedField.isAccessible = true
        val lastConsumed = lastConsumedField.get(coordinator) ?: return null
        // PresentedLayout 是 private data class，用 javaClass 取其 text 字段
        val textField = lastConsumed.javaClass.getDeclaredField("text")
        textField.isAccessible = true
        return textField.get(lastConsumed) as String?
    }
}
