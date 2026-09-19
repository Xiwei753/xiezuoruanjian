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
import com.xiwei.sujian.feature.editor.layout.cursorRect
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #713 评论 5740578331 的回归测试 —
 *
 * 修复光标所有权问题：`cursorAnimating` 同时承担"timeline 正在跑"和"这一帧应该由视觉光标接管"
 * 两个职责，导致三个问题：
 *
 * 1. handoff 首帧 T0 光标没被画出来 — publishLocalHandoffScene 没改 cursorAnimating，
 *    draw 层 `if (scene.cursorAnimating)` 直接忽略 T0 先画新位置。
 * 2. 纯点击 selection redirect 第一帧仍先瞬移到目标 — pendingSelectionRedirect 排队期间
 *    cursorAnimating=false，draw 层直接画到新 selection。
 * 3. editor.cursor.redirect 日志 from 不是实际动画起点 — 用外层 fallbackFromRect 记诊断，
 *    不是 redirectCursor 内部 sampleCursorRect 的真实起点。
 *
 * 修复方案：新增 `cursorOwnedByVisual` 字段分离光标所有权与动画状态；
 * `pendingSelectionRedirect` 保存 fromRect+targetRect，onInputSnapshotResolved 立即标记视觉所有权；
 * `redirectCursor` 返回真实 startRect 用于诊断。
 */
