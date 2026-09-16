package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #689 评论 5675270164 缺陷7：旧 placeholder 测试迁移为新 timeline 行为测试。
 *
 * 原测试断言旧 state suppression。旧机制已删除，本测试验证新持续 timeline
 * 的 hiddenRanges 从当前 overlay unit 推导而非继承。
 *
 * #698 评论 5697612595：ComposeEditorVisualState 对外 hiddenRanges StateFlow 已删除
 * （draw 层改用背景色填充字形 path 裁切，不再通过 OutputTransformation 把 range 设 Transparent）。
 * 改成检查 visualScene StateFlow 存在，scene.hiddenRanges 仍可读（draw 层裁切用）。
 *
 * 等价行为覆盖参见 [ComposeVisualTransactionRestartReproTest] hiddenRanges_derivedFromCurrentOverlayUnits。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualStateSuppressionReproTest {
    @Test
    fun state_exposes_visualScene_not_suppressedCurrentRanges() {
        val state = ComposeEditorVisualState(targetId = "test-state-suppression")
        val fields = ComposeEditorVisualState::class.java.declaredFields.map { it.name }
        assertTrue("state 应有 visualScene（新 API）", fields.contains("_visualScene"))
        assertFalse("state 不应有 suppressedCurrentRanges（旧 API）", fields.contains("suppressedCurrentRanges"))
    }

    @Test
    fun emptyState_visualScene_hiddenRanges_empty() {
        val state = ComposeEditorVisualState(targetId = "test-state-suppression-empty")
        assertTrue("空状态 scene.hiddenRanges 应为空", state.visualScene.value.hiddenRanges.isEmpty())
    }
}
