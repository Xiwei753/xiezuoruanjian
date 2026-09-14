package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Constraints
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
 * #684 评论 5668108597 问题1 回归测试 — masterProgress 事务身份守卫。
 *
 * 场景：快速连续输入时 A 的旧 LaunchedEffect 可能在 B 已成为 active 后继续写全局 progress。
 *
 * 旧实现：`reportProgress(progress: Float)` 没有 transactionId，
 * A 的旧 LaunchedEffect 在 B 已接管后仍能把全局 _masterProgress 写成 0.9f，
 * 下一笔 rebase 物化 startFrame 拿到错误进度。
 *
 * 修复：
 * 1. `reportProgress(transactionId, progress)` 加 ID 守卫 — 不匹配直接 return。
 * 2. `applyFrameUpdate(NewTransaction)` 在设置 `_activeTransaction` 之前同步重置 `_masterProgress=0f`。
 *
 * 本测试验证：
 * - A progress=0.7 → 生成 B → 断言 B 接管后 `masterProgress==0f`。
 * - 模拟迟到的 `reportProgress(A.id, 0.9f)`，断言 B 的 progress 仍为 0。
 * - `reportProgress(B.id, 0.2f)` 才允许写入。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication")
class ComposeVisualMasterProgressGuardTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 新事务接管时 masterProgress 同步重置为 0f，且旧事务迟到的 reportProgress 不污染新事务。
     *
     * 场景：
     * - 生成事务 A（Insert "" → "abc"）。
     * - A 报告 progress=0.7f。
     * - 生成事务 B（Insert "abc" → "abcde"）— B 接管时 _masterProgress 应重置为 0f。
     * - 模拟 A 迟到的 reportProgress(A.id, 0.9f) — 应被 ID 守卫拒绝。
     * - reportProgress(B.id, 0.2f) — 应允许写入。
     *
     * 断言：
     * - B 接管后 masterProgress==0f。
     * - reportProgress(A.id, 0.9f) 后 masterProgress 仍为 0f（A 已不是 active）。
     * - reportProgress(B.id, 0.2f) 后 masterProgress==0.2f。
     */
    @Test
    fun newTransaction_resetsMasterProgress_andStaleReportIsRejected() {
        val layouts = captureLayouts("", "abc", "abcde")
        val state = ComposeEditorVisualState(targetId = "test-target-master-progress-guard")

        // === 生成事务 A（Insert "" → "abc"）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 3),
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val txA = state.activeTransaction.value
        assertNotNull("事务 A 应生成", txA)
        val txAId = txA?.id ?: return

        // A 报告 progress=0.7f — A 是 active，应写入。
        state.reportProgress(txAId, 0.7f)
        assertEquals(
            "A 报告 0.7f 后 masterProgress 应为 0.7f",
            0.7f,
            state.masterProgress.value,
            0.001f,
        )

        // === A 到 1f 前生成事务 B（Insert "abc" → "abcde"）===
        // B 接管时 _masterProgress 应同步重置为 0f（不等 Compose 下一帧）。
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(3, 5)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 3, oldEnd = 3, newStart = 3, newEnd = 5),
                expectedOldText = "abc",
                expectedNewText = "abcde",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(5, 5), 0)
        val txB = state.activeTransaction.value
        assertNotNull("事务 B 应生成", txB)
        val txBId = txB?.id ?: return

        // 核心断言 1：B 接管后 masterProgress 同步重置为 0f。
        assertEquals(
            "B 接管后 masterProgress 应为 0f，实际=${state.masterProgress.value}\n" +
                "#684 评论 5668108597 问题1：visual state 接管新事务时同步重置 _masterProgress=0f，" +
                "不等 Compose 下一帧再靠 Animatable.snapTo(0f) 修正",
            0f,
            state.masterProgress.value,
            0.001f,
        )

        // 核心断言 2：模拟 A 迟到的 reportProgress(A.id, 0.9f) — 应被 ID 守卫拒绝。
        state.reportProgress(txAId, 0.9f)
        assertEquals(
            "reportProgress(A.id, 0.9f) 后 masterProgress 应仍为 0f（A 已不是 active），实际=${state.masterProgress.value}\n" +
                "#684 评论 5668108597 问题1：旧事务迟到的 progress 不应污染新事务的 _masterProgress",
            0f,
            state.masterProgress.value,
            0.001f,
        )

        // 核心断言 3：reportProgress(B.id, 0.2f) 才允许写入。
        state.reportProgress(txBId, 0.2f)
        assertEquals(
            "reportProgress(B.id, 0.2f) 后 masterProgress 应为 0.2f",
            0.2f,
            state.masterProgress.value,
            0.001f,
        )

        // 确认 B 仍是当前活跃事务（A 迟到完成不应清掉 B）。
        assertEquals(
            "B 应仍是当前活跃事务",
            txBId,
            state.activeTransaction.value?.id,
        )
    }

    /**
     * 对照：reportProgress(当前事务 id, progress) 应正常写入。
     *
     * 验证 ID 守卫在匹配时确实生效，不是永远 no-op。
     */
    @Test
    fun reportProgress_matchingId_writesMasterProgress() {
        val layouts = captureLayouts("", "abc")
        val state = ComposeEditorVisualState(targetId = "test-target-matching-progress")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 3),
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val txId = state.activeTransaction.value?.id ?: return

        // 新事务接管时 masterProgress 已重置为 0f。
        assertEquals(
            "事务生成后 masterProgress 应为 0f",
            0f,
            state.masterProgress.value,
            0.001f,
        )

        state.reportProgress(txId, 0.5f)
        assertEquals(
            "reportProgress(匹配 id, 0.5f) 后 masterProgress 应为 0.5f",
            0.5f,
            state.masterProgress.value,
            0.001f,
        )
    }

    /**
     * reportProgress 在无活跃事务时应被拒绝（不写入）。
     */
    @Test
    fun reportProgress_noActiveTransaction_isRejected() {
        val state = ComposeEditorVisualState(targetId = "test-target-no-active")

        // 无活跃事务时 reportProgress 应直接 return（_activeTransaction.value?.id == null != transactionId）。
        state.reportProgress(transactionId = 999L, progress = 0.5f)
        assertEquals(
            "无活跃事务时 reportProgress 不应写入",
            0f,
            state.masterProgress.value,
            0.001f,
        )
    }

    /**
     * reportProgress 的 progress 应被 coerceIn(0f, 1f)。
     */
    @Test
    fun reportProgress_isCoercedToUnitRange() {
        val layouts = captureLayouts("", "abc")
        val state = ComposeEditorVisualState(targetId = "test-target-coerce")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 3),
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val txId = state.activeTransaction.value?.id ?: return

        state.reportProgress(txId, 1.5f)
        assertEquals(
            "reportProgress(1.5f) 应被 coerce 到 1f",
            1f,
            state.masterProgress.value,
            0.001f,
        )

        state.reportProgress(txId, -0.5f)
        assertEquals(
            "reportProgress(-0.5f) 应被 coerce 到 0f",
            0f,
            state.masterProgress.value,
            0.001f,
        )
    }

    /**
     * 用 [rememberTextMeasurer] 在 Compose 测试环境里构造真实 [TextLayoutResult]。
     * maxWidth=1000 避免折行，让 bounds 计算确定。
     */
    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        constraints = Constraints(maxWidth = 1000),
                    ),
                )
            }
        }
        return results
    }
}