@Suppress("LongMethod", "MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue713Comment5740578331ReproTest {
    companion object {
        private const val TEXT_ABCDE = "abcde"
    }

    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 问题1：handoff 首帧 T0 光标没被画出来 ====================

    /**
     * 问题1（纯插入）：handoff 首帧 T0 应标记 cursorOwnedByVisual=true，
     * draw 层画 scene.cursorRect（旧位置），不先瞬移到新位置。
     *
     * 旧 bug：publishLocalHandoffScene 算出 handoffCursorRect 并 scene.copy(cursorRect=...)，
     * 但没改 cursorAnimating。从静止状态开始时旧 scene cursorAnimating=false，
     * draw 层 `if (scene.cursorAnimating)` 直接忽略 T0，先画新位置，
     * 下一帧 timeline 启动又从旧位置动画。结果：新位置先闪一帧 -> 回旧位置 -> 再动画到新位置。
     */
    @Test
    fun handoff_pureInsert_firstFrameCursorOwnedByVisual() {
        val layouts = captureLayoutsWithWidth(arrayOf("ab", "axb"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-713-5740578331-handoff-insert",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 计算旧光标位置（offset 1 在 "ab" 中的 cursor rect）
        val oldLayoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val oldCursorRect = oldLayoutSnapshot.cursorRect(1)

        // 初始 layout："ab"，caret 在 offset 1
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 本地输入: "ab" -> "axb"（在 offset 1 插入 'x'）
        state.recordLocalInput(
            oldText = "ab",
            newText = "axb",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )

        // onAuthoritativeLayout 配对生成 localPatch，建立首帧 scene（触发 publishLocalHandoffScene）
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        val firstFrameScene = state.drawSnapshot().scene
        // #713 评论 5740578331：首帧应标记 cursorOwnedByVisual=true，
        // draw 层据此画 scene.cursorRect（旧位置），不先瞬移到新位置。
        assertTrue(
            "handoff-insert: 首帧 scene.cursorOwnedByVisual 应为 true" +
                "（旧 bug：cursorAnimating=false 导致 draw 层忽略 T0 先画新位置，" +
                "表现为新位置先闪一帧 -> 回旧位置 -> 再动画到新位置）",
            firstFrameScene.cursorOwnedByVisual,
        )
        // 首帧 scene.cursorRect 应保持旧光标位置
        assertEquals(
            "handoff-insert: 首帧 scene.cursorRect 应等于旧光标位置（offset=1 in 'ab'）",
            oldCursorRect,
            firstFrameScene.cursorRect,
        )
    }

    /**
     * 问题1（纯删除）：handoff 首帧 T0 应标记 cursorOwnedByVisual=true。
     */
    @Test
    fun handoff_pureDelete_firstFrameCursorOwnedByVisual() {
        val layouts = captureLayoutsWithWidth(arrayOf("axb", "ab"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-713-5740578331-handoff-delete",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 计算旧光标位置（offset 2 在 "axb" 中的 cursor rect）
        val oldLayoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(2, 2), 0)
        val oldCursorRect = oldLayoutSnapshot.cursorRect(2)

        // 初始 layout："axb"，caret 在 offset 2
        state.onAuthoritativeLayout(layouts[0], TextRange(2, 2), 0)

        // 本地输入: "axb" -> "ab"（删除 offset 1 的 'x'）
        state.recordLocalInput(
            oldText = "axb",
            newText = "ab",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 1), oldRange = TextRange(1, 2))),
        )

        // onAuthoritativeLayout 配对生成 localPatch，建立首帧 scene
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        val firstFrameScene = state.drawSnapshot().scene
        assertTrue(
            "handoff-delete: 首帧 scene.cursorOwnedByVisual 应为 true",
            firstFrameScene.cursorOwnedByVisual,
        )
        assertEquals(
            "handoff-delete: 首帧 scene.cursorRect 应等于旧光标位置（offset=2 in 'axb'）",
            oldCursorRect,
            firstFrameScene.cursorRect,
        )
    }

    // ==================== 问题2：纯点击 selection redirect 第一帧仍先瞬移到目标 ====================

    /**
     * 问题2：纯 selection redirect 应立即标记 cursorOwnedByVisual=true，
     * draw 层画 fromRect（旧位置），不先瞬移到 target。
     *
     * 旧 bug：onInputSnapshotResolved 只设 pendingSelectionRedirect = targetRect 并唤醒帧循环。
     * 但 selection 一变 liveSelection 已是新位置，redirectCursor() 要等下一帧才执行。
     * 这之前 scene.cursorAnimating=false，draw 层直接画到新 selection。
     * 下一帧 redirect 才从旧位置开始。结果：先瞬移到目标 -> 下一帧回旧位置 -> 再平滑过去。
     */
    @Test
    fun pureSelectionRedirect_immediatelyMarksCursorOwnedByVisual() {
        val layouts = captureLayoutsWithWidth(arrayOf(TEXT_ABCDE), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-713-5740578331-pure-selection-redirect",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout："abcde"，caret 在 offset 1
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 第一次 onInputSnapshotResolved：建立 lastResolvedSelection = TextRange(1,1)
        val snapshotInitial =
            EditorInputSnapshot(
                text = TEXT_ABCDE,
                selection = TextRange(1, 1),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotInitial, InputSnapshotOutcome.NoTextChange)

        // 先 drain + sample 一次让 timeline 收口
        state.drainPendingPatchesAtFrame(0L)
        state.sampleVisualScene(0L)

        // 计算 fromRect（offset 1 在 "abcde" 中的 cursor rect）— selection 改变前屏幕真正可见的 cursor
        val layoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val fromRect = layoutSnapshot.cursorRect(1)

        // 注：Robolectric 测试环境下无真实字体渲染，cursorRect 可能对不同 offset 返回相同位置。
        // 此处不断言 fromRect != targetRect — 核心断言是 cursorOwnedByVisual=true（新字段），
        // 旧 bug 时该字段不存在或为 false，draw 层直接画到 liveSelection（新位置）。

        // 模拟纯 selection 变化：用户点击 offset 3，text 不变，selection 从 (1,1) 变成 (3,3)
        val snapshotRedirect =
            EditorInputSnapshot(
                text = TEXT_ABCDE,
                selection = TextRange(3, 3),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotRedirect, InputSnapshotOutcome.NoTextChange)

        // #713 评论 5740578331：onInputSnapshotResolved 应立即把 draw snapshot 的 cursor 保持在 fromRect，
        // 并标记 cursorOwnedByVisual=true — 防止下一帧 drain 之前 draw 层先瞬移到 target。
        val sceneAfterRedirect = state.drawSnapshot().scene
        assertTrue(
            "pure-selection-redirect: pending redirect 期间 scene.cursorOwnedByVisual 应为 true" +
                "（旧 bug：pending redirect 期间 cursorAnimating=false，draw 层直接画到新 selection，" +
                "表现为先瞬移到目标 -> 下一帧回旧位置 -> 再平滑过去）",
            sceneAfterRedirect.cursorOwnedByVisual,
        )
        // cursorRect 应等于 fromRect（selection 改变前屏幕真正可见的 cursor）
        // 即使 Robolectric 下 fromRect == targetRect，此断言仍验证 cursor 被显式保持在 fromRect，
        // 而非由 draw 层从 liveSelection 算出（cursorOwnedByVisual=true 时 draw 层用 scene.cursorRect）。
        assertEquals(
            "pure-selection-redirect: pending redirect 期间 scene.cursorRect 应等于 fromRect（offset=1 in 'abcde'），" +
                "不是由 draw 层从 liveSelection(target) 算出",
            fromRect,
            sceneAfterRedirect.cursorRect,
        )
    }

    // ==================== 问题3：editor.cursor.redirect 日志 from 不是实际动画起点 ====================

    /**
     * 问题3：redirectCursor 应返回实际使用的 startRect（sampleCursorRect 的值），
     * 不是外层传入的 fallbackFromRect。
     *
     * 旧 bug：drainPendingPatchesAtFrame 记日志用的是外层算的 fallbackFromRect，
     * 但 redirectCursor() 内部若已有 cursorChannel，真实起点是 sampleCursorRect(frameTimeNanos)。
     * 快速点击/动画中重定向时诊断 fromX/fromY 与屏幕真实起点不一致。
     */
    @Test
    fun redirectCursor_returnsActualStartRect() {
        val timeline = ComposeVisualTimeline()

        // 第一次 redirect：从 (0,0,2,20) 动画到 (100,0,102,20)，时长 1s
        val fallback1 = Rect(0f, 0f, 2f, 20f)
        val target1 = Rect(100f, 0f, 102f, 20f)
        val durationNanos = 1_000_000_000L
        val startRect1 =
            timeline.redirectCursor(
                frameTimeNanos = 0L,
                fallbackFromRect = fallback1,
                targetRect = target1,
                durationNanos = durationNanos,
            )
        // 第一次没有 cursorChannel，startRect 应等于 fallback
        assertEquals(
            "redirectCursor 第一次调用：无 cursorChannel，startRect 应等于 fallbackFromRect",
            fallback1,
            startRect1,
        )

        // 第二次 redirect：在 500ms 时重定向到 (200,0,202,20)
        // 此时 cursorChannel 已存在，真实起点应是 sampleCursorRect(500ms) — 在 0->100 动画中点附近
        val fallback2 = Rect(0f, 0f, 2f, 20f)
        val target2 = Rect(200f, 0f, 202f, 20f)
        val frameTime2 = 500_000_000L
        val expectedActualStart = timeline.sampleCursorRect(frameTime2)
        assertTrue(
            "redirectCursor 测试前提：sampleCursorRect(500ms) 应非 null（cursorChannel 已建立）",
            expectedActualStart != null,
        )
        val startRect2 =
            timeline.redirectCursor(
                frameTimeNanos = frameTime2,
                fallbackFromRect = fallback2,
                targetRect = target2,
                durationNanos = durationNanos,
            )
        // #713 评论 5740578331：第二次 redirectCursor 应返回 sampleCursorRect 的值，
        // 不是 fallback。在 0->100 动画中点（500ms），left 应约等于 50，不是 fallback 的 0。
        assertEquals(
            "redirectCursor 第二次调用：有 cursorChannel，startRect 应等于 sampleCursorRect(500ms)" +
                "（在 0->100 动画中点附近，约 left≈50），不是 fallback Rect(0,0,2,20)" +
                "（旧 bug：诊断记 fallback，与屏幕真实起点不一致）",
            expectedActualStart,
            startRect2,
        )
        assertNotEquals(
            "redirectCursor 第二次调用：startRect 应不等于 fallbackFromRect" +
                "（快速点击/动画中重定向时两者不同）",
            fallback2,
            startRect2,
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
