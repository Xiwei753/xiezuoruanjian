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
 * Issue #735 评论 5775326365：IME preedit 已经是最终文字时，commit 后 coordinator 仍能拿到这份 layout。
 *
 * 背景：preedit 已经显示成最终文字 B（`A -> composition layout B`），此时 VisualState 已有 B 的真实
 * TextLayoutResult，但旧实现 compositionActive=true 分支不把这份 layout 交给 frameCoordinator。
 * IME commit 后通常只是清掉 composition 标记，文字仍是同一个 B，Compose 不保证 commit 后一定重新算
 * 一次 layout；即使又回调一次 B，fingerprintUnchanged 也会直接 return，不调 frameCoordinator.onLayout。
 * 于是 coordinator baseline=A、latest=A、VisualState _latestLayout=B，fact(A->B) 进入 pending 后
 * coordinator 没有 B layout 可以配，patch 卡住。
 *
 * 修复（Issue #735 评论 5775326365）：
 * - 新增 [ComposeVisualFrameCoordinator.onProvisionalLayout]：只缓存 latest，不推进 lastConsumed，
 *   不生成 patch。
 * - [ComposeEditorVisualState.onAuthoritativeLayout] 的 compositionActive=true 分支把 preedit layout
 *   交给 onProvisionalLayout，让 coordinator 知道平台已算出的候选几何 B，但 committed baseline 仍保持 A。
 *
 * 本测试直接测 [ComposeVisualFrameCoordinator]，覆盖评论列出的三条 commit 路径：
 * 1. fact 先到（commit 后没有新的 onTextLayout 回调）-> 用 provisional 缓存的 B 配对生成 patch。
 * 2. final layout 先到（commit 后又回调一次 B）-> onLayout(B) 不提前推进 A baseline，fact 后到照样配。
 * 3. provisional 不推进 baseline -> 连续 provisional 不会把 lastConsumed 偷偷推到 B。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("MaxLineLength", "StringLiteralDuplication")
