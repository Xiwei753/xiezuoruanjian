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
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #689 对抗式回归测试 — 独立验证 patch 的边界情况。
 *
 * 这些测试由 result-verifier 独立生成，试图打破 patch：
 * 1. 多步快速输入 A→B→C 时，A 的 alpha 通道不被重置（Type 1 强化）。
 * 2. 删换行后立即快速输入，timeline 状态正确（Type 2a + Type 1 混合）。
 * 3. timeline clear 后状态正确重置，可重新使用（Type 3 生命周期）。
 * 4. hasActiveVisuals 正确反映动画状态（Type 3 协议契约）。
 * 5. 动画结束后 sample 返回 alpha=1 的 unit（Type 3 终态正确性）。
 */
@Suppress("StringLiteralDuplication", "MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualTimelineAdversarialRegressionTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * Type 1 强化：快速输入 A→B→C 三步，A 的 alpha 通道不被重置。
     *
     * 场景："" → "a" → "ab" → "abc"，每步间隔 30ms（动画时长 100ms，远没跑完）。
     * 断言：C 应用后，"a" 的 alpha 应大于 0（A 的动画已跑了 60ms）。
     */
    @Test
    fun rapidInput_threeSteps_firstUnitAlphaNotReset() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val state = ComposeEditorVisualState(targetId = "test-adv-three-step")

        // Step A: "" → "a"
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1), VisualReplaceBounds(0, 0, 0, 1)),
            EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        val patchA = state.latestPatch.value!!
        state.applyVisualPatchAtFrame(patchA, 0L)

        // Step B: "a" → "ab" at 30ms
        val frameTimeB = 30L * 1_000_000L
        state.onVisualIntent(
            makeInsertIntent(
                2L, 1L, 2L, "a", "ab", TextRange(1, 2), VisualReplaceBounds(1, 1, 1, 2),
                VisualOffsetMap(listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY))),
            ),
            EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        val patchB = state.latestPatch.value!!
        state.applyVisualPatchAtFrame(patchB, frameTimeB)

        // Step C: "ab" → "abc" at 60ms
        val frameTimeC = 60L * 1_000_000L
        state.onVisualIntent(
            makeInsertIntent(
                3L, 2L, 3L, "ab", "abc", TextRange(2, 3), VisualReplaceBounds(2, 2, 2, 3),
                VisualOffsetMap(listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY))),
            ),
            EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(3, 3), 0)
        val patchC = state.latestPatch.value!!
        state.applyVisualPatchAtFrame(patchC, frameTimeC)

        val sceneC = state.sampleVisualScene(frameTimeC)
        // "a" 对应的存活 unit（targetRange = [0,1)）的 alpha 应大于 0
        val unitA = sceneC.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("C 应用后应仍有 'a' 的存活 unit", unitA)
        val alphaA = unitA!!.alpha.from
        assertTrue(
            "三步快速输入后 'a' 的 alpha 应大于 0（持续 timeline 不重置），实际=$alphaA\n" +
                "Type 1 强化：多步快速输入时首个 unit alpha 通道不被重置",
            alphaA > 0f,
        )
    }

    /**
     * Type 3 生命周期：timeline clear 后状态正确重置，可重新使用。
     *
     * 场景：应用 patch A → clear → 应用 patch B。
     * 断言：clear 后 hasActiveVisuals=false，sample 返回空场景；
     *       重新应用 patch B 后正常工作。
     */
    @Test
    fun clear_resetsState_canReuseAfterClear() {
        val layouts = captureLayouts("", "abc", "xyz")
        val state = ComposeEditorVisualState(targetId = "test-adv-clear")

        // 应用 patch A
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "abc", TextRange(0, 3), VisualReplaceBounds(0, 0, 0, 3)),
            EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val patchA = state.latestPatch.value!!
        state.applyVisualPatchAtFrame(patchA, 0L)
        assertTrue("应用 patch A 后应有活动动画", state.hasActiveVisuals(0L))

        // clear
        state.clear()
        assertFalse("clear 后不应有活动动画", state.hasActiveVisuals(0L))
        val sceneAfterClear = state.sampleVisualScene(0L)
        assertEquals("clear 后场景应为空", 0, sceneAfterClear.units.size)

        // 重新应用 patch B
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(2L, 0L, 2L, "", "xyz", TextRange(0, 3), VisualReplaceBounds(0, 0, 0, 3)),
            EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(3, 3), 0)
        val patchB = state.latestPatch.value
        assertNotNull("clear 后应能生成新 patch", patchB)
        state.applyVisualPatchAtFrame(patchB!!, 0L)
        assertTrue("重新应用 patch B 后应有活动动画", state.hasActiveVisuals(0L))
    }

    /**
     * Type 3 终态正确性：动画结束后 sample 返回 alpha=1 的 unit。
     *
     * 场景：应用 patch A（duration=100ms），在 frameTime=200ms（动画已结束）sample。
     * 断言：unit 的 alpha 应为 1f（动画已完成）。
     */
    @Test
    fun animationCompleted_unitAlphaReachesOne() {
        val layouts = captureLayouts("", "abc")
        val state = ComposeEditorVisualState(targetId = "test-adv-terminal")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "abc", TextRange(0, 3), VisualReplaceBounds(0, 0, 0, 3)),
            EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val patch = state.latestPatch.value!!
        state.applyVisualPatchAtFrame(patch, 0L)

        // 在 200ms sample（动画时长 100ms，已结束）
        val frameTimeEnd = 200L * 1_000_000L
        val scene = state.sampleVisualScene(frameTimeEnd)
        assertEquals("动画结束后应有 1 个 unit", 1, scene.units.size)
        val unit = scene.units[0]
        assertEquals(
            "动画结束后 alpha 应为 1f，实际=${unit.alpha.from}\n" +
                "Type 3 终态正确性：动画完成后 unit alpha 到达 1",
            1f,
            unit.alpha.from,
            0.001f,
        )
        assertFalse(
            "动画结束后 hasActiveVisuals 应为 false",
            state.hasActiveVisuals(frameTimeEnd),
        )
    }

    /**
     * Type 3 协议契约：hasActiveVisuals 在动画进行中返回 true，结束后返回 false。
     *
     * 场景：应用 patch A（duration=100ms）。
     * 断言：frameTime=0 时 true，frameTime=50ms 时 true，frameTime=100ms 时 false。
     */
    @Test
    fun hasActiveVisuals_correctlyReflectsAnimationState() {
        val layouts = captureLayouts("", "abc")
        val state = ComposeEditorVisualState(targetId = "test-adv-active-visuals")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "abc", TextRange(0, 3), VisualReplaceBounds(0, 0, 0, 3)),
            EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val patch = state.latestPatch.value!!
        state.applyVisualPatchAtFrame(patch, 0L)

        assertTrue("frameTime=0 时应有活动动画", state.hasActiveVisuals(0L))
        assertTrue(
            "frameTime=50ms 时应有活动动画",
            state.hasActiveVisuals(50L * 1_000_000L),
        )
        assertFalse(
            "frameTime=100ms 时不应有活动动画",
            state.hasActiveVisuals(100L * 1_000_000L),
        )
    }

    /**
     * Type 2a + Type 1 混合：删换行后立即快速输入，timeline 不崩溃。
     *
     * 场景："" → "ab\nc" → "abc" → "abcde"（删换行后立即插入）。
     * 断言：不崩溃，最终场景有正确的存活 unit。
     */
    @Test
    fun deleteNewlineThenRapidInput_timelineDoesNotCrash() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "ab\nc", "abc", "abcde"), 1000)
        val state = ComposeEditorVisualState(targetId = "test-adv-mixed")

        // Step A: "" → "ab\nc"
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "ab\nc", TextRange(0, 4), VisualReplaceBounds(0, 0, 0, 4)),
            EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(4, 4), 0)
        val patchA = state.latestPatch.value!!
        state.applyVisualPatchAtFrame(patchA, 0L)

        // Step B: "ab\nc" → "abc" (删换行) at 30ms
        val frameTimeB = 30L * 1_000_000L
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = VisualOffsetMap(
                    entries = listOf(
                        VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                        VisualOffsetMapEntry(3, 2, 1, VisualOffsetMapKind.SHIFTED),
                    ),
                ),
                oldRanges = listOf(TextRange(2, 3)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(2, 3, 2, 2),
                expectedOldText = "ab\nc",
                expectedNewText = "abc",
            ),
            EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(3, 3), 0)
        val patchB = state.latestPatch.value!!
        state.applyVisualPatchAtFrame(patchB, frameTimeB)

        // Step C: "abc" → "abcde" (立即快速插入) at 60ms
        val frameTimeC = 60L * 1_000_000L
        state.onVisualIntent(
            makeInsertIntent(
                3L, 2L, 3L, "abc", "abcde", TextRange(3, 5), VisualReplaceBounds(3, 3, 3, 5),
                VisualOffsetMap(listOf(VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY))),
            ),
            EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(5, 5), 0)
        val patchC = state.latestPatch.value!!
        state.applyVisualPatchAtFrame(patchC, frameTimeC)

        // 不崩溃即通过；额外验证场景非空
        val scene = state.sampleVisualScene(frameTimeC)
        assertTrue(
            "删换行后立即快速输入，场景应有 unit（不崩溃）",
            scene.units.isNotEmpty(),
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
    ): EditorVisualIntent =
        EditorVisualIntent(
            coreTransactionId = coreTxnId,
            baseRevision = baseRev,
            newRevision = newRev,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 100L,
            offsetMap = offsetMap,
            oldRanges = emptyList(),
            newRanges = listOf(newRange),
            textKind = TextVisualKind.Insert,
            cursor = null,
            replaceBounds = replaceBounds,
            expectedOldText = oldText,
            expectedNewText = newText,
        )

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> =
        captureLayoutsWithWidth(texts, 1000)

    private fun captureLayoutsWithWidth(
        texts: Array<out String>,
        maxWidth: Int,
        fontSizeSp: Float = 14f,
    ): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = fontSizeSp.sp),
                        constraints = Constraints(maxWidth = maxWidth),
                    ),
                )
            }
        }
        return results
    }
}
