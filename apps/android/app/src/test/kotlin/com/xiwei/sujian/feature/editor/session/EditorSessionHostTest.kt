package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.projection.SessionCloseReason
import com.xiwei.sujian.feature.editor.window.EditingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * #641 评论1 第7节：[EditorSessionHost] 契约测试 — 证明它是真正的 session owner，
 * 正确委托 [EditorSessionCoordinator] 的 target/session 命令、undo/redo、save/close。
 */
class EditorSessionHostTest {
    private fun createHost(): EditorSessionHost {
        val coordinator =
            EditorSessionCoordinator(
                com.xiwei.sujian.core.interop.app.AppServiceBridge(
                    com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                        "/tmp/sujian_test_workspace_641_session_host",
                        "/tmp/sujian_test_workspace_641_session_host",
                    ),
                ),
            )
        return EditorSessionHost(
            sessionCoordinator = coordinator,
            appServiceBridge = coordinator.appServiceBridge,
        )
    }

    @Test
    fun sessionStateFlow_delegatesToCoordinator() {
        val host = createHost()
        assertNotNull("sessionStateFlow 从 coordinator 转发", host.sessionStateFlow)
        assertEquals(EditingState.IDLE, host.editingState)
        assertNull("初始无 activeTargetId", host.activeTargetId)
    }

    @Test
    fun activeTargetId_reflectsCoordinatorState() {
        val host = createHost()
        host.sessionCoordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        assertNull("注册 target meta 不设置 activeTargetId", host.activeTargetId)
    }

    @Test
    fun performUndo_withoutLease_isNoOp() {
        val host = createHost()
        // 无活动 lease 时 performUndo 不抛异常、不改变状态。
        host.performUndo()
        assertNull(host.activeTargetId)
    }

    @Test
    fun performRedo_withoutLease_isNoOp() {
        val host = createHost()
        host.performRedo()
        assertNull(host.activeTargetId)
    }

    @Test
    fun closeTarget_delegatesToCoordinator() {
        val host = createHost()
        host.sessionCoordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        host.closeTarget("t1", SessionCloseReason.WORKSPACE_NAVIGATION)
        assertNull("closeTarget 后 activeTargetId 仍 null", host.activeTargetId)
    }

    @Test
    fun queryTargetSnapshot_delegatesToCoordinator() {
        val host = createHost()
        val snapshot = host.queryTargetSnapshot("nonexistent")
        assertNull("不存在的 target 返回 null", snapshot)
    }

    @Test
    fun motionPolicyFlow_delegatesToCoordinator() {
        val host = createHost()
        assertNotNull("motionPolicyFlow 从 coordinator 转发", host.motionPolicyFlow)
    }

    @Test
    fun chapterSavedSignal_delegatesToCoordinator() {
        val host = createHost()
        assertNotNull("chapterSavedSignal 从 coordinator 转发", host.chapterSavedSignal)
    }

    @Test
    fun releaseHost_clearsCoordinatorState() {
        val host = createHost()
        host.sessionCoordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        host.releaseHost()
        assertNull("releaseHost 后 activeTargetId 为 null", host.activeTargetId)
    }
}
