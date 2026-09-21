package com.xiwei.sujian.feature.editor.motion

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Issue #732 评论 5764716281 对抗性边界验证 —
 *
 * 独立验证三个硬问题的边界语义，不依赖现有测试。由 ResultVerifier 独立编写，
 * 覆盖现有测试未触及的边界情况：
 * - coordinated=false 时派生值回退到 raw 字段
 * - reduceMotion=true 优先级最高
 * - selectionCursorDurationMillis 在 coordinated=true 时用 textDurationMillis（即使 cursorDurationMillis 不同）
 * - editDurationMillis 始终等于 textDurationMillis
 */
class Issue732Comment5764716281AdversarialBoundaryTest {
    @Test
    fun hardProblem1_coordinatedFalse_derivedValuesFallBackToRawFields() {
        // coordinated=false：派生值回退到 raw textEnabled/cursorEnabled
        val policy =
            EditorMotionPolicy(
                textEnabled = false,
                cursorEnabled = true,
                coordinated = false,
                reduceMotion = false,
            )
        assertFalse(
            "coordinated=false → textAnimationEnabledForEdit == textEnabled (false)",
            policy.textAnimationEnabledForEdit,
        )
        assertTrue(
            "coordinated=false → cursorAnimationEnabledForEdit == cursorEnabled (true)",
            policy.cursorAnimationEnabledForEdit,
        )
        // selectionCursorDurationMillis 在 coordinated=false 时用 cursorDurationMillis
        assertEquals(
            "coordinated=false → selectionCursorDurationMillis == cursorDurationMillis",
            policy.cursorDurationMillis,
            policy.selectionCursorDurationMillis,
        )
    }

    @Test
    fun hardProblem1_coordinatedTrue_selectionCursorDurationUsesTextDurationNotCursorDuration() {
        // coordinated=true 且 cursorDurationMillis != textDurationMillis：
        // selectionCursorDurationMillis 必须等于 textDurationMillis，不是 cursorDurationMillis
        val policy =
            EditorMotionPolicy(
                textEnabled = false,
                cursorEnabled = false,
                textDurationMillis = 200L,
                cursorDurationMillis = 50L,
                coordinated = true,
                reduceMotion = false,
            )
        assertEquals(
            "coordinated=true → selectionCursorDurationMillis == textDurationMillis (200L)，不是 cursorDurationMillis (50L)",
            200L,
            policy.selectionCursorDurationMillis,
        )
        // editDurationMillis 始终等于 textDurationMillis
        assertEquals(
            "editDurationMillis == textDurationMillis (200L)",
            200L,
            policy.editDurationMillis,
        )
    }

    @Test
    fun hardProblem1_reduceMotionTrue_overridesCoordinated() {
        // reduceMotion=true 优先级最高：即使 coordinated=true，派生值也应降级
        val policy =
            EditorMotionPolicy(
                textEnabled = true,
                cursorEnabled = true,
                coordinated = true,
                reduceMotion = true,
            )
        val effective = policy.effective()
        // reduceMotion -> effective() 把 coordinated 设为 false
        assertFalse("reduceMotion=true → effective().coordinated == false", effective.coordinated)
        // 派生值：coordinated=false（被 reduceMotion 降级）&& textEnabled=false → false
        assertFalse(
            "reduceMotion=true → textAnimationEnabledForEdit == false",
            effective.textAnimationEnabledForEdit,
        )
        assertFalse(
            "reduceMotion=true → cursorAnimationEnabledForEdit == false",
            effective.cursorAnimationEnabledForEdit,
        )
    }

    @Test
    fun hardProblem1_coordinatedTrue_textEnabledTrue_derivedValuesAlsoTrue() {
        // 正常路径：coordinated=true && textEnabled=true → 派生值 true
        val policy =
            EditorMotionPolicy(
                textEnabled = true,
                cursorEnabled = true,
                coordinated = true,
                reduceMotion = false,
            )
        assertTrue(policy.textAnimationEnabledForEdit)
        assertTrue(policy.cursorAnimationEnabledForEdit)
        assertEquals(policy.textDurationMillis, policy.selectionCursorDurationMillis)
    }

    @Test
    fun hardProblem1_coordinatedFalse_allDisabled_derivedValuesAllFalse() {
        // coordinated=false && textEnabled=false && cursorEnabled=false → 派生值全 false
        val policy =
            EditorMotionPolicy(
                textEnabled = false,
                cursorEnabled = false,
                coordinated = false,
                reduceMotion = false,
            )
        assertFalse(policy.textAnimationEnabledForEdit)
        assertFalse(policy.cursorAnimationEnabledForEdit)
    }
}
