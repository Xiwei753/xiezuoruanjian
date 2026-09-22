package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #737 评论 5784705864 缺口2测试 — 同一帧多笔 patch 时 prepared motion 应是 batch 合成结果。
 *
 * 场景：第一笔 "" → "a" 后未 drain，第二笔 "a" → "ab" 到达。pendingPatches=[P1, P2]。
 *
 * 旧实现（缺口2）：每次 NewPatch 立刻 applyPreparedMotionFromPatch(update.patch)（单笔），
 * 连续 P1, P2 → activeMotion=prepared(P2)。drain 里 `if (existingMotion.isPrepared) start`
 * 直接 start P2，跳过 ComposeVisualPatchBatch.compose([P1, P2])。prepared motion 的
 * hiddenRanges 只覆盖第二笔 [1,2)，不覆盖第一笔 [0,1)。
 *
 * 期望（缺口2修复）：第二笔 NewPatch 时对整个 pending queue 合成，activeMotion=prepared(compose([P1,P2]))。
 * prepared motion 的 hiddenRanges 覆盖合成范围 [0,2)（包含从 0 开始的 range，证明覆盖第一笔的 "a"）。
 * drain 时 preparedMotionSequence == consumedMaxSequence → 直接 start 合成 motion。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength")
class ComposeVisualStateMultiPatchBatchPreparedTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 两笔 patch 同一帧到达时，prepared motion 应是 batch 合成结果
     * （hiddenRanges 覆盖两笔合并范围，不是只最后一笔）。
     */
    @Test
    fun twoPatchesSameFrame_preparedMotionIsBatchComposed() {
        val layouts = captureLayouts("", "a", "ab")
        val state = ComposeEditorVisualState(targetId = "737-multi-patch-batch")

        // === 第一笔："" -> "a"（未 drain，patch 留在 pendingPatches）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "a",
                newRange = TextRange(0, 1),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 1),
            ),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        // 此时 pendingPatches=[P1]，activeMotion=prepared(compose([P1])=P1)，preparedMotionSequence=0
        val pendingSizeAfterFirst = pendingPatchesSize(state)
        assertTrue(
            "第一笔后 pendingPatches 应有 1 个 patch，实际=$pendingSizeAfterFirst",
            pendingSizeAfterFirst == 1,
        )

        // === 第二笔："a" -> "ab"（fact 先到，layout 后到配对生成 P2，未 drain）===
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 2L,
                baseRev = 1L,
                newRev = 2L,
                oldText = "a",
                newText = "ab",
                newRange = TextRange(1, 2),
                replaceBounds = VisualReplaceBounds(1, 1, 1, 2),
                offsetMap =
                    VisualOffsetMap(
                        entries = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                    ),
            ),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        // 此时 pendingPatches=[P1, P2]，activeMotion=prepared(compose([P1, P2]))，preparedMotionSequence=1
        val pendingSizeAfterSecond = pendingPatchesSize(state)
        assertTrue(
            "第二笔后 pendingPatches 应有 2 个 patch，实际=$pendingSizeAfterSecond",
            pendingSizeAfterSecond == 2,
        )

        // 关键断言：drain 前 prepared motion 的 hiddenRanges 覆盖合成范围。
        // 合成 P1(""->"a") + P2("a"->"ab") = "" -> "ab"，inserted 覆盖 [0,2)。
        // 若只 prepared(P2)（"a"->"ab"），hiddenRanges 只 [1,2) — 从 1 开始。
        // 合成 motion 的 hiddenRanges 包含从 0 开始的 range，证明覆盖第一笔的 "a"。
        val snap = state.drawSnapshot()
        assertNotNull("drain 前 motionSample 应非 null（prepared motion）", snap.motionSample)
        val hidden = snap.motionSample!!.hiddenRanges
        assertTrue("hiddenRanges 应非空", hidden.isNotEmpty())
        assertTrue(
            "合成 motion 的 hiddenRanges 应包含从 0 开始的 range（覆盖第一笔 'a'），" +
                "实际 hiddenRanges=$hidden — 若只 prepared(P2) 则 hiddenRanges 从 1 开始",
            hidden.any { it.start == 0 },
        )

        // === drain：preparedMotionSequence == consumedMaxSequence → start 合成 motion ===
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)
        assertNotNull("drain + sample 后应有 motion sample", scene)
        assertTrue("motion sample 应有效", scene!!.isValid)
        // 动画进行中，新字仍 hidden
        assertTrue(
            "动画进行中 hiddenRanges 应非空（新字仍 hidden）",
            scene!!.hiddenRanges.isNotEmpty(),
        )
    }

    // ==================== 辅助方法 ====================

    private fun makeInsertIntent(
        coreTxnId: Long,
        baseRev: Long,
        newRev: Long,
        oldText: String,
        newText: String,
        newRange: TextRange,
        replaceBounds: VisualReplaceBounds,
        offsetMap: VisualOffsetMap? = null,
    ): EditorEditFact =
        EditorEditFact(
            cause = uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            coreTransactionId = coreTxnId,
            baseRevision = baseRev,
            newRevision = newRev,
            animationMode = AnimationMode.CLUSTER_ANIMATION,
            durationMs = 100L,
            offsetMap = offsetMap,
            oldRanges = emptyList(),
            newRanges = listOf(newRange),
            textKind = TextVisualKind.Insert,
            replaceBounds = replaceBounds,
            expectedOldText = oldText,
            expectedNewText = newText,
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

    /** 反射访问 ComposeEditorVisualState 的 private pendingPatches 队列大小。 */
    private fun pendingPatchesSize(state: ComposeEditorVisualState): Int {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("pendingPatches")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val deque = field.get(state) as kotlin.collections.ArrayDeque<*>
        return deque.size
    }
}
