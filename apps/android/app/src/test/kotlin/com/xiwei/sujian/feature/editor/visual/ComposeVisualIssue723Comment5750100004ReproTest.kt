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
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * Issue #723 评论 5750100004 回归测试 — 跨两套 patch id 的时序边界。
 *
 * [ComposeVisualPatch.id] 有两套来源：
 * - [ComposeVisualFrameCoordinator.nextPatchId]：Core/external patch 从 0 起递增（id 很小，如 0,1,2）。
 * - [ComposeEditorVisualState.nextLocalPatchId]：本地输入 patch 从 1_000_000 起递增（id 很大，如 1_000_001）。
 *
 * 旧代码用 `suppressCursorThroughPatchId = pendingPatches.maxOfOrNull { it.id }` 做 release 前后 patch 的
 * 时间边界，但两套 id 跨来源不单调，导致反例：
 * 1. local patch A（id≈1_000_001）pending → release 记 boundary=1_000_001；
 * 2. 新 Core patch B（id≈1）pending → drain 按 `it.id <= boundary` 分组；
 * 3. B 的 id=1 <= 1_000_001 被误判成旧 patch，assignCursorChannel=false，
 *    release 之后的新 Core 光标动画被错误压掉。
 *
 * 修复（评论 5750100004）：用 [PendingPatch.sequence]（统一入队序号、连续递增）替代 [ComposeVisualPatch.id]
 * 做边界。release 记 `suppressCursorThroughSequence = pendingPatches.lastOrNull()?.sequence`，
 * drain 按 `it.sequence <= boundary` 分段。任何来源的 patch 入队都拿到连续递增的 sequence，
 * 不受两套 patch.id 影响。
 *
 * 本测试覆盖核心时序：
 * - local patch（id≈1_000_001, sequence=0）pending → release（boundary sequence=0）→
 *   新 Core patch（id≈1, sequence=1）pending → drain →
 *   旧 local patch 被 suppress（assignCursorChannel=false），新 Core patch 不被 suppress（assignCursorChannel=true）→
 *   cursorOwnedByVisual=true（新 Core patch 取得 caret）。
 *
 * 基线缺陷（用 patch.id 边界）：新 Core patch id=1 <= boundary=1_000_001 被误判成旧 patch，
 * assignCursorChannel=false，cursorOwnedByVisual=false（新 Core 光标动画被错误压掉）。
 */
