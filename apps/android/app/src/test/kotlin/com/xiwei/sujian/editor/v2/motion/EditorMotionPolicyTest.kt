package com.xiwei.sujian.editor.v2.motion

import com.xiwei.sujian.editor.v2.coordinator.EditorAnimationSettings
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 三：EditorMotionPolicy 契约测试 — 验证不可变策略的初始值、reduce-motion
 * 降级和与 EditorAnimationSettings 的互转。
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
        val policy = EditorMotionPolicy(
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
        val policy = EditorMotionPolicy(
            textEnabled = true,
            cursorEnabled = false,
            coordinated = true,
            reduceMotion = false,
        )
        val effective = policy.effective()
        assertEquals(policy, effective)
    }

    @Test
    fun roundTripToAnimationSettingsPreservesAllFields() {
        val policy = EditorMotionPolicy(
            textEnabled = true,
            textDurationMillis = 150L,
            cursorEnabled = false,
            cursorDurationMillis = 60L,
            coordinated = true,
            reduceMotion = false,
        )
        val settings = EditorAnimationSettings.fromMotionPolicy(policy)
        assertEquals(policy.textEnabled, settings.typingAnimationEnabled)
        assertEquals(policy.textDurationMillis, settings.typingAnimationDurationMs)
        assertEquals(policy.cursorEnabled, settings.smoothCursorEnabled)
        assertEquals(policy.cursorDurationMillis, settings.smoothCursorDurationMs)
        assertEquals(policy.coordinated, settings.coordinated)
        assertEquals(policy.reduceMotion, settings.reduceMotion)

        val restored = settings.toMotionPolicy()
        assertEquals(policy, restored)
    }

    @Test
    fun animationSettingsDefaultsMatchCoreDefaults() {
        val settings = EditorAnimationSettings()
        assertTrue("Default typing animation enabled", settings.typingAnimationEnabled)
        assertTrue("Default smooth cursor enabled", settings.smoothCursorEnabled)
        assertTrue("Default coordinated enabled", settings.coordinated)
        assertFalse("Default reduce motion disabled", settings.reduceMotion)
    }
}
