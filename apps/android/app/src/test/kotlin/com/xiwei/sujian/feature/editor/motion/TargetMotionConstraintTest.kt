package com.xiwei.sujian.feature.editor.motion

import com.xiwei.sujian.feature.editor.session.AnimationPolicy
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.window.EditorWindowHost
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 四：TargetMotionConstraint 契约测试 — 验证约束应用不成为第二个动画状态写入者。
 *
 * profile 只是约束条件：SYSTEM_SUPPRESSED（Search/Token/RepositoryUrl/BranchName/
 * ReplaceQuery）→ forceStatic；INHERIT_GLOBAL / ENABLED → 无约束。
 * effectivePolicy = globalPolicy.apply(profileConstraint).effective() 只在一个计算点合成。
 *
 * Issue #725：自绘 caret 已删除，allowCursor 不再生效；光标动画由系统 BasicTextField 处理。
 */
class TargetMotionConstraintTest {
    @Test
    fun defaultConstraintAllowsAll() {
        val constraint = TargetMotionConstraint()
        val policy = EditorMotionPolicy(textEnabled = true, coordinated = true)
        val result = constraint.apply(policy)
        assertTrue(result.textEnabled)
        assertTrue(result.coordinated)
    }

    @Test
    fun forceStaticDisablesAllAnimation() {
        val constraint = TargetMotionConstraint(forceStatic = true)
        val policy = EditorMotionPolicy(textEnabled = true, coordinated = true)
        val result = constraint.apply(policy)
        assertFalse("forceStatic disables text", result.textEnabled)
        assertFalse("forceStatic disables coordinated", result.coordinated)
    }

    @Test
    fun allowTextFalseDisablesTextOnly() {
        val constraint = TargetMotionConstraint(allowText = false)
        val policy = EditorMotionPolicy(textEnabled = true, coordinated = true)
        val result = constraint.apply(policy)
        assertFalse("allowText=false disables text", result.textEnabled)
        // coordinated requires textEnabled, so it's also disabled
        assertFalse("coordinated requires text", result.coordinated)
    }

    @Test
    fun constraintComposesWithReduceMotion() {
        val constraint = TargetMotionConstraint(allowText = false)
        val policy =
            EditorMotionPolicy(textEnabled = true, coordinated = true, reduceMotion = true)
        val effective = policy.effective()
        val result = constraint.apply(effective)
        assertFalse("reduce-motion disables text", result.textEnabled)
    }

    // ── #595 四：profile → 约束的生产映射（EditorWindowHost.constraintFor）──

    @Test
    fun systemSuppressedProfileYieldsForceStatic() {
        // Search/Token/RepositoryUrl/BranchName/ReplaceQuery 都是 SYSTEM_SUPPRESSED
        val constraint = EditorWindowHost.constraintFor(TextEditorProfile.SearchQuery)
        assertTrue("SearchQuery must be forceStatic", constraint.forceStatic)

        val policy = EditorMotionPolicy(textEnabled = true, coordinated = true)
        val effective = constraint.apply(policy).effective()
        assertFalse("SYSTEM_SUPPRESSED profile must suppress text", effective.textEnabled)
    }

    @Test
    fun documentBodyProfileYieldsNoConstraint() {
        val constraint = EditorWindowHost.constraintFor(TextEditorProfile.DocumentBody)
        assertEquals(TargetMotionConstraint(), constraint)
    }

    @Test
    fun shortTitleProfileYieldsNoConstraint() {
        val constraint = EditorWindowHost.constraintFor(TextEditorProfile.ShortTitle)
        assertEquals(TargetMotionConstraint(), constraint)
    }

    @Test
    fun nullProfileYieldsNoConstraint() {
        assertEquals(TargetMotionConstraint(), EditorWindowHost.constraintFor(null))
    }

    @Test
    fun explicitEnabledPolicyYieldsNoConstraint() {
        val profile = TextEditorProfile(animationPolicy = AnimationPolicy.ENABLED)
        assertEquals(TargetMotionConstraint(), EditorWindowHost.constraintFor(profile))
    }
}
