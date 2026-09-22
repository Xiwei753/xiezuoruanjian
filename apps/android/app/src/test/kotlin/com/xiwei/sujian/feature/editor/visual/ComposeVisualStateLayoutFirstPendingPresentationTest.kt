package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #737 评论 5784705864 缺口1测试 — layout 先到、fact 后到时的 pending presentation ownership。
 *
 * 场景：第一笔 "" → "a" 正常完成后，第二笔 "a" → "ab" 的 layout 先到、fact 未到。
 * 此时 BasicTextField 已是 "ab" 正文，drawContent() 会裸画最终态。
 *
 * 期望（缺口1修复）：onAuthoritativeLayout("ab") 在 FrameUpdate.Empty 分支建立 pending presentation —
 * drawSnapshot().motionSample != null、hiddenRanges 覆盖新字 "b" 的 range [1,2)。
 * fact 到达后 pendingPresentation 原子升级成 prepared motion（pendingPresentation == null）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength")
class ComposeVisualStateLayoutFirstPendingPresentationTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * layout-first 时 drawSnapshot 应携带 pending presentation ownership，
     * fact 到达后 pendingPresentation 清空、prepared motion 接管。
     */
    @Test
    fun layoutFirst_factPending_drawSnapshotHasPendingOwnership() {
        val layouts = captureLayouts("", "a", "ab")
        val state = ComposeEditorVisualState(targetId = "737-layout-first")

        // === 第一笔："" -> "a"（正常完成，建立 baseline presentation）===
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
        // 第一笔完成后 drawSnapshotState.layout = "a" layout（applyPreparedMotionFromPatch 设）
        val snapAfterFirst = state.drawSnapshot()
        assertNotNull("第一笔完成后 drawSnapshot.layout 应非 null", snapAfterFirst.layout)

        // === 第二笔 layout-first："a" -> "ab" 的 layout 先到，fact 未到 ===
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        // 触发缺口1修复：Empty 分支用 drawSnapshotState.layout（"a" layout）当 oldLayout，
        // buildPendingPresentation("a", "ab") 建立 pending presentation。
        val snap = state.drawSnapshot()
        assertNotNull("layout-first 时 motionSample 应非 null（pending presentation 接管）", snap.motionSample)
        assertTrue("pending sample 应有效", snap.motionSample!!.isValid)
        assertTrue(
            "hiddenRanges 应非空（新字 'b' 的 range 被裁）",
            snap.motionSample!!.hiddenRanges.isNotEmpty(),
        )
        assertEquals(
            "hiddenRanges 应覆盖新字 'b' 的 range [1,2)",
            TextRange(1, 2),
            snap.motionSample!!.hiddenRanges.first(),
        )
        // 反射断言 pendingPresentation != null
        val pendingBeforeFact = readPendingPresentation(state)
        assertNotNull("fact 到达前 pendingPresentation 应非 null", pendingBeforeFact)

        // === fact 到达："a" -> "ab" 配对生成 NewPatch，原子升级成 prepared motion ===
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
        // 反射断言 pendingPresentation == null（已升级成 prepared motion）
        val pendingAfterFact = readPendingPresentation(state)
        assertNull("fact 到达后 pendingPresentation 应清空（升级成 prepared motion）", pendingAfterFact)
        // prepared motion 的 progress=0 sample 应非 null
        val snap2 = state.drawSnapshot()
        assertNotNull("fact 到达后 motionSample 应非 null（prepared motion 接管）", snap2.motionSample)

        // === drain：start prepared motion ===
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)
        assertNotNull("drain + sample 后应有 motion sample", scene)
        assertTrue("motion sample 应有效", scene!!.isValid)
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

    /** 反射读取 ComposeEditorVisualState 的 private pendingPresentation 字段。 */
    private fun readPendingPresentation(state: ComposeEditorVisualState): Any? {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("pendingPresentation")
        field.isAccessible = true
        return field.get(state)
    }
}
