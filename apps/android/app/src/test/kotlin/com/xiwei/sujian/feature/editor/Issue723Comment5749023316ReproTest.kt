package com.xiwei.sujian.feature.editor

import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #723 评论 5749023316 复现测试 —
 *
 * 复核分支上一轮（评论 5748592923）已经把字号/行距移回写作区、协同模式隐藏独立控件、
 * 空段落手工 +X 删除。但还剩两个缺口，本测试原断言两个缺口在当前代码中仍然存在
 * （断言 buggy 行为，证明缺口未修）。
 *
 * Issue #723 评论 5749023316 修复后：缺口2 已通过 EditorMotionPolicy.effective() 归一修复。
 * 缺口1（系统 caret 所有权）在 Issue #725 评论 5750735497 停止自绘屏幕 caret 后不再适用 —
 * drawsVisualCursor/cursorOwnedByVisual/initialDrawsVisualCursor 已删除，相关测试已移除。
 *
 * ## 缺口 2（已修复）：协同动画策略层未收死旧状态
 *
 * 修复方式：EditorMotionPolicy.effective() 在 coordinated=true 时强制
 * textEnabled=true，收死旧持久化状态。
 *
 * Issue #725：自绘 caret 已删除，cursorEnabled 不再参与计算。
 */
@Suppress("MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue723Comment5749023316ReproTest {
    // ==================== 缺口 2：协同动画策略层未收死旧状态 ====================

    /**
     * 缺口 2-1（已修复）：`EditorMotionPolicy(coordinated=true, textEnabled=false).effective()`
     * 现在返回 `textEnabled=true`——协同已开启时策略层强制文字动画开启。
     *
     * Issue #723 评论 5749023316 缺口2修复：effective() 在 coordinated=true 时
     * 强制 textEnabled=true，收死旧持久化状态。
     */
    @Test
    fun gap2_effective_keepsTextEnabledFalse_whenCoordinatedTrueAndTextDisabled() {
        // 旧持久化状态：协同已开启，但文字动画被旧设置关掉
        val legacyPolicy =
            EditorMotionPolicy(
                textEnabled = false,
                coordinated = true,
                reduceMotion = false,
            )
        val effective = legacyPolicy.effective()

        // 修复后：coordinated=true → effective() 强制 textEnabled=true
        assertTrue(
            "修复后：coordinated=true && textEnabled=false 时 effective() 强制 textEnabled=true——" +
                "策略层收死旧状态，协同动画文字部分不被暗中关闭",
            effective.textEnabled,
        )
        assertTrue(
            "协同标记仍为 true（页面认为协同已开启，独立开关已藏）",
            effective.coordinated,
        )
    }

    /**
     * 缺口 2-2（已修复）：`effective()` 现在同时处理 reduceMotion 和 coordinated 归一。
     *
     * 修复后：reduceMotion=true 强制全 false；coordinated=true 强制 textEnabled=true。
     * 两个语义对称——都有策略层保证。
     */
    @Test
    fun gap2_effective_onlyHandlesReduceMotion_notCoordinatedNormalization() {
        // reduceMotion=true → effective() 强制全 false（收口）
        val reduceMotionPolicy =
            EditorMotionPolicy(
                textEnabled = true,
                coordinated = true,
                reduceMotion = true,
            )
        val reduceMotionEffective = reduceMotionPolicy.effective()
        assertFalse(
            "reduceMotion=true → effective() 强制 textEnabled=false（有策略层保证）",
            reduceMotionEffective.textEnabled,
        )

        // coordinated=true 但 textEnabled=false → effective() 强制 textEnabled=true（收口）
        val coordinatedLegacyPolicy =
            EditorMotionPolicy(
                textEnabled = false,
                coordinated = true,
                reduceMotion = false,
            )
        val coordinatedEffective = coordinatedLegacyPolicy.effective()
        assertTrue(
            "修复后：coordinated=true 但 textEnabled=false → effective() 强制 textEnabled=true" +
                "（策略层保证协同语义，与 reduceMotion 对称）",
            coordinatedEffective.textEnabled,
        )
    }
}
