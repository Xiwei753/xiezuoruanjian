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
 * ## 缺口 2（Issue #732 评论 5763493968 第4节重新收口）：协同动画是完整模式
 *
 * Issue #732 评论 5763493968 第4节：coordinated=true 定义成一种完整模式，
 * 不再靠 effective() 强制 textEnabled=true 来成立。
 * effective() 删除 `coordinated -> copy(textEnabled = true, cursorEnabled = true)` 归一，
 * coordinated=true 时直接返回 this。
 * 协同模式统一用 textDurationMillis 作为这一笔 ComposeEditMotion 的时长；
 * 独立的 textEnabled/cursorEnabled/cursorDurationMillis 只在 coordinated=false 时生效。
 *
 * Issue #725：自绘 caret 已删除，cursorEnabled 不再参与计算。
 */
@Suppress("MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue723Comment5749023316ReproTest {
    // ==================== 缺口 2：协同动画是完整模式（#732 评论 5763493968 第4节） ====================

    /**
     * 缺口 2-1（#732 重新收口）：`EditorMotionPolicy(coordinated=true, textEnabled=false).effective()`
     * 现在直接返回 this——coordinated 本身就是完整模式，不靠改 textEnabled 才成立。
     *
     * 旧归一（#723 评论 5749023316）在 effective() 里强制 textEnabled=true，
     * 但 #732 评论 5763493968 第4节删除了这条归一：协同本身就是完整模式，
     * 不能靠改另一个隐藏设置才能成立。
     */
    @Test
    fun gap2_effective_coordinatedTrue_isIdentity_preservesTextEnabledFalse() {
        // 旧持久化状态：协同已开启，但文字动画被旧设置关掉
        val legacyPolicy =
            EditorMotionPolicy(
                textEnabled = false,
                coordinated = true,
                reduceMotion = false,
            )
        val effective = legacyPolicy.effective()

        // #732 收口后：coordinated=true → effective() 直接返回 this，不强制 textEnabled=true
        assertFalse(
            "#732 收口后：coordinated=true && textEnabled=false 时 effective() 保持 textEnabled=false——" +
                "coordinated 本身就是完整模式，不靠改 textEnabled 才成立",
            effective.textEnabled,
        )
        assertTrue(
            "协同标记仍为 true（页面认为协同已开启，独立开关已藏）",
            effective.coordinated,
        )
    }

    /**
     * 缺口 2-2（#732 重新收口）：`effective()` 只处理 reduceMotion，不再做 coordinated 归一。
     *
     * reduceMotion=true 仍强制全 false（优先级最高）。
     * coordinated=true 但 textEnabled=false 时 effective() 直接返回 this（不强制 textEnabled=true）。
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

        // coordinated=true 但 textEnabled=false → effective() 直接返回 this（#732 收口）
        val coordinatedLegacyPolicy =
            EditorMotionPolicy(
                textEnabled = false,
                coordinated = true,
                reduceMotion = false,
            )
        val coordinatedEffective = coordinatedLegacyPolicy.effective()
        assertFalse(
            "#732 收口后：coordinated=true 但 textEnabled=false → effective() 保持 textEnabled=false" +
                "（coordinated 本身就是完整模式，不靠改 textEnabled 才成立）",
            coordinatedEffective.textEnabled,
        )
    }
}
