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
 * 原测试断言旧 layout-first policy。旧机制已删除，本测试验证新持续 timeline
 * 的 patch 携带 oldLayout/newLayout（layout-first 语义保留）。
 *
 * 等价行为覆盖参见 [ComposeVisualTransactionRestartReproTest] patch_doesNotCarry_oldTransactionState。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualLayoutFirstPolicyReproTest {
    @Test
    fun patch_carries_oldLayout_and_newLayout() {
        val fields = ComposeVisualPatch::class.java.declaredFields.map { it.name }
        assertTrue("patch 应有 oldLayout 字段（layout-first）", fields.contains("oldLayout"))
        assertTrue("patch 应有 newLayout 字段（layout-first）", fields.contains("newLayout"))
    }

    @Test
    fun patch_doesNotCarry_startFrame() {
        val fields = ComposeVisualPatch::class.java.declaredFields.map { it.name }
        assertFalse("patch 不应有 startFrame 字段（旧事务物化已删除）", fields.contains("startFrame"))
    }
}
