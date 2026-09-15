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
 * 原测试断言旧 finishTransaction guard。旧机制已删除，本测试验证新持续 timeline
 * 用 hasActiveVisuals 判断动画是否完成（替代旧 finishTransaction）。
 *
 * 等价行为覆盖参见 [ComposeVisualTimelineComment5675270164ReproTest] defect3。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualFinishTransactionGuardReproTest {
    @Test
    fun finishTransaction_removed_hasActiveVisuals_isNewGuard() {
        val state = ComposeEditorVisualState(targetId = "test-finish-guard")
        val methods = ComposeEditorVisualState::class.java.methods.map { it.name }
        assertFalse("旧 finishTransaction 应已删除", methods.contains("finishTransaction"))
        assertTrue("新 hasActiveVisuals 应存在（替代 finishTransaction guard）", methods.contains("hasActiveVisuals"))
    }

    @Test
    fun emptyState_hasActiveVisuals_false() {
        val state = ComposeEditorVisualState(targetId = "test-finish-guard-empty")
        assertFalse("空状态不应有活动动画", state.hasActiveVisuals(0L))
    }
}
