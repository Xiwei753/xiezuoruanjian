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
 * 原测试断言旧 ComposeEditorVisualState 事务 API。旧机制已删除，本测试验证
 * 新 ComposeEditorVisualState 暴露 latestPatch / visualScene，
 * 不暴露 activeTransaction / masterProgress。
 *
 * #698 评论 5697612595：ComposeEditorVisualState 对外 hiddenRanges StateFlow 已删除
 * （draw 层改用背景色填充字形 path 裁切）。改成检查 visualScene StateFlow 存在。
 *
 * 等价行为覆盖参见 [ComposeVisualTransactionRestartReproTest] newModel_doesNotExpose_oldTransactionApis。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeEditorVisualStateTest {
    @Test
    fun newApi_present_oldApi_removed() {
        val state = ComposeEditorVisualState(targetId = "test-visual-state")
        val methods = ComposeEditorVisualState::class.java.methods.map { it.name }
        assertTrue("应有 drainPendingPatchesAtFrame", methods.contains("drainPendingPatchesAtFrame"))
        assertTrue("应有 sampleVisualScene", methods.contains("sampleVisualScene"))
        assertTrue("应有 hasActiveVisuals", methods.contains("hasActiveVisuals"))
        assertFalse("不应有 reportProgress", methods.contains("reportProgress"))
        assertFalse("不应有 finishTransaction", methods.contains("finishTransaction"))
    }

    @Test
    fun state_exposes_latestPatch_and_visualScene() {
        val fields = ComposeEditorVisualState::class.java.declaredFields.map { it.name }
        assertTrue("state 应有 _latestPatch", fields.contains("_latestPatch"))
        assertTrue("state 应有 _visualScene", fields.contains("_visualScene"))
    }

    @Test
    fun clear_resetsState() {
        val state = ComposeEditorVisualState(targetId = "test-visual-state-clear")
        state.clear()
        assertFalse("clear 后不应有活动动画", state.hasActiveVisuals(0L))
        assertTrue("clear 后 scene.hiddenRanges 为空", state.visualScene.value.hiddenRanges.isEmpty())
    }
}
