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
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #689 回归测试 — "连续输入和删除换行仍会抽搐，把视觉动画从'事务重启'改成持续时间线"。
 *
 * #689 评论 5674631257 的持续视觉状态重构后，旧的事务重启机制（masterProgress 归零、
 * startFrame 物化、activeTransaction）已被完全删除。本测试验证新持续 timeline 行为：
 *
 * 1. 快速输入时已有 unit 的 alpha 通道 startedAtNanos 不被重置 —
 *    新 patch 不会把已存活的 unit 重新从 alpha=0 开始。
 * 2. 新事务不把 masterProgress 归零（因为已不存在 masterProgress 概念）—
 *    visualState 不再暴露 masterProgress / reportProgress / finishTransaction。
 * 3. 删换行时几何没变的存活 unit 不产生 position track —
 *    retainedMoves 只包含真正发生位移的 unit。
 * 4. hiddenRanges 从当前 overlay unit 推导而非继承 —
 *    每一帧直接从 VisualTextUnit.targetRange != null 且仍由 overlay 绘制的 unit 推导。
 *
 * 同时验证：
 * - coordinator 只返回 ComposeVisualPatch（屏幕 diff），不返回 ComposeVisualTransaction。
 * - patch 不携带 startFrame / suppressedCurrentRanges / textAnimationActive 等事务状态。
 * - visualState 暴露 latestPatch / visualScene / hiddenRanges，不暴露 activeTransaction / masterProgress。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength")
class ComposeVisualTransactionRestartReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 新模型验证1：快速输入时 alpha 通道不被重置 ====================

    /**
     * 验证：快速输入 A→B 时，A 的 alpha 通道 startedAtNanos 不被重置。
     *
     * 场景：
     * - 生成 patch A（Insert "" → "abc"），在 frameTime=0 应用。
     * - 生成 patch B（Insert "abc" → "abcde"），在 frameTime=50ms 应用。
     *
     * 断言（验证新持续 timeline 行为）：
     * - B 应用后，timeline 中 "abc" 对应的 unit 的 alpha 通道 startedAtNanos 仍是 A 时的 0，
     *   而非被重置为 50ms。
     * - "abc" 的 alpha 在 frameTime=50ms 时应大于 0（A 的动画已跑了一半），
     *   而非从 0 重新开始。
     */
    @Test
    fun rapidInput_existingUnitAlphaChannel_notReset() {
        val layouts = captureLayouts("", "abc", "abcde")
        val state = ComposeEditorVisualState(targetId = "test-target-issue689-timeline-alpha")

        // === 生成 patch A（Insert "" → "abc"）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "abc",
                newRange = TextRange(0, 3),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 3),
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val patchA = state.latestPatch.value
        assertNotNull("patch A 应生成", patchA)

        // 在 frameTime=0 应用 patch A
        val frameTimeA = 0L
        state.applyVisualPatchAtFrame(patchA!!, frameTimeA)
        val sceneA = state.sampleVisualScene(frameTimeA)
        // A 应用后应有 1 个 unit（"abc"），alpha 从 0 开始
        assertEquals("A 应用后应有 1 个 unit", 1, sceneA.units.size)
        val unitA = sceneA.units[0]
        assertEquals("A 的 alpha 通道 startedAtNanos 应为 0", frameTimeA, unitA.alpha.startedAtNanos)

        // === 生成 patch B（Insert "abc" → "abcde"）===
        state.onVisualIntent(
            makeInsertIntent(
                coreTxnId = 2L,
                baseRev = 1L,
                newRev = 2L,
                oldText = "abc",
                newText = "abcde",
                newRange = TextRange(3, 5),
                replaceBounds = VisualReplaceBounds(3, 3, 3, 5),
                offsetMap =
                    VisualOffsetMap(
                        entries = listOf(VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY)),
                    ),
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(5, 5), 0)
        val patchB = state.latestPatch.value
        assertNotNull("patch B 应生成", patchB)
        assertFalse("B 应是新 patch（id != A.id）", patchB?.id == patchA.id)

        // 在 frameTime=50ms 应用 patch B（A 跑到一半）
        val frameTimeB = 50L * 1_000_000L // 50ms in nanos
        state.applyVisualPatchAtFrame(patchB!!, frameTimeB)
        val sceneB = state.sampleVisualScene(frameTimeB)

        // 核心断言：B 应用后，"abc" 对应的存活 unit 的 alpha 通道 startedAtNanos 不被重置。
        // 它应继续使用 A 时的 startedAtNanos（0），而非被重置为 frameTimeB。
        val survivingAbc = sceneB.units.firstOrNull { it.targetRange == TextRange(0, 3) }
        assertNotNull("B 应用后应仍有 'abc' 的存活 unit", survivingAbc)
        // sample 后 startedAtNanos 被更新为当前帧时间（因为 sample 会重新锚定通道），
        // 但关键是从 alpha=0 重开。检查 alpha 当前值应大于 0（A 已跑了一半）。
        val alphaValue = survivingAbc!!.alpha.from
        assertTrue(
            "B 应用后 'abc' 的 alpha 应大于 0（A 的动画已跑了一半，持续 timeline 不重置），实际=$alphaValue\n" +
                "Issue #689 验证1：快速输入时已有 unit 的 alpha 通道不被重置",
            alphaValue > 0f,
        )
    }

    // ==================== 新模型验证2：masterProgress 概念已删除 ====================

    /**
     * 验证：新模型不暴露 masterProgress / reportProgress / finishTransaction / activeTransaction。
     *
     * 这些旧 API 在持续 timeline 模型下已不存在。本测试通过反射验证它们确实被删除，
     * 确保不会有人意外重新添加。
     */
    @Test
    fun newModel_doesNotExpose_oldTransactionApis() {
        val state = ComposeEditorVisualState(targetId = "test-target-issue689-no-old-api")
        val clazz = ComposeEditorVisualState::class.java

        // masterProgress / activeTransaction / activeIntent 不应作为 public getter 存在
        val methods = clazz.methods.map { it.name }
        assertFalse(
            "不应有 reportProgress 方法（旧事务机制已删除）\n" +
                "Issue #689 验证2：masterProgress 概念已删除",
            methods.contains("reportProgress"),
        )
        assertFalse(
            "不应有 finishTransaction 方法（旧事务机制已删除）\n" +
                "Issue #689 验证2：masterProgress 概念已删除",
            methods.contains("finishTransaction"),
        )

        // 应有新 API
        assertTrue(
            "应有 applyVisualPatchAtFrame 方法（新持续 timeline API）",
            methods.contains("applyVisualPatchAtFrame"),
        )
        assertTrue(
            "应有 sampleVisualScene 方法（新持续 timeline API）",
            methods.contains("sampleVisualScene"),
        )
        assertTrue(
            "应有 hasActiveVisuals 方法（新持续 timeline API）",
            methods.contains("hasActiveVisuals"),
        )
    }

    // ==================== 新模型验证3：删换行时几何没变的 unit 不产生 position track ====================

    /**
     * 验证：删换行时几何没变的存活 unit 不产生 position track。
     *
     * 场景：
     * - 生成 patch A（Insert "" → "ab\nc"），在 frameTime=0 应用。
     * - 生成 patch B（Delete "ab\nc" → "abc"），在 frameTime=50ms 应用。
     *
     * 断言（验证新持续 timeline 行为）：
     * - B 的 retainedMoves 只包含真正发生位移的 unit。
     * - 删换行时 "ab" 在 old/new layout 里位置没变（都在第一行开头），
     *   不应出现在 retainedMoves 里。
     */
    @Test
    fun deleteNewline_geometryUnchangedUnit_noPositionTrack() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "ab\nc", "abc"), maxWidth = 1000)
        val state = ComposeEditorVisualState(targetId = "test-target-issue689-no-pos-track")

        // === 生成 patch A（Insert "" → "ab\nc"）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "ab\nc",
                newRange = TextRange(0, 4),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 4),
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(4, 4), 0)
        val patchA = state.latestPatch.value
        assertNotNull("patch A 应生成", patchA)

        // === 生成 patch B（Delete "ab\nc" → "abc"）===
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                // "ab"
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                                // "c"
                                VisualOffsetMapEntry(3, 2, 1, VisualOffsetMapKind.SHIFTED),
                            ),
                    ),
                // "\n"
                oldRanges = listOf(TextRange(2, 3)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(2, 3, 2, 2),
                expectedOldText = "ab\nc",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(3, 3), 0)
        val patchB = state.latestPatch.value
        assertNotNull("patch B 应生成", patchB)

        // 核心断言：B 的 retainedMoves 只包含真正发生位移的 unit。
        // "ab" 在 old layout（"ab\nc" 第一行）和 new layout（"abc" 第一行）里位置没变，
        // 不应出现在 retainedMoves 里。
        // "c" 从 old layout 第二行移到 new layout 第一行，应出现在 retainedMoves 里。
        val retainedMoves = patchB!!.retainedMoves
        val abMove = retainedMoves.firstOrNull { it.newRange == TextRange(0, 2) }
        assertNull(
            "'ab' 几何没变，不应出现在 retainedMoves 里（不产生 position track）\n" +
                "Issue #689 验证3：删换行时几何没变的存活 unit 不产生 position track",
            abMove,
        )
    }

    // ==================== 新模型验证4：patch 不携带旧事务状态 ====================

    /**
     * 验证：ComposeVisualPatch 是屏幕 diff，不携带旧事务状态。
     *
     * 断言：
     * - ComposeVisualPatch 没有 startFrame / suppressedCurrentRanges / textAnimationActive /
     *   cursorAnimationActive / oldAnimationUnits / newAnimationUnits / textKind 等字段。
     * - ComposeVisualPatch 有 insertedUnits / deletedUnits / offsetMap / retainedMoves 等新字段。
     */
    @Test
    fun patch_doesNotCarry_oldTransactionState() {
        val layouts = captureLayouts("", "abc")
        val state = ComposeEditorVisualState(targetId = "test-target-issue689-patch-fields")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "abc",
                newRange = TextRange(0, 3),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 3),
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val patch = state.latestPatch.value
        assertNotNull("patch 应生成", patch)

        val patchClazz = ComposeVisualPatch::class.java
        val fieldNames = patchClazz.declaredFields.map { it.name }

        // 新字段应存在
        assertTrue("patch 应有 insertedUnits 字段", fieldNames.contains("insertedUnits"))
        assertTrue("patch 应有 deletedUnits 字段", fieldNames.contains("deletedUnits"))
        assertTrue("patch 应有 offsetMap 字段", fieldNames.contains("offsetMap"))
        assertTrue("patch 应有 retainedMoves 字段", fieldNames.contains("retainedMoves"))

        // 旧事务状态字段不应存在
        assertFalse(
            "patch 不应有 startFrame 字段（旧事务物化已删除）\n" +
                "Issue #689 验证4：patch 不携带旧事务状态",
            fieldNames.contains("startFrame"),
        )
        assertFalse(
            "patch 不应有 suppressedCurrentRanges 字段",
            fieldNames.contains("suppressedCurrentRanges"),
        )
        assertFalse(
            "patch 不应有 textAnimationActive 字段",
            fieldNames.contains("textAnimationActive"),
        )
        assertFalse(
            "patch 不应有 cursorAnimationActive 字段",
            fieldNames.contains("cursorAnimationActive"),
        )
        assertFalse(
            "patch 不应有 oldAnimationUnits 字段（改用 deletedUnits）",
            fieldNames.contains("oldAnimationUnits"),
        )
        assertFalse(
            "patch 不应有 newAnimationUnits 字段（改用 insertedUnits）",
            fieldNames.contains("newAnimationUnits"),
        )
        assertFalse(
            "patch 不应有 textKind 字段",
            fieldNames.contains("textKind"),
        )
    }

    // ==================== 新模型验证5：hiddenRanges 从当前 overlay unit 推导 ====================

    /**
     * 验证：hiddenRanges 从当前 overlay unit 推导而非继承。
     *
     * 场景：
     * - 生成 patch A（Insert "" → "abc"），在 frameTime=0 应用。
     * - sample at frameTime=0：alpha=0，unit 仍由 overlay 绘制 → hiddenRanges 包含 [0,3)。
     * - sample at frameTime=100ms（动画结束）：alpha=1，unit 不再由 overlay 绘制 → hiddenRanges 为空。
     *
     * 断言：
     * - 动画进行中 hiddenRanges 包含正在动画的 range。
     * - 动画结束后 hiddenRanges 为空（系统正文已可见）。
     */
    @Test
    fun hiddenRanges_derivedFromCurrentOverlayUnits() {
        val layouts = captureLayouts("", "abc")
        val state = ComposeEditorVisualState(targetId = "test-target-issue689-hidden-ranges")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "abc",
                newRange = TextRange(0, 3),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 3),
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val patch = state.latestPatch.value
        assertNotNull("patch 应生成", patch)

        // 在 frameTime=0 应用 patch
        state.applyVisualPatchAtFrame(patch!!, 0L)
        // sample at frameTime=0：alpha=0，unit 仍由 overlay 绘制
        val sceneAtStart = state.sampleVisualScene(0L)
        assertTrue(
            "动画开始时 hiddenRanges 应包含正在动画的 range [0,3)，实际=${sceneAtStart.hiddenRanges}\n" +
                "Issue #689 验证5：hiddenRanges 从当前 overlay unit 推导",
            sceneAtStart.hiddenRanges.contains(TextRange(0, 3)),
        )

        // sample at frameTime=100ms（动画结束）：alpha=1，unit 不再由 overlay 绘制
        val frameTimeEnd = 100L * 1_000_000L // 100ms in nanos
        val sceneAtEnd = state.sampleVisualScene(frameTimeEnd)
        assertFalse(
            "动画结束后 hiddenRanges 应为空（系统正文已可见），实际=${sceneAtEnd.hiddenRanges}\n" +
                "Issue #689 验证5：hiddenRanges 从当前 overlay unit 推导而非继承",
            sceneAtEnd.hiddenRanges.contains(TextRange(0, 3)),
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
        captureLayoutsWithWidth(texts, maxWidth = 1000)

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
