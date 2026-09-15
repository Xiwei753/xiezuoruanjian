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
 * 原测试断言旧 patch adversarial 场景。旧机制已删除，本测试验证新持续 timeline
 * 的 patch 是屏幕 diff，不携带旧事务状态。
 *
 * 等价行为覆盖参见 [ComposeVisualTransactionRestartReproTest] patch_doesNotCarry_oldTransactionState。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualPatchAdversarialTest {
    @Test
    fun patch_isScreenDiff_notTransactionState() {
        val fields = ComposeVisualPatch::class.java.declaredFields.map { it.name }
        assertTrue("patch 应有 insertedUnits", fields.contains("insertedUnits"))
        assertTrue("patch 应有 deletedUnits", fields.contains("deletedUnits"))
        assertTrue("patch 应有 retainedMoves", fields.contains("retainedMoves"))
        assertTrue("patch 应有 offsetMap", fields.contains("offsetMap"))
        assertFalse("patch 不应有 textAnimationActive", fields.contains("textAnimationActive"))
        assertFalse("patch 不应有 cursorAnimationActive", fields.contains("cursorAnimationActive"))
    }

    @Test
    fun retainedMove_hasOldAndNewRange() {
        val fields = RetainedMove::class.java.declaredFields.map { it.name }
        assertTrue("RetainedMove 应有 oldRange", fields.contains("oldRange"))
        assertTrue("RetainedMove 应有 newRange", fields.contains("newRange"))
    }
}
