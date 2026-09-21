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
        // Issue #732 评论 5763493968 第4节：coordinated=true 时 effective() 也是 identity。
        // coordinated 本身就是完整模式，不靠改 textEnabled/cursorEnabled 才成立。
        // effective() 只在 reduceMotion=true 时降级，其余情况直接返回 this。
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
    fun coordinatedTrueIsIdentity_preservesUserSettings() {
        // Issue #732 评论 5763493968 第4节：coordinated=true 时 effective() 直接返回 this，
        // 不再强制 textEnabled=true / cursorEnabled=true。
        // coordinated 本身就是完整模式，统一用 textDurationMillis 作为这一笔
        // ComposeEditMotion 的时长；独立的 textEnabled/cursorEnabled/cursorDurationMillis
        // 只在 coordinated=false 时生效。
        val legacyPolicy =
            EditorMotionPolicy(
                textEnabled = false,
                cursorEnabled = false,
                coordinated = true,
                reduceMotion = false,
            )
        val effective = legacyPolicy.effective()
        assertFalse("coordinated=true → effective() 保持 textEnabled=false（不强制归一）", effective.textEnabled)
        assertFalse("coordinated=true → effective() 保持 cursorEnabled=false（不强制归一）", effective.cursorEnabled)
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
