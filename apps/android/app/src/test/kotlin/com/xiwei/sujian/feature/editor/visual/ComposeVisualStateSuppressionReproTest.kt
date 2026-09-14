package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #684 评论 5665907509 复现测试 — 两个状态机 bug：
 *
 * 问题1：SYSTEM_SUPPRESSED 打断旧动画时，仍然会继续隐藏正文并跑 startFrame。
 *   上一笔 CLUSTER_ANIMATION 动画留下的 suppressed ranges 会被带进新的
 *   SYSTEM_SUPPRESSED 事务（mappedPrevSuppressedRanges 非空），startFrame 仍会被计算，
 *   导致 overlay 继续画 startFrame rebase，"禁用自定义动画"没有真正收口。
 *
 * 问题2：没有 pending 文本事务的真实重新排版不会推进 lastConsumed。
 *   正文没变但真实 TextLayoutResult 已变（宽度变化、字体/字号变化、窗口/方向变化导致
 *   软换行重排）时，屏幕已在 layout B，coordinator 旧侧基线还停在 layout A。
 *   下一次输入生成 layout C 时，事务错误地拿 A→C 做 retained move / cursor geometry。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualStateSuppressionReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    /** layout 构造规格 — 文本 + maxWidth，用于一次 setContent 构造多份不同几何的 layout。 */
    private data class LayoutSpec(val text: String, val maxWidth: Int, val fontSizeSp: Float = 14f)

    // ==================== 问题1 测试 ====================

    /**
     * 问题1 复现：SYSTEM_SUPPRESSED 打断旧动画时，应清空 hiddenRanges 和 startFrame。
     *
     * 场景：
     * - 事务 A：CLUSTER_ANIMATION Insert "abc"（hiddenRanges=[0,3)，active=A）。
     * - A 跑到 progress=0.5（物化 B 时 startFrame 默认非 null）。
     * - 事务 B：SYSTEM_SUPPRESSED "abc" → "abcd"。
     *
     * 正确行为：B.startFrame == null、B.suppressedCurrentRanges == emptyList、
     * state.hiddenRanges.value == emptyList。上一笔动画不会跨过 suppressed 事务继续跑。
     *
     * 当前代码：mappedPrevSuppressedRanges 仍映射 A.suppressedCurrentRanges，
     * startFrame 仍被计算，导致 overlay 继续画 startFrame rebase。
     */
    @Test
    fun systemSuppressed_afterActiveAnimation_clearsHiddenRangesAndStartFrame() {
        val layouts = captureLayouts("", "abc", "abcd")
        val state = ComposeEditorVisualState(targetId = "test-target-suppression-1")

        // === 生成事务 A（CLUSTER_ANIMATION Insert "abc"）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        val intentA =
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
            )
        state.onVisualIntent(
            intentA,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val txA = state.activeTransaction.value
        assertNotNull("事务 A 应生成", txA)
        assertTrue(
            "事务 A 应有非空 hiddenRanges（CLUSTER_ANIMATION Insert）",
            state.hiddenRanges.value.isNotEmpty(),
        )

        // A 跑到 progress=0.5（物化 B 时若未收口 startFrame 会非 null）。
        state.reportProgress(state.activeTransaction.value?.id ?: 0L, 0.5f)

        // === 生成事务 B（SYSTEM_SUPPRESSED "abc" → "abcd"）===
        val intentB =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(3, 4)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 3, oldEnd = 3, newStart = 3, newEnd = 4),
                expectedOldText = "abc",
                expectedNewText = "abcd",
            )
        state.onVisualIntent(
            intentB,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(4, 4), 0)

        val txB = state.activeTransaction.value
        assertNotNull("事务 B 应生成", txB)

        // 核心断言 1：B.startFrame 应为 null（不让上一笔 overlay 动画跨过 suppressed 事务继续跑）。
        assertNull(
            "SYSTEM_SUPPRESSED 事务的 startFrame 应为 null，实际 startFrame=${txB?.startFrame}\n" +
                "#684 评论 5665907509 问题1：SYSTEM_SUPPRESSED 到来时应直接落到系统最终正文，" +
                "不应让上一笔动画的 startFrame 跨过这笔 suppressed 事务继续跑 rebase",
            txB?.startFrame,
        )

        // 核心断言 2：B.suppressedCurrentRanges 应为空。
        assertTrue(
            "SYSTEM_SUPPRESSED 事务的 suppressedCurrentRanges 应为空，实际=${txB?.suppressedCurrentRanges}\n" +
                "#684 评论 5665907509 问题1：上一笔动画留下的 suppressed ranges 不应跨过 " +
                "suppressed 事务继续被 OutputTransformation 设透明",
            txB?.suppressedCurrentRanges.isNullOrEmpty(),
        )

        // 核心断言 3：state.hiddenRanges.value 应为空（overlay 不隐藏正文）。
        assertTrue(
            "SYSTEM_SUPPRESSED 事务后 hiddenRanges 应为空，实际=${state.hiddenRanges.value}\n" +
                "#684 评论 5665907509 问题1：SYSTEM_SUPPRESSED 时 overlay 不画自定义正文，" +
                "hiddenRanges 也必须为空，否则正文被隐藏到事务结束",
            state.hiddenRanges.value.isEmpty(),
        )
    }

    /**
     * 问题1 补充：事务的 animationMode 字段应等于 intent 的 animationMode。
     *
     * 分别测 CLUSTER_ANIMATION 和 SYSTEM_SUPPRESSED 两种模式。
     */
    @Test
    fun systemSuppressed_freezesAnimationModeInTransaction() {
        // 一次 setContent 构造所有需要的 layout："", "abc", "abcd"。
        val layouts = captureLayouts("", "abc", "abcd")

        // === CLUSTER_ANIMATION ===
        val stateCluster = ComposeEditorVisualState(targetId = "test-target-mode-cluster")
        stateCluster.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        stateCluster.onVisualIntent(
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
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        stateCluster.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        assertEquals(
            "CLUSTER_ANIMATION 事务的 animationMode 应被冻结进事务",
            AnimationModeDto.CLUSTER_ANIMATION,
            stateCluster.activeTransaction.value?.animationMode,
        )

        // === SYSTEM_SUPPRESSED ===
        val stateSuppressed = ComposeEditorVisualState(targetId = "test-target-mode-suppressed")
        stateSuppressed.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        stateSuppressed.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(3, 4)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "abc",
                expectedNewText = "abcd",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        stateSuppressed.onAuthoritativeLayout(layouts[2], TextRange(4, 4), 0)
        assertEquals(
            "SYSTEM_SUPPRESSED 事务的 animationMode 应被冻结进事务",
            AnimationModeDto.SYSTEM_SUPPRESSED,
            stateSuppressed.activeTransaction.value?.animationMode,
        )
    }

    // ==================== 问题2 测试 ====================

    /**
     * 问题2 复现：没有 pending 文本事务的真实重新排版也要推进 lastConsumed。
     *
     * 场景：
     * - onLayout("abc" 宽) — 首次 layout，lastConsumed = 宽 "abc"。
     * - onLayout("abc" 窄) — 同文本不同几何（模拟宽度变化导致软换行重排）。
     *   没有 pending 且没有 active 且文本相同，lastConsumed 应推进到窄 "abc"。
     * - 发一笔 intent expectedOldText="abc" expectedNewText="abcd"。
     * - onLayout("abcd" 窄) — 事务应生成，oldLayout 应是窄 "abc"（最新几何）。
     *
     * 当前代码：onLayout 只在首次设 lastConsumed，之后 pending==null 时 tryStartTransaction
     * 直接返回 Empty，lastConsumed 不更新。下一次输入事务错误地拿宽 "abc"→"abcd" 做 retained move。
     */
    @Test
    fun onLayout_noPending_advancesLastConsumed() {
        // 一次 setContent 构造所有需要的 layout：宽 "abc"、窄 "abc"、窄 "abcd"。
        // 用不同 maxWidth 产生不同几何，通过 layoutInput.constraints.maxWidth 区分。
        val layouts =
            captureLayoutsWithSpecs(
                listOf(
                    // 0: 宽 "abc"
                    LayoutSpec(text = "abc", maxWidth = 1000),
                    // 1: 窄 "abc"
                    LayoutSpec(text = "abc", maxWidth = 50),
                    // 2: 窄 "abcd"
                    LayoutSpec(text = "abcd", maxWidth = 50),
                ),
            )
        val wideAbc = layouts[0]
        val narrowAbc = layouts[1]
        val narrowAbcd = layouts[2]

        val state = ComposeEditorVisualState(targetId = "test-target-layout-advance")

        // 1. 首次 onLayout("abc" 宽) — lastConsumed = 宽 "abc"。
        state.onAuthoritativeLayout(wideAbc, TextRange(3, 3), 0)

        // 2. onLayout("abc" 窄) — 同文本不同几何，没有 pending 且没有 active 且文本相同。
        //    lastConsumed 应推进到窄 "abc"。
        state.onAuthoritativeLayout(narrowAbc, TextRange(3, 3), 0)

        // 3. 发一笔 intent expectedOldText="abc" expectedNewText="abcd"。
        val intent =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(3, 4)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 3, oldEnd = 3, newStart = 3, newEnd = 4),
                expectedOldText = "abc",
                expectedNewText = "abcd",
            )
        state.onVisualIntent(
            intent,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // 4. onLayout("abcd" 窄) — 事务应生成。
        state.onAuthoritativeLayout(narrowAbcd, TextRange(4, 4), 0)

        val transaction = state.activeTransaction.value
        assertNotNull(
            "事务应生成（lastConsumed 已推进到窄 'abc'，与 intent.expectedOldText 匹配）",
            transaction,
        )

        // 核心断言：事务的 oldLayout 应是窄 "abc"（最新几何），不是宽 "abc"（首次 layout）。
        // 通过 layoutInput.constraints.maxWidth 区分两份 layout。
        val oldLayoutMaxWidth = transaction?.oldLayout?.result?.layoutInput?.constraints?.maxWidth
        assertEquals(
            "事务 oldLayout 应是窄 'abc'（maxWidth=50），实际 maxWidth=$oldLayoutMaxWidth\n" +
                "#684 评论 5665907509 问题2：没有 pending 文本事务的真实重新排版也要推进 lastConsumed，" +
                "否则事务错误地拿旧几何（宽 'abc'）→ 新几何做 retained move / cursor geometry",
            50,
            oldLayoutMaxWidth,
        )
    }

    /**
     * 问题2 补充：completeTransaction 后 lastConsumed 应更新到最新真实 layout。
     *
     * 场景：
     * - 事务 A：Insert "abc"（宽）→ active=A，lastConsumed=宽 "abc"。
     * - 动画期间 layout 几何变化：onLayout("abc" 窄)（同文本不同几何）。
     *   active != null，onLayout 不推进 lastConsumed，但 latest 更新到窄 "abc"。
     * - completeTransaction(A) — latest 是窄 "abc"，与 A.newLayout.text 同文本，
     *   lastConsumed 应推进到窄 "abc"。
     * - 发一笔 intent expectedOldText="abc" expectedNewText="abcd"。
     * - onLayout("abcd" 窄) — 事务 B 应生成，oldLayout 应是窄 "abc"（最新几何）。
     *
     * 当前代码：completeTransaction 只清 active，不更新 lastConsumed。
     * 动画结束后基线仍是旧几何（宽 "abc"），下一次输入事务错误地拿宽 "abc"→"abcd"。
     */
    @Test
    fun completeTransaction_advancesLastConsumedToLatestLayout() {
        // 一次 setContent 构造所有需要的 layout：宽 ""、宽 "abc"、窄 "abc"、窄 "abcd"。
        val layouts =
            captureLayoutsWithSpecs(
                listOf(
                    // 0: 宽 ""
                    LayoutSpec(text = "", maxWidth = 1000),
                    // 1: 宽 "abc"
                    LayoutSpec(text = "abc", maxWidth = 1000),
                    // 2: 窄 "abc"
                    LayoutSpec(text = "abc", maxWidth = 50),
                    // 3: 窄 "abcd"
                    LayoutSpec(text = "abcd", maxWidth = 50),
                ),
            )
        val wideEmpty = layouts[0]
        val wideAbc = layouts[1]
        val narrowAbc = layouts[2]
        val narrowAbcd = layouts[3]

        val state = ComposeEditorVisualState(targetId = "test-target-complete-advance")

        // === 生成事务 A（Insert "abc" 宽）===
        state.onAuthoritativeLayout(wideEmpty, TextRange(0, 0), 0)
        val intentA =
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
            )
        state.onVisualIntent(
            intentA,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(wideAbc, TextRange(3, 3), 0)
        val txA = state.activeTransaction.value
        assertNotNull("事务 A 应生成", txA)
        val txAId = txA?.id ?: return

        // 动画期间 layout 几何变化：onLayout("abc" 窄)（同文本不同几何）。
        // active != null，onLayout 不推进 lastConsumed，但 latest 更新到窄 "abc"。
        state.onAuthoritativeLayout(narrowAbc, TextRange(3, 3), 0)

        // completeTransaction(A) — latest 是窄 "abc"，与 A.newLayout.text 同文本，
        // lastConsumed 应推进到窄 "abc"。
        // #684 评论 5667483662 问题2：用 finishTransaction 收口带 ID 守卫的完成方法，
        // 不再分两步 completeActiveTransaction + clearAnimation。
        state.finishTransaction(txAId)

        // 发一笔 intent expectedOldText="abc" expectedNewText="abcd"。
        val intentB =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(3, 4)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 3, oldEnd = 3, newStart = 3, newEnd = 4),
                expectedOldText = "abc",
                expectedNewText = "abcd",
            )
        state.onVisualIntent(
            intentB,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // onLayout("abcd" 窄) — 事务 B 应生成。
        state.onAuthoritativeLayout(narrowAbcd, TextRange(4, 4), 0)

        val txB = state.activeTransaction.value
        assertNotNull(
            "事务 B 应生成（completeTransaction 已推进 lastConsumed 到窄 'abc'）",
            txB,
        )

        // 核心断言：事务 B 的 oldLayout 应是窄 "abc"（最新几何），不是宽 "abc"（事务 A 的 newLayout）。
        val oldLayoutMaxWidth = txB?.oldLayout?.result?.layoutInput?.constraints?.maxWidth
        assertEquals(
            "事务 B oldLayout 应是窄 'abc'（maxWidth=50），实际 maxWidth=$oldLayoutMaxWidth\n" +
                "#684 评论 5665907509 问题2：completeTransaction 后 lastConsumed 应更新到最新真实 layout，" +
                "否则动画结束后基线仍是旧几何（宽 'abc'），下一次输入事务错误地拿宽 'abc'→'abcd'",
            50,
            oldLayoutMaxWidth,
        )
    }

    // ==================== 辅助方法 ====================

    /**
     * 用 [rememberTextMeasurer] 在 Compose 测试环境里构造真实 [TextLayoutResult]。
     * maxWidth=1000 避免折行，让 bounds 计算确定。
     */
    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> =
        captureLayoutsWithSpecs(texts.map { LayoutSpec(text = it, maxWidth = 1000) })

    /**
     * 用一组 [LayoutSpec] 一次性构造多份 layout — 一次 setContent 调用，
     * 避免多次 setContent 触发 IllegalStateException。
     * 不同 maxWidth 产生不同几何（行数/行宽），供问题2测试模拟"同文本不同几何"的真实重新排版场景。
     */
    private fun captureLayoutsWithSpecs(specs: List<LayoutSpec>): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = rememberTextMeasurer()
            specs.forEach { spec ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(spec.text),
                        style = TextStyle(fontSize = spec.fontSizeSp.sp),
                        constraints = Constraints(maxWidth = spec.maxWidth),
                    ),
                )
            }
        }
        return results
    }
}
