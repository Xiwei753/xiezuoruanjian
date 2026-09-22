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
 * #694 评论 5692161955 复现测试 — 上一轮提交 ab91aaffb 修了 3 个问题后还剩的 3 个
 * 会直接影响真实输入/删除动画的问题：
 *
 * 问题1：一次 InputTransformation 提交多个字，仍然会整块吐出。
 *   ComposeLocalVisualRebase.composeLocalChainInsertedUnits()/composeLocalChainDeletedUnits()
 *   把 changedRangesFromOffsetMap 的 newRanges/oldRanges 直接当动画 unit，
 *   不能处理"一笔里多个 grapheme"。例如 "" -> "我们" 得到 1 个 [0,2) unit，timeline 整体吐出。
 *
 * 问题2：buildLocalInputPatch() 把所有本地输入硬编码成 CLUSTER_ANIMATION。
 *   ComposeEditorVisualState.kt 中 animationMode = AnimationMode.CLUSTER_ANIMATION，
 *   丢弃 Core 已做好的视觉分类。Core 规则：0 cluster -> SYSTEM_SUPPRESSED；
 *   含换行 -> LINE_REFLOW_ANIMATION；复杂 grapheme -> CLUSTER_ANIMATION；
 *   <= 8 cluster -> GLYPH_ANIMATION；> 8 cluster -> RUN_ANIMATION。
 *
 * 问题3：composition 活跃时不应该把 Core/external coordinator 基线推进到 preedit。
 *   onAuthoritativeLayout composition 分支调用 frameCoordinator.observePresentedLayout(snapshot)，
 *   把 coordinator.lastConsumed 推到未提交给 Core 的 preedit，导致 Undo 后 external patch 卡死。
 *   另外 observePresentedLayout 只要 pending != null 就不推进，在
 *   "本地输入完成 -> external intent 先到 -> 本地 layout 后到"顺序下会再次卡住。
 *
 * 本测试在当前实现下应 **FAIL**，体现 3 个 bug。修复后应 PASS。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength")
class ComposeVisualIssue694Comment5692161955ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 问题1：一笔多 grapheme 整块吐出 ====================

    // ==================== 问题2：buildLocalInputPatch 硬编码 CLUSTER_ANIMATION ====================

    // ==================== 问题3：composition 活跃时误推 coordinator 基线 ====================

    /**
     * 问题3 核心复现 b：observePresentedLayout 在 pending != null 但
     * pending.baseText == presented.text 时应推进 lastConsumed，否则
     * "本地输入完成 -> external intent 先到 -> 本地 layout 后到"顺序会卡住。
     *
     * 场景（直接测 ComposeVisualFrameCoordinator）：
     * 1. onLayout("A") — lastConsumed = "A"
     * 2. onEditFact(baseText="AB", targetText="ABC") — pending != null
     * 3. observePresentedLayout("AB") — pending.baseText="AB" == presented.text="AB"
     *    当前实现：pending != null 时不推进，lastConsumed 保持 "A"
     * 4. onLayout("ABC") — tryBuildPatch
     *    当前实现：lastConsumed="A" != pending.baseText="AB"，返回 Empty，卡住
     *    期望：返回 NewPatch
     *
     * 期望：第 4 步 onLayout("ABC") 返回 FrameUpdate.NewPatch。
     * 当前实现：返回 FrameUpdate.Empty，卡住。FAIL。
     */
    @Test
    fun observePresentedLayout_pendingBaseTextMatchesPresented_shouldAdvanceAndGeneratePatch() {
        val layouts = captureLayouts("A", "AB", "ABC")
        val coordinator = ComposeVisualFrameCoordinator(targetId = "issue694-c5692161955-p3b")

        // 1. onLayout("A") — 设基线 lastConsumed = "A"
        coordinator.onLayout(ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0))

        // 2. external intent 先到: "AB" -> "ABC"（pending != null）
        coordinator.onEditFact(
            EditorEditFact(
                cause = uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC,
                operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
                coreTransactionId = 300L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationMode.GLYPH_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(1, 2)),
                newRanges = listOf(TextRange(1, 3)),
                textKind = TextVisualKind.Insert,
                replaceBounds = VisualReplaceBounds(oldStart = 1, oldEnd = 2, newStart = 1, newEnd = 3),
                expectedOldText = "AB",
                expectedNewText = "ABC",
            ),
        )

        // 3. 本地 layout 后到: observePresentedLayout("AB")
        //    pending.baseText="AB" == presented.text="AB"
        coordinator.observePresentedLayout(ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0))

        // 4. target layout "ABC" 到达: onLayout("ABC")
        val update = coordinator.onLayout(ComposeLayoutSnapshot(layouts[2], TextRange(3, 3), 0))

        assertTrue(
            "问题3：observePresentedLayout 在 pending.baseText == presented.text 时应推进 lastConsumed，" +
                "后续 target layout 到达后 patch 应生成（FrameUpdate.NewPatch）\n" +
                "当前实现：observePresentedLayout 只要 pending != null 就不推进，lastConsumed 保持 \"A\"，" +
                "onLayout(\"ABC\") 时 pending.baseText=\"AB\" != lastConsumed.text=\"A\"，" +
                "tryBuildPatch 返回 Empty，patch 卡死。实际 update=$update\n" +
                "Issue #694 评论 5692161955 问题3：observePresentedLayout 应补并发顺序 " +
                "pending?.baseText == presented.text 时也推进 lastConsumed",
            update is FrameUpdate.NewPatch,
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
     * 通过反射访问 ComposeEditorVisualState 的 private pendingPatches 队列，
     * 查找包含指定 coreTransactionId 的 patch。
     *
     * Issue #723 评论 5750100004：pendingPatches 元素类型从 ComposeVisualPatch
     * 改为 PendingPatch（携带 sequence），需要通过反射读 .patch 字段。
     */
    private fun findPatchWithCoreTxnId(
        state: ComposeEditorVisualState,
        coreTxnId: Long,
    ): ComposeVisualPatch? {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("pendingPatches")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val deque = field.get(state) as kotlin.collections.ArrayDeque<*>
        // 先在 pendingPatches 队列里找 — 元素是 PendingPatch，需要反射读 .patch
        for (item in deque) {
            val patchField = item?.javaClass?.getDeclaredField("patch")
            if (patchField != null) {
                patchField.isAccessible = true
                val patch = patchField.get(item) as ComposeVisualPatch
                if (patch.coreTransactionIds.contains(coreTxnId)) return patch
            }
        }
        // 再看 latestPatch（可能已被 drain 或就是最新）
        val latest = state.latestPatch.value
        if (latest != null && latest.coreTransactionIds.contains(coreTxnId)) return latest
        return null
    }
}
