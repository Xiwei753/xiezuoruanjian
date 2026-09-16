package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
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
 * #694 评论 5691696678 回归测试 — 覆盖新路由 `recordLocalInput() -> onAuthoritativeLayout()`。
 *
 * 现有 [ComposeVisualIssue694ReproTest] 主要通过 `onVisualIntent()` 构造 Core patch 测 same-VSync batch，
 * 没有真正覆盖新路由。本测试直接覆盖评论 5691696678 指出的 3 个问题：
 *
 * 1. 中间 layout 被跳过的快速输入（问题1：drainMatchingChain 合并连续链，patch 不被丢掉）
 * 2. 快速 Backspace/删除换行（问题1：删除链也能合并）
 * 3. 本地输入后接 Undo/Redo（问题2：observePresentedLayout 推进 coordinator 基线）
 * 4. 同一 VSync 多笔本地输入保留 ordered insertedUnits/cursor path（问题3：batch 合成保留吐字顺序）
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength")
class ComposeVisualIssue694LocalRouteTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 问题1 核心回归：中间 layout 被跳过的快速输入，patch 不应被丢掉。
     *
     * 场景：recordLocalInput(""->"a"), recordLocalInput("a"->"ab"), recordLocalInput("ab"->"abc") 三笔，
     * 中间两个 layout 没真正呈现，只调一次 onAuthoritativeLayout("abc" 的 layout)。
     *
     * 旧实现 drainMatching("abc") 只返回最后一笔 "ab"->"abc"，buildLocalInputPatch 检查
     * oldText("ab") != oldLayout.text("") 直接丢 patch。
     * 新实现 drainMatchingChain("", "abc") 返回完整 chain，patch 不被丢掉。
     */
    @Test
    fun skippedIntermediateLayouts_shouldNotDropPatch() {
        val layouts = captureLayouts("", "abc")
        val state = ComposeEditorVisualState(targetId = "issue694-local-skip")

        // 设置基线：lastPresentedLayout = ""
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 三笔快速输入，中间 layout 被跳过（没调 onAuthoritativeLayout）
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )
        state.recordLocalInput(
            oldText = "a",
            newText = "ab",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        state.recordLocalInput(
            oldText = "ab",
            newText = "abc",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(3, 3),
            changes = listOf(LocalInputChange(newRange = TextRange(2, 3), oldRange = TextRange(2, 2))),
        )

        // 只收到最终 "abc" 的 layout
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0, compositionActive = false)

        val patch = state.latestPatch.value
        assertNotNull(
            "patch 应生成（旧实现 drainMatching 只返回最后一笔导致 patch 被丢）\n" +
                "Issue #694 评论 5691696678 问题1：drainMatchingChain 应合并连续链",
            patch,
        )
        assertEquals(
            "patch.oldLayout 应是空串（T0 = chain.first().oldText）",
            "",
            patch!!.oldLayout.result.layoutInput.text.text,
        )
        assertEquals(
            "patch.newLayout 应是 abc（Tn = chain.last().newText）",
            "abc",
            patch.newLayout.result.layoutInput.text.text,
        )
    }

    /**
     * 问题1 回归：快速 Backspace 删除链也能合并。
     *
     * 场景：recordLocalInput("abc"->"ab"), recordLocalInput("ab"->"a") 两笔，
     * 只调一次 onAuthoritativeLayout("a" 的 layout)。
     */
    @Test
    fun fastBackspaceChain_shouldNotDropPatch() {
        val layouts = captureLayouts("abc", "a")
        val state = ComposeEditorVisualState(targetId = "issue694-local-backspace")

        // 设置基线：lastPresentedLayout = "abc"
        state.onAuthoritativeLayout(layouts[0], TextRange(3, 3), 0)

        // 两笔快速 Backspace，中间 layout 被跳过
        state.recordLocalInput(
            oldText = "abc",
            newText = "ab",
            oldSelection = TextRange(3, 3),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(2, 2), oldRange = TextRange(2, 3))),
        )
        state.recordLocalInput(
            oldText = "ab",
            newText = "a",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 1), oldRange = TextRange(1, 2))),
        )

        // 只收到最终 "a" 的 layout
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0, compositionActive = false)

        val patch = state.latestPatch.value
        assertNotNull("删除链 patch 应生成", patch)
        assertEquals(
            "patch.oldLayout 应是 abc",
            "abc",
            patch!!.oldLayout.result.layoutInput.text.text,
        )
        assertEquals(
            "patch.newLayout 应是 a",
            "a",
            patch.newLayout.result.layoutInput.text.text,
        )
        assertTrue(
            "deletedUnits 应非空（删除了 bc）",
            patch.deletedUnits.isNotEmpty(),
        )
    }

    /**
     * 问题2 核心回归：本地输入后接 Undo，coordinator 基线已推进，Undo patch 能生成。
     *
     * 场景：本地输入 "" -> "a" + onAuthoritativeLayout("a")，然后 Undo intent "a" -> "" + onAuthoritativeLayout("")。
     *
     * 旧实现本地输入命中后直接 return，frameCoordinator.lastConsumed 还停在 ""，
     * Undo intent 的 pending.baseText("a") 与 lastConsumed.text("") 对不上，tryBuildPatch 返回 Empty。
     * 新实现 observePresentedLayout 推进 lastConsumed 到 "a"，Undo patch 能生成。
     */
    @Test
    fun localInputThenUndo_coordinatorBaselineAdvanced_undoPatchGenerated() {
        val layouts = captureLayouts("", "a", "")
        val state = ComposeEditorVisualState(targetId = "issue694-local-undo")

        // 设置基线：lastPresentedLayout = ""
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 本地输入 "" -> "a"
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0, compositionActive = false)

        val localPatch = state.latestPatch.value
        assertNotNull("本地输入 patch 应生成", localPatch)

        // Undo: "a" -> ""（Core 驱动）
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 100L,
                baseRevision = 1L,
                newRevision = 0L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(0, 1)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 1, newStart = 0, newEnd = 0),
                expectedOldText = "a",
                expectedNewText = "",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(0, 0), 0, compositionActive = false)

        val undoPatch = state.latestPatch.value
        assertNotNull(
            "Undo patch 应生成（frameCoordinator.lastConsumed 已通过 observePresentedLayout 推进到 a）\n" +
                "Issue #694 评论 5691696678 问题2：本地输入后 coordinator 基线必须推进",
            undoPatch,
        )
        assertTrue(
            "Undo patch 应包含 coreTransactionId=100",
            undoPatch!!.coreTransactionIds.contains(100L),
        )
    }

    /**
     * 问题3 核心回归：同一 VSync 多笔本地输入保留 ordered insertedUnits/cursor path。
     *
     * 场景：三笔本地输入 "" -> "a" -> "ab" -> "abc"，每笔都配对 layout，3 个 patch 入队，
     * 同一 VSync drainPendingPatchesAtFrame 消费。
     *
     * 旧实现 batch 合成后从补集算 insertedUnits = [TextRange(0,3)]（单个 [0,3)），
     * abc 作为整体淡入，不保留 a/b/c 吐字顺序；cursor 只留最后一笔路径。
     * 新实现用通用 stage-map 版本合成，insertedUnits = [TextRange(0,1), TextRange(1,2), TextRange(2,3)]，
     * 保留 a/b/c 吐字顺序；cursor path 用最终存活 ordered units 重新生成。
     */
    @Test
    fun sameVsyncMultipleLocalPatches_preserveOrderedInsertedUnitsAndCursorPath() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val state = ComposeEditorVisualState(targetId = "issue694-local-batch")

        // 设置基线
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 三笔本地输入，每笔都配对 layout（3 个 patch 入队）
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0, compositionActive = false)

        state.recordLocalInput(
            oldText = "a",
            newText = "ab",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0, compositionActive = false)

        state.recordLocalInput(
            oldText = "ab",
            newText = "abc",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(3, 3),
            changes = listOf(LocalInputChange(newRange = TextRange(2, 3), oldRange = TextRange(2, 2))),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(3, 3), 0, compositionActive = false)

        val pendingSize = pendingPatchesSize(state)
        assertTrue(
            "drain 前 pendingPatches 应有 3 个 patch，实际=$pendingSize",
            pendingSize == 3,
        )

        // 同一 VSync 消费
        val applied = state.drainPendingPatchesAtFrame(0L)
        assertEquals(
            "应只 applyPatch 一次（batch 合成）",
            1,
            applied.size,
        )

        val framePatch = applied[0]
        assertTrue(
            "insertedUnits 应保留 a/b/c 顺序（3 个 unit），实际=${framePatch.insertedUnits}\n" +
                "Issue #694 评论 5691696678 问题3：batch 合成不应把多字符压成单个 [0,3)",
            framePatch.insertedUnits.size == 3,
        )
        // 验证顺序：[0,1), [1,2), [2,3)
        assertEquals(TextRange(0, 1), framePatch.insertedUnits[0])
        assertEquals(TextRange(1, 2), framePatch.insertedUnits[1])
        assertEquals(TextRange(2, 3), framePatch.insertedUnits[2])

        assertNotNull(
            "cursorMotionPath 应非空",
            framePatch.cursorMotionPath,
        )
        assertTrue(
            "cursorMotionPath 应有多个点（不只 1 个），实际=${framePatch.cursorMotionPath?.points?.size}\n" +
                "Issue #694 评论 5691696678 问题3：cursor path 不应只拿 last.cursorMotionPath",
            (framePatch.cursorMotionPath?.points?.size ?: 0) > 1,
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
}