@Suppress("LongMethod", "MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue723Comment5750100004ReproTest {
    companion object {
        private const val TEXT_ABCDE = "abcde"
        private const val TEXT_FABCDE = "fabcde"
        private const val TEXT_FABCDEG = "fabcdeg"
    }

    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 核心时序（评论 5750100004）：跨两套 patch id 的 release 边界 —
     *
     * 1. 初始 layout "abcde" caret offset 1，drain+sample 收口。
     * 2. 用户输入 "f" → recordLocalInput + onAuthoritativeLayout(layouts[1]) 配对生成 localPatch
     *    （id≈1_000_001, sequence=0）入队 pendingPatches。
     * 3. 用户拖成非 collapsed selection → onInputSnapshotResolved → releaseVisualCursorOwnership()
     *    记录 suppressCursorThroughSequence=0（local patch 的 sequence）。
     * 4. release 之后新 Core 视觉意图到达 → onVisualIntent(intent) 串进 frameCoordinator pending chain。
     * 5. 新 layout "fabcdeg" 到达 → onAuthoritativeLayout(layouts[2]) → frameCoordinator.onLayout
     *    生成 Core patch（id≈1, sequence=1）入队 pendingPatches。
     *    关键：Core patch.id=1 << local patch.id=1_000_001（跨两套 id），
     *    但 Core patch.sequence=1 > local patch.sequence=0（sequence 连续递增）。
     * 6. 下一帧 drainPendingPatchesAtFrame 消费两笔 patch：
     *    - 旧 local patch sequence=0 <= boundary=0 → assignCursorChannel=false（suppress）；
     *    - 新 Core patch sequence=1 > boundary=0 → assignCursorChannel=true（不 suppress）。
     * 7. sampleVisualScene → cursorOwnedByVisual=true（新 Core patch 取得 caret）。
     *
     * 基线缺陷（用 patch.id 边界）：boundary=1_000_001，Core patch id=1 <= 1_000_001
     * 被误判成旧 patch → assignCursorChannel=false → cursorOwnedByVisual=false。
     */
    @Test
    fun localPatchPending_thenRelease_thenNewCorePatchPending_drainMustSuppressOldButNotNew() {
        val layouts = captureLayoutsWithWidth(arrayOf(TEXT_ABCDE, TEXT_FABCDE, TEXT_FABCDEG), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-723-5750100004-cross-id",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 步骤1：初始 layout "abcde"，caret 在 offset 1
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 设置 lastResolvedSelection
        val snapshotInitial =
            EditorInputSnapshot(
                text = TEXT_ABCDE,
                selection = TextRange(1, 1),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotInitial, InputSnapshotOutcome.NoTextChange)

        // drain + sample 让 timeline 收口
        state.drainPendingPatchesAtFrame(0L)
        state.sampleVisualScene(0L)

        // 步骤2：用户输入 "f" → recordLocalInput
        state.recordLocalInput(
            oldText = TEXT_ABCDE,
            newText = TEXT_FABCDE,
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )

        // onAuthoritativeLayout 配对生成 localPatch（id≈1_000_001, sequence=0）入队 pendingPatches
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // 前提确认：pendingPatches 非空
        assertTrue(
            "前提：输入后 pendingPatches 应非空（localPatch id≈1_000_001 已入队）",
            state.hasPendingPatches(),
        )

        // 步骤3（关键）：用户马上拖成非 collapsed selection → releaseVisualCursorOwnership()
        // 记录 suppressCursorThroughSequence = 0（local patch 的 sequence）
        val snapshotDrag =
            EditorInputSnapshot(
                text = TEXT_FABCDE,
                selection = TextRange(2, 4),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotDrag, InputSnapshotOutcome.NoTextChange)

        // 释放后 cursorOwnedByVisual 必须立刻 false
        assertFalse(
            "步骤3：拖成非 collapsed selection 后 cursorOwnedByVisual 必须 false",
            state.cursorOwnedByVisual.value,
        )

        // 步骤4：release 之后新 Core 视觉意图到达 → onVisualIntent
        // 构造一个插入 "g" 的 Core intent：把 "fabcde" → "fabcdeg"
        // intent.expectedOldText = "fabcde"（与 frameCoordinator.lastConsumed.text 一致）
        // intent.expectedNewText = "fabcdeg"（与步骤5 的 layout 一致）
        // cursor: oldEndUtf16=6 → newEndUtf16=7, animate=true（带 cursor motion）
        val coreIntent =
            EditorVisualIntent(
                coreTransactionId = 100L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.GLYPH_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(6, 7)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 6, newEndUtf16 = 7, animate = true),
                replaceBounds = VisualReplaceBounds(oldStart = 6, oldEnd = 6, newStart = 6, newEnd = 7),
                expectedOldText = TEXT_FABCDE,
                expectedNewText = TEXT_FABCDEG,
                oldAnimationUnits = emptyList(),
                newAnimationUnits = listOf(TextRange(6, 7)),
            )
        state.onVisualIntent(coreIntent, EditorMotionPolicy())

        // 步骤5：新 layout "fabcdeg" 到达 → onAuthoritativeLayout
        // frameCoordinator.onLayout 生成 Core patch（id≈1, sequence=1）入队 pendingPatches
        // 关键：Core patch.id=1 << local patch.id=1_000_001（跨两套 id）
        // 但 Core patch.sequence=1 > local patch.sequence=0（sequence 连续递增）
        state.onAuthoritativeLayout(layouts[2], TextRange(7, 7), 0)

        // 前提确认：pendingPatches 有两笔 patch（local + Core）
        assertTrue(
            "前提：release 后新 Core patch 入队，pendingPatches 应非空",
            state.hasPendingPatches(),
        )

        // 步骤6：下一帧 drainPendingPatchesAtFrame 消费两笔 patch
        // 修复后（sequence 边界）：
        // - 旧 local patch sequence=0 <= boundary=0 → assignCursorChannel=false（suppress）
        // - 新 Core patch sequence=1 > boundary=0 → assignCursorChannel=true（不 suppress）
        // 基线缺陷（patch.id 边界）：
        // - 两笔 patch id 都 <= boundary=1_000_001 → 都被误判成旧 patch → assignCursorChannel=false
        val frameTime1 = 1_000_000L
        state.drainPendingPatchesAtFrame(frameTime1)

        // 步骤7：sampleVisualScene — cursorOwnedByVisual 必须为 true
        // 修复后：新 Core patch 取得 caret 所有权 → cursorOwnedByVisual=true
        // 基线缺陷：新 Core patch 被误 suppress → cursorOwnedByVisual=false
        state.sampleVisualScene(frameTime1)
        assertTrue(
            "步骤7（关键）：drain 后 cursorOwnedByVisual 必须为 true，" +
                "release 之后新 Core patch（id≈1, sequence=1）不应被 release 之前 local patch" +
                "（id≈1_000_001, sequence=0）的 suppressCursorThroughSequence 边界误 suppress。" +
                "跨两套 patch.id 时序边界必须用 PendingPatch.sequence 而非 ComposeVisualPatch.id 判断。",
            state.cursorOwnedByVisual.value,
        )

        // 进一步证据：再过一帧仍然 true（新 Core patch 的 cursor 动画在跑）
        val frameTime2 = 2_000_000L
        state.sampleVisualScene(frameTime2)
        assertTrue(
            "步骤7（下一帧）：cursorOwnedByVisual 必须仍为 true（新 Core patch cursor 动画在跑）",
            state.cursorOwnedByVisual.value,
        )
    }

    /**
     * 辅助时序（评论 5750100004）：跨两套 patch id 时文字 clip 动画继续 —
     * drain 时旧 local patch 虽被 suppress（assignCursorChannel=false），
     * 但 clipTracks 仍正常创建，文字吞字/吐字动画不受影响。
     */
    @Test
    fun localPatchPending_thenRelease_thenNewCorePatchPending_textClipAnimationMustContinue() {
        val layouts = captureLayoutsWithWidth(arrayOf(TEXT_ABCDE, TEXT_FABCDE, TEXT_FABCDEG), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-723-5750100004-clip-continue",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout "abcde"，caret 在 offset 1
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        val snapshotInitial =
            EditorInputSnapshot(
                text = TEXT_ABCDE,
                selection = TextRange(1, 1),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotInitial, InputSnapshotOutcome.NoTextChange)

        state.drainPendingPatchesAtFrame(0L)
        state.sampleVisualScene(0L)

        // 步骤2：用户输入 "f" → localPatch（id≈1_000_001, sequence=0）
        state.recordLocalInput(
            oldText = TEXT_ABCDE,
            newText = TEXT_FABCDE,
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // 步骤3：拖成非 collapsed selection → release
        val snapshotDrag =
            EditorInputSnapshot(
                text = TEXT_FABCDE,
                selection = TextRange(2, 4),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotDrag, InputSnapshotOutcome.NoTextChange)

        // 步骤4：新 Core intent 到达
        val coreIntent =
            EditorVisualIntent(
                coreTransactionId = 200L,
                baseRevision = 3L,
                newRevision = 4L,
                animationMode = AnimationModeDto.GLYPH_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(6, 7)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 6, newEndUtf16 = 7, animate = true),
                replaceBounds = VisualReplaceBounds(oldStart = 6, oldEnd = 6, newStart = 6, newEnd = 7),
                expectedOldText = TEXT_FABCDE,
                expectedNewText = TEXT_FABCDEG,
                oldAnimationUnits = emptyList(),
                newAnimationUnits = listOf(TextRange(6, 7)),
            )
        state.onVisualIntent(coreIntent, EditorMotionPolicy())

        // 步骤5：新 layout 到达 → Core patch（id≈1, sequence=1）
        state.onAuthoritativeLayout(layouts[2], TextRange(7, 7), 0)

        // 步骤6：drain + sample
        val frameTime1 = 1_000_000L
        state.drainPendingPatchesAtFrame(frameTime1)
        val scene = state.sampleVisualScene(frameTime1)

        // 关键断言：cursorOwnedByVisual 为 true（新 Core patch 取得 caret）
        assertTrue(
            "辅助时序：drain 后 cursorOwnedByVisual 必须 true（新 Core patch 取得 caret）",
            state.cursorOwnedByVisual.value,
        )

        // 关键断言：文字 clip 动画继续 — scene.units 非空
        // 旧 local patch 虽被 suppress（assignCursorChannel=false），clipTracks 仍正常创建；
        // 新 Core patch 的 insertedUnits 也正常创建。文字吞字/吐字动画在跑。
        assertTrue(
            "辅助时序：drain 后文字 clip 动画必须继续 — scene.units 应非空" +
                "（旧 local patch 被 suppress 只影响 cursorChannel，不影响 clipTracks）",
            scene.units.isNotEmpty(),
        )
    }

    // ==================== 辅助方法 ====================

    private fun captureLayoutsWithWidth(
        texts: Array<out String>,
        maxWidth: Int,
        fontSizeSp: Float = 14f,
    ): List<TextLayoutResult> =
        captureLayoutsWithMultipleWidths(
            *texts.map { it to maxWidth }.toTypedArray(),
            fontSizeSp = fontSizeSp,
        )

    private fun captureLayoutsWithMultipleWidths(
        vararg textWidthPairs: Pair<String, Int>,
        fontSizeSp: Float = 14f,
    ): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            for ((text, width) in textWidthPairs) {
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = fontSizeSp.sp),
                        constraints = Constraints(maxWidth = width),
                    ),
                )
            }
        }
        return results
    }
}
