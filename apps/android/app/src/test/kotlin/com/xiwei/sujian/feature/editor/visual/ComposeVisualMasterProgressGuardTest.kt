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
 * 原测试断言旧 masterProgress guard。旧机制已删除，本测试验证新持续 timeline
 * 没有 masterProgress 概念，用 hasActiveVisuals 判断动画状态。
 *
 * 等价行为覆盖参见 [ComposeVisualTransactionRestartReproTest] newModel_doesNotExpose_oldTransactionApis。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualMasterProgressGuardTest {
    @Test
    fun masterProgress_removed_hasActiveVisuals_isNewGuard() {
        val state = ComposeEditorVisualState(targetId = "test-master-progress")
        val methods = ComposeEditorVisualState::class.java.methods.map { it.name }
        assertFalse("旧 reportProgress 应已删除", methods.contains("reportProgress"))
        assertFalse("旧 finishTransaction 应已删除", methods.contains("finishTransaction"))
        assertTrue("新 hasActiveVisuals 应存在（替代 masterProgress guard）", methods.contains("hasActiveVisuals"))
    }

    @Test
    fun patch_doesNotHave_masterProgress() {
        val fields = ComposeVisualPatch::class.java.declaredFields.map { it.name }
        assertFalse("patch 不应有 masterProgress 字段", fields.contains("masterProgress"))
        assertFalse("patch 不应有 textAnimationActive 字段", fields.contains("textAnimationActive"))
    }
}
