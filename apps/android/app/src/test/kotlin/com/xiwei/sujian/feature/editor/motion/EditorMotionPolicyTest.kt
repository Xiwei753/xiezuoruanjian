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
        assertTrue("Core default: coordinated enabled", policy.coordinated)
        assertFalse("Core default: reduce motion disabled", policy.reduceMotion)
        assertEquals(100L, policy.textDurationMillis)
        // Issue #728：cursor settings defaults
        assertTrue("Core default: cursor animation enabled", policy.cursorEnabled)
        assertEquals(100L, policy.cursorDurationMillis)
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
        // Issue #723 评论 5749023316 缺口2：coordinated=false 时 effective() 才是 identity。
        // coordinated=true 时 effective() 会强制 textEnabled=true（归一旧持久化状态）。
        val policy =
            EditorMotionPolicy(
                textEnabled = true,
                coordinated = false,
                reduceMotion = false,
            )
        val effective = policy.effective()
        assertEquals(policy, effective)
    }

    @Test
    fun coordinatedTrueForcesTextEnabled() {
        // Issue #723 评论 5749023316 缺口2：coordinated=true 时 effective() 强制
        // textEnabled=true，收死旧持久化状态
        // （coordinated=true 但 textEnabled=false）。
        // Issue #728：coordinated=true 也强制 cursorEnabled=true。
        val legacyPolicy =
            EditorMotionPolicy(
                textEnabled = false,
                cursorEnabled = false,
                coordinated = true,
                reduceMotion = false,
            )
        val effective = legacyPolicy.effective()
        assertTrue("coordinated=true → effective() 强制 textEnabled=true", effective.textEnabled)
        assertTrue("coordinated=true → effective() 强制 cursorEnabled=true", effective.cursorEnabled)
        assertTrue("coordinated 标记保持 true", effective.coordinated)
    }

    @Test
    fun policyIsImmutableDataClassWithAllFields() {
        val policy =
            EditorMotionPolicy(
                textEnabled = true,
                textDurationMillis = 150L,
                coordinated = true,
                reduceMotion = false,
            )
        assertEquals(true, policy.textEnabled)
        assertEquals(150L, policy.textDurationMillis)
        assertEquals(true, policy.coordinated)
        assertEquals(false, policy.reduceMotion)
        // 复制修改不影响原实例 — 不可变性契约
        val copy = policy.copy(textEnabled = false)
        assertTrue("original must stay unchanged", policy.textEnabled)
        assertTrue("copy must reflect change", !copy.textEnabled)
    }
}
