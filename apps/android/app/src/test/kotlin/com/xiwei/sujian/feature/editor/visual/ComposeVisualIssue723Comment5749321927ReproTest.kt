package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.input.EditorInputSnapshot
import com.xiwei.sujian.feature.editor.input.InputSnapshotOutcome
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.EditorSoftBreakProjection
import com.xiwei.sujian.feature.editor.layout.cursorRect
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #723 评论 5749321927 复现测试 —
 *
 * 基线提交 0a95ed86 已修对的部分（不要动）：
 * - EditorMotionPolicy.effective() 在 coordinated=true 时强制文字和光标一起开启。
 * - 系统 caret 所有权拆成 drawsVisualCursor 和 cursorOwnedByVisual 两层。
 *
 * 剩余缺陷：点击 wedge / 拖动选区时，只是不创建新自绘动画，没有取消旧的自绘所有权。
 *
 * ComposeEditorVisualState.onInputSnapshotResolved() 当前逻辑：
 * - collapsed selection 且没命中 wedge → 建 pendingSelectionRedirect，视觉层接管 caret；
 * - collapsed selection 命中 wedge → 直接跳过，不建 redirect；
 * - 非 collapsed selection（拖动选区）→ 整段也直接跳过。
 *
 * 问题："跳过"不等于"交还"。如果前一笔输入动画还在跑，或者前一笔 pendingSelectionRedirect
 * 还没消费，那么：
 * - visualTimeline 里的旧 cursorChannel 还在；
 * - pendingSelectionRedirect 也可能还是旧目标；
 * - _cursorOwnedByVisual 仍可能是 true；
 * - 下一次 sampleVisualScene() 又会从旧 cursor track 产出 cursorOwnedByVisual=true。
 *
 * 于是系统 caret 仍然会被透明掉，甚至旧 redirect 还能继续把自绘光标拉向上一次点击的位置。
 * 拖选时系统手柄已经到了新选区，但旧自绘 caret 还可能继续存在，这正是 #723 想断掉的
 * "手柄和光标两套几何"。
 *
 * 本测试覆盖评论 5749321927 明确点名的两个最关键时序：
 * 1. 文字/光标动画正在拥有 caret → 用户点击 wedge → 本次调用后 ownership 必须立刻 false，
 *    旧 cursor track 不能下一帧重新抢回来；
 * 2. 已有 pending selection redirect → 用户马上拖成非 collapsed selection → pending redirect
 *    必须被清掉，不能之后再动画到旧目标。
 *
 * 在基线 0a95ed86 下两个测试均 FAIL（暴露缺陷），断言到残留状态：
 * - cursorOwnedByVisual 应为 false 但实际仍为 true；
 * - hasPendingPatches() 应为 false（旧 redirect 已清）但实际仍为 true。
 */
