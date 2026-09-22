package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #737：[ComposeEditorVisualState] API 测试 — 验证新协调 motion 架构的公共 API。
 *
 * 旧架构（已删除）：visualScene StateFlow / activeTransaction / masterProgress
 * 新架构：[CoordinatedEditMotion] / [ComposeEditorDrawSnapshot] / drawSnapshot()
 *
 * 一笔编辑只有一个 motion — caret 和吞字/吐字共用同一个 traversal、同一个 progress。
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
    fun state_exposes_latestPatch_and_drawSnapshot() {
        val fields = ComposeEditorVisualState::class.java.declaredFields.map { it.name }
        assertTrue("state 应有 _latestPatch", fields.contains("_latestPatch"))
        // drawSnapshotState 使用 mutableStateOf 委托，反射字段名带 $delegate 后缀
        assertTrue(
            "state 应有 drawSnapshotState（新 API，委托字段名含 \$delegate）",
            fields.any { it.startsWith("drawSnapshotState") },
        )
        assertFalse("state 不应有 _visualScene（旧 API）", fields.contains("_visualScene"))
    }

    @Test
    fun clear_resetsState() {
        val state = ComposeEditorVisualState(targetId = "test-visual-state-clear")
        state.clear()
        assertFalse("clear 后不应有活动动画", state.hasActiveVisuals(0L))
        // Issue #737：clear 后 drawSnapshot 的 motionSample 应为 null（无 active motion）
        val snapshot = state.drawSnapshot()
        assertTrue("clear 后 motionSample 应为 null", snapshot.motionSample == null)
    }
}
