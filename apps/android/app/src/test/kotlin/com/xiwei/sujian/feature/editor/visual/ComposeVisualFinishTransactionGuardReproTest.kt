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
 * #684 评论 5667483662 问题2 回归测试 — 旧事务迟到完成回调不清新事务。
 *
 * 场景：快速连续输入时：A 刚到 1f，B 已生成并写进 visual state，
 * 随后 A 的完成回调执行。
 *
 * 旧实现：overlay 分两步 `completeActiveTransaction(A)` + `clearAnimation()`。
 * `completeActiveTransaction(A)` 因 coordinator 已是 B 安全返回，
 * 但 `clearAnimation()` 没有 transactionId，无条件清空 _hiddenRanges/_activeIntent/
 * _visualCursorSnapshot/_activeTransaction/_masterProgress，把 B 的 visual state 清空。
 * 动画偶发消失，B 的 hiddenRanges 被提前解除。
 *
 * 修复：收口成 `finishTransaction(transactionId)`，先检查 _activeTransaction.id == transactionId
 * 才生效。overlay 到 1f 只调这一个方法。
 *
 * 本测试验证：生成 A → 生成 B → 模拟 A 迟到完成（finishTransaction(A.id)），
 * B 仍然保持在 _activeTransaction，B 的 hiddenRanges/activeIntent 不被 A 清掉。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication")
class ComposeVisualFinishTransactionGuardReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 旧事务 A 迟到完成回调不应清掉新事务 B 的 visual state。
     *
     * 场景：
     * - 生成事务 A（Insert "" → "abc"）。
     * - A 到 1f 前生成事务 B（Insert "abc" → "abcde"）。
     * - 调用 finishTransaction(A.id)（模拟 A 迟到完成）。
     *
     * 断言：
     * - activeTransaction.value?.id == B.id（B 仍是当前活跃事务）。
     * - hiddenRanges.value 仍是 B 的（非空，[0,5)）。
     * - activeIntent.value 仍是 B 的（expectedNewText == "abcde"）。
     */
    @Test
    fun staleTransactionCompletion_doesNotClearNewTransactionVisualState() {
        val layouts = captureLayouts("", "abc", "abcde")
        val state = ComposeEditorVisualState(targetId = "test-target-stale-complete")

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
        assertTrue(
            "事务 A 应有非空 hiddenRanges",
            state.hiddenRanges.value.isNotEmpty(),
        )

        // === A 到 1f 前生成事务 B（Insert "abc" → "abcde"）===
        // A 还在跑（active = A），B 到来会 rebase A 并生成新事务 B。
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
        assertTrue("事务 B 应有非空 hiddenRanges", state.hiddenRanges.value.isNotEmpty())
        assertEquals(
            "事务 B 的 activeIntent 应是 B 的（expectedNewText=abcde）",
            "abcde",
            state.activeIntent.value?.expectedNewText,
        )

        // === 模拟 A 迟到完成回调 — finishTransaction(A.id) ===
        // 此时 _activeTransaction.value?.id == B.id != A.id，finishTransaction 应直接 return，
        // 不清 B 的 visual state。
        state.finishTransaction(txAId)

        // 核心断言 1：B 仍是当前活跃事务。
        assertEquals(
            "finishTransaction(A.id) 后 activeTransaction 应仍是 B，实际 id=${state.activeTransaction.value?.id}\n" +
                "#684 评论 5667483662 问题2：旧事务迟到的完成回调不应清掉新事务的 visual state",
            txBId,
            state.activeTransaction.value?.id,
        )

        // 核心断言 2：B 的 hiddenRanges 仍保留（未被 A 清掉）。
        assertTrue(
            "finishTransaction(A.id) 后 hiddenRanges 应仍是 B 的（非空），实际=${state.hiddenRanges.value}\n" +
                "#684 评论 5667483662 问题2：旧事务迟到完成不应提前解除新事务的 hiddenRanges",
            state.hiddenRanges.value.isNotEmpty(),
        )

        // 核心断言 3：B 的 activeIntent 仍保留。
        assertEquals(
            "finishTransaction(A.id) 后 activeIntent 应仍是 B 的（expectedNewText=abcde）",
            "abcde",
            state.activeIntent.value?.expectedNewText,
        )
    }

    /**
     * 对照：finishTransaction(当前事务 id) 应正常清掉 visual state。
     *
     * 验证 finishTransaction 在 ID 匹配时确实生效，不是永远 no-op。
     */
    @Test
    fun finishTransaction_matchingId_clearsVisualState() {
        val layouts = captureLayouts("", "abc")
        val state = ComposeEditorVisualState(targetId = "test-target-matching-complete")

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
        assertTrue("完成前应有非空 hiddenRanges", state.hiddenRanges.value.isNotEmpty())

        state.finishTransaction(txId)

        assertTrue(
            "finishTransaction(匹配 id) 后 hiddenRanges 应清空",
            state.hiddenRanges.value.isEmpty(),
        )
        assertEquals(
            "finishTransaction(匹配 id) 后 activeTransaction 应清空",
            null,
            state.activeTransaction.value,
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
