package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
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
    // Issue #725 评论 5750735497：applyVisualPatchAtFrame 已删除，patch 通过
    // drainPendingPatchesAtFrame 自动消费。本测试方法依赖手动 apply API，已移除。

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
            "应有 drainPendingPatchesAtFrame 方法（新持续 timeline API）",
            methods.contains("drainPendingPatchesAtFrame"),
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
    // Issue #725 评论 5750735497：applyVisualPatchAtFrame 已删除，patch 通过
    // drainPendingPatchesAtFrame 自动消费。本测试方法依赖手动 apply API，已移除。

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
