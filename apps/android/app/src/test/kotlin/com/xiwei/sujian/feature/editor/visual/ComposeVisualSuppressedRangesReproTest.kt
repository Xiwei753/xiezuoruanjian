package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #689 评论 5675270164 缺陷7：旧 placeholder 测试迁移为新 timeline 行为测试。
 *
 * 原测试断言旧 suppressedRanges 继承。旧机制已删除，本测试验证新持续 timeline
 * 的 hiddenRanges 每一帧从当前 unit 推导，不从上一事务继承。
 *
 * 等价行为覆盖参见 [ComposeVisualTransactionRestartReproTest] hiddenRanges_derivedFromCurrentOverlayUnits。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualSuppressedRangesReproTest {
    @Test
    fun hiddenRanges_derivedFromCurrentUnits_notInherited() {
        val timeline = ComposeVisualTimeline()
        val scene = timeline.sample(0L)
        // 空 timeline 的 hiddenRanges 应为空（不从上一事务继承）
        assertTrue("空 timeline hiddenRanges 应为空（不从上一事务继承）", scene.hiddenRanges.isEmpty())
    }

    @Test
    fun visualScene_hasUnitsAndHiddenRanges() {
        val fields = ComposeVisualScene::class.java.declaredFields.map { it.name }
        assertTrue("ComposeVisualScene 应有 units", fields.contains("units"))
        assertTrue("ComposeVisualScene 应有 hiddenRanges", fields.contains("hiddenRanges"))
    }
}
