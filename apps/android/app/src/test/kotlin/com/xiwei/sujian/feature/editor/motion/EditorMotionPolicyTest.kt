package com.xiwei.sujian.feature.editor.motion

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 三/十：EditorMotionPolicy 契约测试 — 验证不可变策略的初始值、reduce-motion
 * 降级。EditorAnimationSettings 桥接类型已删除，策略字段直接作为唯一事实源。
 */
class EditorMotionPolicyTest {
    @Test
    fun defaultPolicyMatchesCoreDefaults() {
        val policy = EditorMotionPolicy()
        assertTrue("Core default: text animation enabled", policy.textEnabled)
        assertTrue("Core default: cursor animation enabled", policy.cursorEnabled)
        assertTrue("Core default: coordinated enabled", policy.coordinated)
        assertFalse("Core default: reduce motion disabled", policy.reduceMotion)
        assertEquals(100L, policy.textDurationMillis)
        assertEquals(80L, policy.cursorDurationMillis)
    }

    @Test
    fun reduceMotionDegradesAllAnimationToStatic() {
        val policy =
            EditorMotionPolicy(
                textEnabled = true,
                cursorEnabled = true,
                coordinated = true,
                reduceMotion = true,
            )
        val effective = policy.effective()
        assertFalse("reduce-motion disables text", effective.textEnabled)
        assertFalse("reduce-motion disables cursor", effective.cursorEnabled)
        assertFalse("reduce-motion disables coordinated", effective.coordinated)
    }

    @Test
    fun effectiveIsIdentityWhenReduceMotionFalse() {
        val policy =
            EditorMotionPolicy(
                textEnabled = true,
                cursorEnabled = false,
                coordinated = true,
                reduceMotion = false,
            )
        val effective = policy.effective()
        assertEquals(policy, effective)
    }

    @Test
    fun policyIsImmutableDataClassWithAllFields() {
        val policy =
            EditorMotionPolicy(
                textEnabled = true,
                textDurationMillis = 150L,
                cursorEnabled = false,
                cursorDurationMillis = 60L,
                coordinated = true,
                reduceMotion = false,
            )
        assertEquals(true, policy.textEnabled)
        assertEquals(150L, policy.textDurationMillis)
        assertEquals(false, policy.cursorEnabled)
        assertEquals(60L, policy.cursorDurationMillis)
        assertEquals(true, policy.coordinated)
        assertEquals(false, policy.reduceMotion)
        // 复制修改不影响原实例 — 不可变性契约
        val copy = policy.copy(cursorEnabled = true)
        assertFalse("original must stay unchanged", policy.cursorEnabled)
        assertTrue("copy must reflect change", copy.cursorEnabled)
    }
}
