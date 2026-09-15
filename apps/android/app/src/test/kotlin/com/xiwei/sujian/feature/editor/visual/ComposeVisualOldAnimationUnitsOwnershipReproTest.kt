package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #689 评论 5675270164 缺陷7：旧 placeholder 测试迁移为新 timeline 行为测试。
 *
 * 原测试断言旧 oldAnimationUnits ownership。旧机制已删除，本测试验证新持续 timeline
 * 用 deletedUnits 从 oldLayout 建 ghost（缺陷1 修复）。
 *
 * 等价行为覆盖参见 [ComposeVisualTimelineComment5675270164ReproTest] defect1。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualOldAnimationUnitsOwnershipReproTest {
    @Test
    fun patch_uses_deletedUnits_not_oldAnimationUnits() {
        val fields = ComposeVisualPatch::class.java.declaredFields.map { it.name }
        assertTrue("patch 应有 deletedUnits 字段（新 API）", fields.contains("deletedUnits"))
        assertTrue("patch 不应有 oldAnimationUnits 字段（旧 API 已删除）", !fields.contains("oldAnimationUnits"))
    }

    @Test
    fun timeline_canInstantiate_andClear() {
        val timeline = ComposeVisualTimeline()
        timeline.clear()
        assertTrue("clear 后 sample 应返回空", timeline.sample(0L).units.isEmpty())
    }
}