@Suppress("LongMethod", "MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue723Comment5749321927ReproTest {
    companion object {
        private const val TEXT_ABCDE = "abcde"
    }

    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 时序1：文字/光标动画正在拥有 caret → 用户点击 wedge ====================

    /**
     * 时序1（评论 5749321927 最关键时序之一）：
     *
     * 前一笔纯 selection redirect 已 drain 进 timeline，cursorChannel 已建立，
     * visualTimeline.sample() 产出 cursorOwnedByVisual=true（动画正在拥有 caret）。
     * 此时用户点击的位置 raw offset 正好命中 projection wedge（wedgeStart != wedgeEnd）。
     *
     * 期望（#723 要的）：本次 onInputSnapshotResolved 调用后 ownership 必须立刻 false，
     * 旧 cursor track 不能下一帧重新抢回来。
     *
     * 基线 0a95ed86 缺陷：onInputSnapshotResolved 在 hitsWedge=true 时只是跳过建新 redirect，
     * 没有交还旧所有权 — cursorChannel 残留、_cursorOwnedByVisual 残留 true，
     * 下一帧 sampleVisualScene() 又从旧 cursor track 产出 cursorOwnedByVisual=true，
     * 系统 caret 仍被透明掉，旧自绘 caret 继续存在（"手柄和光标两套几何"）。
     *
     * 本测试在基线下 FAIL（断言到残留的 cursorOwnedByVisual=true），暴露"跳过≠交还"。
     */
    @Test
    fun cursorAnimationOwningCaret_thenClickWedge_ownershipMustReturnFalseImmediately() {
        val layouts = captureLayoutsWithWidth(arrayOf(TEXT_ABCDE), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-723-5749321927-wedge-click",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 构造带 wedge 的投影：在 raw offset 3 处插入一个 U+200B，
        // 使得 wedgeStart(3)=3 != wedgeEnd(3)=4 → 点击 offset 3 命中 wedge。
        val projectionWithWedgeAt3 = EditorSoftBreakProjection(
            rawLength = TEXT_ABCDE.length,
            insertPoints = listOf(3),
        )

        // 初始 layout："abcde"，caret 在 offset 1，带 wedge 投影
        state.onAuthoritativeLayout(
            layouts[0],
            TextRange(1, 1),
            0,
            projection = projectionWithWedgeAt3,
            rawText = TEXT_ABCDE,
        )

        // 步骤1：用户先点 offset 1 → offset 2（非 wedge、collapsed），
        // 建立 pendingSelectionRedirect，视觉层接管 caret。
        // （lastResolvedSelection 初始 null，第一次调用必然 selection 变了，直接建 redirect）
        val layoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0, projectionWithWedgeAt3)
        val fromRect = layoutSnapshot.cursorRect(1)
        val snapshotRedirect =
            EditorInputSnapshot(
                text = TEXT_ABCDE,
                selection = TextRange(2, 2),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotRedirect, InputSnapshotOutcome.NoTextChange)

        // pending redirect 已建立，cursorOwnedByVisual=true
        assertTrue(
            "前提：建立 pending redirect 后 cursorOwnedByVisual 应为 true",
            state.cursorOwnedByVisual.value,
        )
        assertTrue(
            "前提：建立 pending redirect 后 hasPendingPatches 应为 true",
            state.hasPendingPatches(),
        )

        // 步骤2：drain 让 pending redirect 进入 timeline，cursorChannel 建立（动画正在拥有 caret）
        val frameTime1 = 1_000_000L
        state.drainPendingPatchesAtFrame(frameTime1)

        // 步骤3：sample — cursorChannel 已建立，cursorAnimating=true → cursorOwnedByVisual=true
        // 此时"文字/光标动画正在拥有 caret"，正是评论 5749321927 时序1 的起点。
        state.sampleVisualScene(frameTime1)
        assertTrue(
            "前提：drain+sample 后 cursorChannel 已建立，cursorOwnedByVisual 应为 true（动画正在拥有 caret）",
            state.cursorOwnedByVisual.value,
        )

        // 步骤4（关键）：用户点击 offset 3 — 正好命中 wedge（wedgeStart(3)=3 != wedgeEnd(3)=4）
        // 期望：本次 onInputSnapshotResolved 调用后 ownership 必须立刻 false，
        // 旧 cursor track 不能下一帧重新抢回来。
        val snapshotClickWedge =
            EditorInputSnapshot(
                text = TEXT_ABCDE,
                selection = TextRange(3, 3),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotClickWedge, InputSnapshotOutcome.NoTextChange)

        // 基线 0a95ed86 缺陷断言：点击 wedge 后 cursorOwnedByVisual 仍为 true（没有交还）。
        // 修复后此处应为 false。本断言在基线下 FAIL（暴露残留），驱动修复。
        assertFalse(
            "时序1 缺陷：点击 wedge 后 cursorOwnedByVisual 必须立刻 false（交还系统 caret），" +
                "但基线 0a95ed86 只是跳过建新 redirect，没有交还旧 cursorChannel 所有权，" +
                "下一帧 sampleVisualScene() 又从旧 cursor track 产出 cursorOwnedByVisual=true，" +
                "系统 caret 仍被透明掉，旧自绘 caret 继续存在（手柄和光标两套几何）",
            state.cursorOwnedByVisual.value,
        )

        // 进一步证据：下一帧 sample 后 cursorOwnedByVisual 仍 true（旧 cursor track 抢回来）
        val frameTime2 = 2_000_000L
        state.sampleVisualScene(frameTime2)
        assertFalse(
            "时序1 缺陷（下一帧）：sample 后 cursorOwnedByVisual 必须仍为 false，" +
                "旧 cursor track 不能重新抢回来",
            state.cursorOwnedByVisual.value,
        )

        // 引用 fromRect 避免未使用警告
        assertTrue("前提：fromRect 应非空", fromRect.width >= 0f)
    }

    // ==================== 时序2：已有 pending selection redirect → 用户马上拖成非 collapsed selection ====================

    /**
     * 时序2（评论 5749321927 最关键时序之二）：
     *
     * 前一笔纯 selection redirect 已建立（pendingSelectionRedirect != null）但还没 drain，
     * 用户马上拖成非 collapsed selection（selection.start != selection.end）。
     *
     * 期望（#723 要的）：pending redirect 必须被清掉，不能之后再动画到旧目标。
     *
     * 基线 0a95ed86 缺陷：onInputSnapshotResolved 在非 collapsed selection 时整段跳过外层 if，
     * 没有清掉 pendingSelectionRedirect — 旧 redirect 残留，drainPendingPatchesAtFrame 下一帧
     * 仍会消费它，把自绘光标动画到旧目标；sampleVisualScene 期间也会强制 cursorOwnedByVisual=true
     * 且 cursorRect=旧 fromRect，系统手柄已到新选区但自绘 caret 还在旧位置（两套几何）。
     *
     * 本测试在基线下 FAIL（断言到残留的 pendingSelectionRedirect 和 cursorOwnedByVisual=true）。
     */
    @Test
    fun pendingSelectionRedirect_thenDragNonCollapsed_redirectMustBeCleared() {
        val layouts = captureLayoutsWithWidth(arrayOf(TEXT_ABCDE), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-723-5749321927-drag-noncollapsed",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout："abcde"，caret 在 offset 1，identity 投影（无 wedge）
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 第一次 onInputSnapshotResolved：建立 lastResolvedSelection = TextRange(1,1)
        val snapshotInitial =
            EditorInputSnapshot(
                text = TEXT_ABCDE,
                selection = TextRange(1, 1),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotInitial, InputSnapshotOutcome.NoTextChange)

        // 先 drain + sample 让 timeline 收口
        state.drainPendingPatchesAtFrame(0L)
        state.sampleVisualScene(0L)

        // 步骤1：用户先点 offset 1 → offset 2（非 wedge、collapsed），建立 pendingSelectionRedirect
        val snapshotRedirect =
            EditorInputSnapshot(
                text = TEXT_ABCDE,
                selection = TextRange(2, 2),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotRedirect, InputSnapshotOutcome.NoTextChange)

        // 前提确认：pending redirect 已建立
        assertTrue(
            "前提：建立 pending redirect 后 hasPendingPatches 应为 true",
            state.hasPendingPatches(),
        )
        assertTrue(
            "前提：建立 pending redirect 后 cursorOwnedByVisual 应为 true",
            state.cursorOwnedByVisual.value,
        )

        // 记算旧 redirect 的 fromRect（offset 1 在 "abcde" 中的 cursor rect）—
        // 这是旧 redirect 目标的起点，残留时下一帧会把自绘光标拉向这里
        val layoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val oldFromRect = layoutSnapshot.cursorRect(1)

        // 步骤2（关键）：用户马上拖成非 collapsed selection（从 offset 2 拖到 offset 4）
        // 期望：pending redirect 必须被清掉，不能之后再动画到旧目标。
        val snapshotDragNonCollapsed =
            EditorInputSnapshot(
                text = TEXT_ABCDE,
                selection = TextRange(2, 4),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotDragNonCollapsed, InputSnapshotOutcome.NoTextChange)

        // 基线 0a95ed86 缺陷：非 collapsed 时整段跳过外层 if，pendingSelectionRedirect 残留。
        // 修复后 hasPendingPatches() 应为 false。本断言在基线下 FAIL（暴露残留），驱动修复。
        assertFalse(
            "时序2 缺陷：拖成非 collapsed selection 后 pendingSelectionRedirect 必须被清掉，" +
                "但基线 0a95ed86 整段跳过外层 if，旧 redirect 残留，" +
                "drainPendingPatchesAtFrame 下一帧仍会消费它，把自绘光标动画到旧目标",
            state.hasPendingPatches(),
        )

        // 进一步证据：cursorOwnedByVisual 必须回 false（交还系统 caret）
        assertFalse(
            "时序2 缺陷：拖成非 collapsed selection 后 cursorOwnedByVisual 必须 false（交还系统 caret），" +
                "但基线下旧 redirect 残留，sampleVisualScene 期间强制 cursorOwnedByVisual=true",
            state.cursorOwnedByVisual.value,
        )

        // 进一步证据：下一帧 sample 后 cursorOwnedByVisual 仍 true（旧 redirect 强制接管）
        val frameTime1 = 1_000_000L
        state.sampleVisualScene(frameTime1)
        assertFalse(
            "时序2 缺陷（下一帧）：sample 后 cursorOwnedByVisual 必须 false，" +
                "旧 redirect 不能继续把自绘光标拉向旧目标",
            state.cursorOwnedByVisual.value,
        )

        // 进一步证据：drain 后不应再触发 editor.cursor.redirect 到旧目标 —
        // 用 hasPendingPatches 在 drain 后仍 false 验证（旧 redirect 不应被消费）
        state.drainPendingPatchesAtFrame(frameTime1)
        assertFalse(
            "时序2 缺陷（drain 后）：旧 redirect 不应被消费，hasPendingPatches 应 false",
            state.hasPendingPatches(),
        )

        // 引用 oldFromRect 避免未使用警告
        assertTrue("前提：oldFromRect 应非空", oldFromRect.width >= 0f)
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

    /**
     * 一次 setContent 内捕获多种宽度的 layout —
     * composeRule.setContent 每个测试只能调用一次，
     * 需要不同宽度 layout 的测试用本方法一次取齐。
     */
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
