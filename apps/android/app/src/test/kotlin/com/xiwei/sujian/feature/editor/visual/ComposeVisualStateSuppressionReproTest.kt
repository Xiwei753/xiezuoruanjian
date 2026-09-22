package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #737：[ComposeEditorVisualState] 状态压制测试 — 验证新协调 motion 架构。
 *
 * 旧架构（已删除）：suppressedCurrentRanges / visualScene StateFlow
 * 新架构：[CoordinatedEditMotion.Sample] / [ComposeEditorDrawSnapshot]
 *
 * 一笔编辑只有一个 motion — hiddenRanges 从 motion sample 推导，不再作为独立状态。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualStateSuppressionReproTest {
    @Test
    fun state_exposes_drawSnapshot_not_suppressedCurrentRanges() {
        val state = ComposeEditorVisualState(targetId = "test-state-suppression")
        val fields = ComposeEditorVisualState::class.java.declaredFields.map { it.name }
        // drawSnapshotState 使用 mutableStateOf 委托，反射字段名带 $delegate 后缀
        assertTrue(
            "state 应有 drawSnapshotState（新 API，委托字段名含 \$delegate）",
            fields.any { it.startsWith("drawSnapshotState") },
        )
        assertFalse("state 不应有 suppressedCurrentRanges（旧 API）", fields.contains("suppressedCurrentRanges"))
        assertFalse("state 不应有 _visualScene（旧 API）", fields.contains("_visualScene"))
    }

    @Test
    fun emptyState_drawSnapshot_motionSample_null() {
        val state = ComposeEditorVisualState(targetId = "test-state-suppression-empty")
        // Issue #737：空状态时 drawSnapshot 的 motionSample 应为 null（无 active motion）
        val snapshot = state.drawSnapshot()
        assertTrue("空状态 motionSample 应为 null", snapshot.motionSample == null)
    }
}
