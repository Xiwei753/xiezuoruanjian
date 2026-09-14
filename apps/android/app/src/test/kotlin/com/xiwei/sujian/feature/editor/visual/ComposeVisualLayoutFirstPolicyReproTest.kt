package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Constraints
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #684 评论 5667483662 问题1 回归测试 — layout-first motionPolicy 一致性。
 *
 * 场景：新 layout 先到，intent 后到（layout-first 路径）。
 * 旧实现：coordinator 的 lastMotionPolicy 只在 onLayout 里更新，
 * layout-first 时 coordinator 记住上一笔/默认 policy；随后 intent 到达立即 tryStartTransaction，
 * 事务的 textAnimationActive/cursorAnimationActive/motionPolicy 都用 coordinator 里那份旧 policy。
 * 第一笔输入最明显：用户真实设置关闭打字动画/关闭 smooth cursor/自定义时长，
 * 而 layout 恰好先到，事务仍可能按默认 textEnabled=true/cursorEnabled=true/100ms 生成。
 *
 * 修复：PendingVisualChain 自己携带 motionPolicy（与 intent 同源），
 * tryStartTransaction 用 chain 的 policy，不再依赖"最近一次 onLayout 顺手记住的 policy"。
 *
 * 本测试验证：
 * 1. layout-first 路径：先送新 layout，再送 intent（policy = textEnabled=false, cursorEnabled=false, 250ms），
 *    事务冻结字段与该 intent 的 policy 一致。
 * 2. intent-first 路径（对照）：先送 intent，再送 layout，结果相同。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication")
class ComposeVisualLayoutFirstPolicyReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * layout-first 路径：先送新 layout "abc"，再送 intent（policy 关闭文字/光标动画 + 250ms）。
     * 事务的 motionPolicy/textAnimationActive/cursorAnimationActive 应与该 intent 的 policy 一致。
     */
    @Test
    fun layoutFirst_freezesMotionPolicyFromIntentNotFromStaleLayout() {
        val layouts = captureLayouts("", "abc")
        val state = ComposeEditorVisualState(targetId = "test-target-layout-first")

        // 1. 基线 layout "" 到达 — lastConsumed = "".
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 2. 先送新 layout "abc"（layout-first）— latest = "abc"，pending == null，无事务生成。
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        assertNull("layout-first 时基线后无事务（pending 未到）", state.activeTransaction.value)

        // 3. 再送 intent（policy = textEnabled=false, cursorEnabled=false, 250ms）。
        //    pending 创建并立即与 latest 匹配合流生成事务。
        val customPolicy =
            EditorMotionPolicy(
                textEnabled = false,
                textDurationMillis = 250L,
                cursorEnabled = false,
                cursorDurationMillis = 200L,
            )
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 250L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 3, animate = true),
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = customPolicy,
        )

        val transaction = state.activeTransaction.value
        assertNotNull("layout-first 路径应生成事务", transaction)

        // 核心断言：事务冻结的 motionPolicy 与该 intent 的 policy 一致，不是默认 policy。
        assertEquals(
            "事务 motionPolicy.textEnabled 应为 false（来自 intent，非默认 true）\n" +
                "#684 评论 5667483662 问题1：layout-first 时事务不应使用 coordinator 里旧的默认 policy",
            false,
            transaction?.motionPolicy?.textEnabled,
        )
        assertEquals(
            "事务 motionPolicy.cursorEnabled 应为 false（来自 intent，非默认 true）",
            false,
            transaction?.motionPolicy?.cursorEnabled,
        )
        assertEquals(
            "事务 motionPolicy.textDurationMillis 应为 250L（来自 intent，非默认 100L）",
            250L,
            transaction?.motionPolicy?.textDurationMillis,
        )

        // textAnimationActive / cursorAnimationActive 应为 false（policy 关闭）。
        assertFalse(
            "textAnimationActive 应为 false（policy.textEnabled=false）",
            transaction?.textAnimationActive == true,
        )
        assertFalse(
            "cursorAnimationActive 应为 false（policy.cursorEnabled=false）",
            transaction?.cursorAnimationActive == true,
        )
    }

    /**
     * intent-first 路径（对照）：先送 intent，再送 layout，结果应与 layout-first 一致。
     */
    @Test
    fun intentFirst_freezesSameMotionPolicyAsLayoutFirst() {
        val layouts = captureLayouts("", "abc")
        val state = ComposeEditorVisualState(targetId = "test-target-intent-first")

        // 1. 基线 layout "" 到达。
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 2. 先送 intent（policy = textEnabled=false, cursorEnabled=false, 250ms）。
        val customPolicy =
            EditorMotionPolicy(
                textEnabled = false,
                textDurationMillis = 250L,
                cursorEnabled = false,
                cursorDurationMillis = 200L,
            )
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 250L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 3, animate = true),
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = customPolicy,
        )

        // 3. 再送 layout "abc" — 匹配 pending 生成事务。
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)

        val transaction = state.activeTransaction.value
        assertNotNull("intent-first 路径应生成事务", transaction)

        // 与 layout-first 路径相同断言。
        assertEquals(false, transaction?.motionPolicy?.textEnabled)
        assertEquals(false, transaction?.motionPolicy?.cursorEnabled)
        assertEquals(250L, transaction?.motionPolicy?.textDurationMillis)
        assertFalse("textAnimationActive 应为 false", transaction?.textAnimationActive == true)
        assertFalse("cursorAnimationActive 应为 false", transaction?.cursorAnimationActive == true)
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