class ComposeVisualIssue735Comment5775326365ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 路径1（核心）：preedit 是最终文字 B，commit 后没有新的 onTextLayout 回调，
     * fact(A->B) 直接拿 provisional 缓存的 latest=B 配对生成 patch。
     *
     * 1. onLayout("好") — 设基线 lastConsumed="好"
     * 2. onProvisionalLayout("你好") — latest="你好"；不推进 lastConsumed；Empty
     * 3. onEditFact("好"->"你好") — pending 建起来；tryBuildPatch: base="好"==consumed ✓, target="你好"==latest ✓ -> NewPatch
     */
    @Test
    fun preeditIsFinalText_noLayoutAfterCommit_factPairsWithProvisionalLayout() {
        val layouts = captureLayouts("好", "你好")
        val coordinator = ComposeVisualFrameCoordinator(targetId = "issue735-c5775326365-fact-only")

        // 1. 基线 layout（正文 A="好"）
        coordinator.onLayout(ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0))

        // 2. IME preedit 已经是最终文字 B="你好"（compositionActive=true 走 onProvisionalLayout）
        val provisional =
            coordinator.onProvisionalLayout(ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0))
        assertTrue(
            "provisional layout 只缓存 latest，不生成 patch，应返回 Empty。实际=$provisional",
            provisional is FrameUpdate.Empty,
        )

        // 3. IME commit — Compose 不保证 commit 后一定重新算 layout，fact(A->B) 可能是唯一驱动
        val update =
            coordinator.onEditFact(
                makeCompositionCommitFact(
                    coreTxnId = 300L,
                    baseRev = 1L,
                    newRev = 2L,
                    oldText = "好",
                    newText = "你好",
                ),
            )
        assertTrue(
            "preedit 是最终文字 + commit 后无新 layout 回调：onEditFact(\"好\"->\"你好\") 应与 " +
                "provisional 缓存的 layout(\"你好\") 配对生成 patch。\n" +
                "实际 update=$update\n" +
                "Issue #735 评论 5775326365：provisional layout 必须让 coordinator 知道平台已算出的 B 几何，" +
                "否则 fact 到达时 latest 仍是 A，patch 卡住",
            update is FrameUpdate.NewPatch,
        )
    }

    /**
     * 路径2：preedit 是最终文字 B，commit 后又回调了一次 B（onTextLayout），
     * onLayout(B) 不提前推进 A baseline，fact 后到照样配。
     *
     * 1. onLayout("好") — 设基线 lastConsumed="好"
     * 2. onProvisionalLayout("你好") — latest="你好"；不推进 lastConsumed
     * 3. onLayout("你好") — latest="你好"；lastConsumed="好" 不为 null 跳过初始化；
     *    pending==null 且 text 不同不推进；tryBuildPatch pending==null -> Empty
     * 4. onEditFact("好"->"你好") — base="好"==consumed ✓, target="你好"==latest ✓ -> NewPatch
     */
    @Test
    fun preeditIsFinalText_layoutArrivesAfterCommit_thenFactArrives_buildsPatch() {
        val layouts = captureLayouts("好", "你好")
        val coordinator = ComposeVisualFrameCoordinator(targetId = "issue735-c5775326365-layout-then-fact")

        // 1. 基线 layout
        coordinator.onLayout(ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0))

        // 2. preedit 是最终文字 B（provisional 缓存）
        coordinator.onProvisionalLayout(ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0))

        // 3. commit 后又回调了一次 B（onTextLayout）— 不应提前推进 A baseline
        val layoutAfterCommit = coordinator.onLayout(ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0))
        assertTrue(
            "commit 后的 onLayout(\"你好\") 在 fact 到达前应返回 Empty（pending 为空）。实际=$layoutAfterCommit",
            layoutAfterCommit is FrameUpdate.Empty,
        )

        // 4. fact 后到 — 应与 latest=B 配对
        val update =
            coordinator.onEditFact(
                makeCompositionCommitFact(
                    coreTxnId = 400L,
                    baseRev = 1L,
                    newRev = 2L,
                    oldText = "好",
                    newText = "你好",
                ),
            )
        assertTrue(
            "commit 后 onLayout(B) 不提前推进 A baseline，fact 后到应配对生成 patch。\n" +
                "实际 update=$update\n" +
                "Issue #735 评论 5775326365：onLayout 不应把 lastConsumed 从 A 偷偷推到 B",
            update is FrameUpdate.NewPatch,
        )
    }

    /**
     * 路径3：provisional layout 绝不推进 committed baseline。
     *
     * 连续多次 onProvisionalLayout(B) 后，lastConsumed 仍为 A，
     * onEditFact(A->B) 仍能配对（说明 baseline 没被偷推到 B）。
     *
     * 1. onLayout("好") — 设基线 lastConsumed="好"
     * 2. onProvisionalLayout("你好") x2 — latest="你好"；lastConsumed 仍="好"
     * 3. onEditFact("好"->"你好") — base="好"==consumed ✓ -> NewPatch（若 baseline 被偷推到"你好"则配不出）
     */
    @Test
    fun repeatedProvisionalLayout_doesNotAdvanceCommittedBaseline() {
        val layouts = captureLayouts("好", "你好")
        val coordinator = ComposeVisualFrameCoordinator(targetId = "issue735-c5775326365-no-baseline-advance")

        // 1. 基线 layout
        coordinator.onLayout(ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0))

        // 2. 连续 provisional layout（模拟 preedit 期间多次 onTextLayout 回调同一份 B）
        repeat(2) {
            val provisional =
                coordinator.onProvisionalLayout(ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0))
            assertTrue(
                "provisional layout 不应生成 patch。实际=$provisional",
                provisional is FrameUpdate.Empty,
            )
        }

        // 3. fact 到达 — 若 provisional 偷推了 baseline 到 "你好"，则 base="好" != consumed="你好"，配不出 patch
        val update =
            coordinator.onEditFact(
                makeCompositionCommitFact(
                    coreTxnId = 500L,
                    baseRev = 1L,
                    newRev = 2L,
                    oldText = "好",
                    newText = "你好",
                ),
            )
        assertTrue(
            "连续 provisional layout 不应推进 committed baseline，fact(A->B) 应配对生成 patch。\n" +
                "实际 update=$update\n" +
                "Issue #735 评论 5775326365：onProvisionalLayout 绝不碰 lastConsumed，" +
                "不能重蹈 observePresentedLayout 偷推 baseline 的覆辙",
            update is FrameUpdate.NewPatch,
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
