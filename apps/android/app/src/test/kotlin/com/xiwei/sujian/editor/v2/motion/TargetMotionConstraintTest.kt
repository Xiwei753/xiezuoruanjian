package com.xiwei.sujian.editor.v2.motion

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #595 四：TargetMotionConstraint 契约测试 — 验证约束应用不成为第二个动画状态写入者。
 */
class TargetMotionConstraintTest {

    @Test
    fun defaultConstraintAllowsAll() {
        val constraint = TargetMotionConstraint()
        val policy = EditorMotionPolicy(textEnabled = true, cursorEnabled = true, coordinated = true)
        val result = constraint.apply(policy)
        assertTrue(result.textEnabled)
        assertTrue(result.cursorEnabled)
        assertTrue(result.coordinated)
    }

    @Test
    fun forceStaticDisablesAllAnimation() {
        val constraint = TargetMotionConstraint(forceStatic = true)
        val policy = EditorMotionPolicy(textEnabled = true, cursorEnabled = true, coordinated = true)
        val result = constraint.apply(policy)
        assertFalse("forceStatic disables text", result.textEnabled)
        assertFalse("forceStatic disables cursor", result.cursorEnabled)
        assertFalse("forceStatic disables coordinated", result.coordinated)
    }

    @Test
    fun allowTextFalseDisablesTextOnly() {
        val constraint = TargetMotionConstraint(allowText = false)
        val policy = EditorMotionPolicy(textEnabled = true, cursorEnabled = true, coordinated = true)
        val result = constraint.apply(policy)
        assertFalse("allowText=false disables text", result.textEnabled)
        assertTrue("cursor still enabled", result.cursorEnabled)
        assertFalse("coordinated requires both text and cursor", result.coordinated)
    }

    @Test
    fun allowCursorFalseDisablesCursorOnly() {
        val constraint = TargetMotionConstraint(allowCursor = false)
        val policy = EditorMotionPolicy(textEnabled = true, cursorEnabled = true, coordinated = true)
        val result = constraint.apply(policy)
        assertTrue("text still enabled", result.textEnabled)
        assertFalse("allowCursor=false disables cursor", result.cursorEnabled)
        assertFalse("coordinated requires both text and cursor", result.coordinated)
    }

    @Test
    fun constraintComposesWithReduceMotion() {
        val constraint = TargetMotionConstraint(allowText = false)
        val policy = EditorMotionPolicy(textEnabled = true, cursorEnabled = true, coordinated = true, reduceMotion = true)
        val effective = policy.effective()
        val result = constraint.apply(effective)
        assertFalse("reduce-motion disables text", result.textEnabled)
        assertFalse("reduce-motion disables cursor", result.cursorEnabled)
    }
}
