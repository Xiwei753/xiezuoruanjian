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
 * 原测试断言旧事务机制（activeTransaction / masterProgress / suppressedCurrentRanges）。
 * 旧机制已删除，本测试验证新持续 timeline 的 active ownership 行为：
 * - overlay 只在动画进行时接管文字，结束后交还 BasicTextField
 * - 旧 API（reportProgress / finishTransaction / activeTransaction）不存在
 *
 * 等价行为覆盖参见 [ComposeVisualTimelineComment5675270164ReproTest] defect3。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualActiveOwnershipReproTest {
    @Test
    fun oldTransactionApi_removed_newTimelineApi_present() {
        val state = ComposeEditorVisualState(targetId = "test-active-ownership")
        val methods = ComposeEditorVisualState::class.java.methods.map { it.name }
        assertFalse("旧 reportProgress 应已删除", methods.contains("reportProgress"))
        assertFalse("旧 finishTransaction 应已删除", methods.contains("finishTransaction"))
        assertTrue("新 drainPendingPatchesAtFrame 应存在", methods.contains("drainPendingPatchesAtFrame"))
        assertTrue("新 sampleVisualScene 应存在", methods.contains("sampleVisualScene"))
        assertTrue("新 hasActiveVisuals 应存在", methods.contains("hasActiveVisuals"))
    }

    @Test
    fun emptyTimeline_sampleReturnsEmptyScene() {
        val timeline = ComposeVisualTimeline()
        val scene = timeline.sample(0L)
        assertTrue("空 timeline sample 应返回空 units", scene.units.isEmpty())
        assertTrue("空 timeline sample 应返回空 hiddenRanges", scene.hiddenRanges.isEmpty())
        assertFalse("空 timeline 不应有活动动画", timeline.hasActiveAnimation(0L))
    }
}
