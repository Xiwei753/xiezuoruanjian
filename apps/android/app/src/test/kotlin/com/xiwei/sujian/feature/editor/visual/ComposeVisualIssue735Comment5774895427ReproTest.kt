package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #735 评论 5774895427：IME composition commit 时序竞态修复验证。
 *
 * 背景：CoreEditFactEvent 通过 `viewModelScope.launch { _editFactEvents.send(...) }` 异步发出，
 * 真实到达顺序可能是：
 * `IME commit -> TextFieldState 最终正文 -> onTextLayout(final layout) -> CoreEditFactEvent 稍后到`
 *
 * 旧实现：[ComposeEditorVisualState.onAuthoritativeLayout] 在 composition active->false 时调
 * `frameCoordinator.observePresentedLayout(snapshot)`，后者在 `pending == null` 时直接
 * `lastConsumed = presented`，把 committed baseline 提前推到新正文 B。
 * 随后 fact(A->B) 到达时 `pendingChain.baseText(A) != consumed.text(B)`，永远配不出 patch。
 *
 * 修复（Issue #735 评论 5774895427）：
 * - 删除 [ComposeVisualFrameCoordinator.observePresentedLayout] 入口和 `wasCompositionActive` 过渡同步。
 * - `compositionActive == false` 一律走 [ComposeVisualFrameCoordinator.onLayout]。
 * - `onEditFact` / `onLayout` 双向合流，无论谁先到都能配对生成 patch。
 *
 * 本测试直接测 [ComposeVisualFrameCoordinator]，覆盖两种到达顺序：
 * 1. fact 先到，layout 后到 -> 生成 patch。
 * 2. layout 先到，fact 后到 -> 生成 patch（旧实现会卡住）。
 * 3. composition cancel（layout 回到基线）-> 不生成 patch，几何基线正常更新。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("MaxLineLength")
class ComposeVisualIssue735Comment5774895427ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 顺序1：fact 先到，layout 后到。
     *
     * 1. onLayout("好") — 设基线 lastConsumed="好"
     * 2. onEditFact("好"->"你好") — pending 建起来；tryBuildPatch: latest===consumed，Empty
     * 3. onLayout("你好") — latest="你好"；tryBuildPatch: base="好"==consumed ✓, target="你好"==latest ✓ -> NewPatch
     */
    @Test
    fun factArrivesBeforeLayout_shouldBuildPatch() {
        val layouts = captureLayouts("好", "你好")
        val coordinator = ComposeVisualFrameCoordinator(targetId = "issue735-c5774895427-fact-first")

        // 1. 基线 layout
        coordinator.onLayout(ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0))

        // 2. fact 先到（Core 异步发出，可能先于最终 layout）
        val factFirst =
            coordinator.onEditFact(
                makeCompositionCommitFact(
                    coreTxnId = 100L,
                    baseRev = 1L,
                    newRev = 2L,
                    oldText = "好",
                    newText = "你好",
                ),
            )
        assertTrue(
            "fact 先到时 pending 尚无匹配 layout，应返回 Empty。实际=$factFirst",
            factFirst is FrameUpdate.Empty,
        )

        // 3. 最终 layout 后到
        val update = coordinator.onLayout(ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0))
        assertTrue(
            "fact 先到 + layout 后到：onLayout(\"你好\") 应与 pending(\"好\"->\"你好\") 配对生成 patch。\n" +
                "实际 update=$update\n" +
                "Issue #735 评论 5774895427：onEditFact/onLayout 双向合流应处理 fact-first 顺序",
            update is FrameUpdate.NewPatch,
        )
    }

    /**
     * 顺序2：layout 先到，fact 后到（旧实现会卡住的核心场景）。
     *
     * 1. onLayout("好") — 设基线 lastConsumed="好"
     * 2. onLayout("你好") — latest="你好"；pending==null 且 text 不同，不推进 lastConsumed；Empty
     * 3. onEditFact("好"->"你好") — pending 建起来；tryBuildPatch: base="好"==consumed ✓, target="你好"==latest ✓ -> NewPatch
     */
    @Test
    fun layoutArrivesBeforeFact_shouldBuildPatch() {
        val layouts = captureLayouts("好", "你好")
        val coordinator = ComposeVisualFrameCoordinator(targetId = "issue735-c5774895427-layout-first")

        // 1. 基线 layout
        coordinator.onLayout(ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0))

        // 2. 最终 layout 先到（IME commit 后 onTextLayout 先于 CoreEditFactEvent）
        val layoutFirst = coordinator.onLayout(ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0))
        assertTrue(
            "layout 先到时 pending 为空，应返回 Empty（不提前推进 committed baseline）。实际=$layoutFirst",
            layoutFirst is FrameUpdate.Empty,
        )

        // 3. fact 后到
        val update =
            coordinator.onEditFact(
                makeCompositionCommitFact(
                    coreTxnId = 200L,
                    baseRev = 1L,
                    newRev = 2L,
                    oldText = "好",
                    newText = "你好",
                ),
            )
        assertTrue(
            "layout 先到 + fact 后到：onEditFact(\"好\"->\"你好\") 应与已缓存 layout(\"你好\") 配对生成 patch。\n" +
                "实际 update=$update\n" +
                "Issue #735 评论 5774895427：旧实现 observePresentedLayout 会把 lastConsumed 提前推到 \"你好\"，" +
                "导致 fact 到达时 baseText(\"好\") != consumed(\"你好\")，永远配不出 patch",
            update is FrameUpdate.NewPatch,
        )
    }

    /**
     * 顺序3：composition cancel — 最终 layout 回到基线正文。
     *
     * 1. onLayout("好") — 设基线
     * 2. onLayout("好") — pending==null 且 text 相同，更新几何基线；不生成 patch
     *
     * 验证：cancel 不产生 patch，onLayout 现有逻辑已处理，不需要 observePresentedLayout。
     */
    @Test
    fun compositionCancel_layoutReturnsToBase_shouldNotBuildPatch() {
        val layouts = captureLayouts("好")
        val coordinator = ComposeVisualFrameCoordinator(targetId = "issue735-c5774895427-cancel")

        // 1. 基线 layout
        coordinator.onLayout(ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0))

        // 2. composition cancel：layout 回到 "好"（text 不变）
        val update = coordinator.onLayout(ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0))
        assertTrue(
            "composition cancel（layout 回到基线）不应生成 patch。实际=$update",
            update is FrameUpdate.Empty,
        )
    }

    // ==================== 辅助方法 ====================

    private fun makeCompositionCommitFact(
        coreTxnId: Long,
        baseRev: Long,
        newRev: Long,
        oldText: String,
        newText: String,
    ): EditorEditFact =
        EditorEditFact(
            cause = uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            coreTransactionId = coreTxnId,
            baseRevision = baseRev,
            newRevision = newRev,
            animationMode = AnimationMode.GLYPH_ANIMATION,
            durationMs = 100L,
            offsetMap = null,
            oldRanges = emptyList(),
            newRanges = listOf(TextRange(0, 1)),
            textKind = TextVisualKind.Insert,
            replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 1),
            expectedOldText = oldText,
            expectedNewText = newText,
            oldSelectionEndUtf16 = oldText.length,
            newSelectionEndUtf16 = newText.length,
        )

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
}
