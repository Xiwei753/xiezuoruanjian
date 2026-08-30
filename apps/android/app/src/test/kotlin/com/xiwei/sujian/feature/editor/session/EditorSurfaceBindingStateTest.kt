@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复

package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.ui.EditorSurfaceMode
import com.xiwei.sujian.feature.editor.ui.editorSurfaceMode
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #644 评论 5467821839 第6节：编辑器 Surface 绑定状态守卫契约测试。
 *
 * 生产入口已改成 Compose [attachSurface]；本测试直接构造/推进到 Attaching，
 * 验证 attachSurface 只能做 Attaching → Attached；覆盖 window、target、session
 * 任一不匹配返回 null，以及同一 Attached 幂等返回当前 lease。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorSurfaceBindingStateTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595",
                    "/tmp/sujian_test_workspace_595",
                ),
            ),
        )
    }

    private fun setSessionState(
        coordinator: EditorSessionCoordinator,
        state: EditorSessionState,
    ) {
        val field = EditorSessionCoordinator::class.java.getDeclaredField("_sessionStateFlow")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val flow = field.get(coordinator) as kotlinx.coroutines.flow.MutableStateFlow<EditorSessionState>
        flow.value = state
    }

    @Test
    fun attachSurface_fromIdle_returnsNull() {
        val coordinator = createCoordinator()
        val lease = coordinator.attachSurface("w1", "t1")
        assertNull("attachSurface from Idle must return null", lease)
        assertEquals(WindowBindingState.Idle, coordinator.sessionStateFlow.value.bindingState)
    }

    @Test
    fun attachSurface_fromAttaching_withMatchingParams_returnsLeaseAndTransitionsToAttached() {
        val coordinator = createCoordinator()
        val sessionId = 42UL
        setSessionState(
            coordinator,
            EditorSessionState(
                sessionId = sessionId,
                activeTargetId = "t1",
                bindingState = WindowBindingState.Attaching("w1", "t1", sessionId),
            ),
        )
        val lease = coordinator.attachSurface("w1", "t1")
        assertNotNull("attachSurface with matching params must return lease", lease)
        assertEquals("t1", lease!!.targetId)
        assertEquals(sessionId, lease.sessionId)
        val binding = coordinator.sessionStateFlow.value.bindingState
        assertTrue("Must transition to Attached", binding is WindowBindingState.Attached)
        val attached = binding as WindowBindingState.Attached
        assertEquals("w1", attached.windowId)
        assertEquals("t1", attached.targetId)
        assertEquals(sessionId, attached.sessionId)
    }

    @Test
    fun attachSurface_fromAttaching_withWrongWindowId_returnsNull() {
        val coordinator = createCoordinator()
        val sessionId = 42UL
        setSessionState(
            coordinator,
            EditorSessionState(
                sessionId = sessionId,
                activeTargetId = "t1",
                bindingState = WindowBindingState.Attaching("w1", "t1", sessionId),
            ),
        )
        val lease = coordinator.attachSurface("w2", "t1")
        assertNull("attachSurface with wrong windowId must return null", lease)
        assertTrue(
            "Must stay in Attaching",
            coordinator.sessionStateFlow.value.bindingState is WindowBindingState.Attaching,
        )
    }

    @Test
    fun attachSurface_fromAttaching_withWrongTargetId_returnsNull() {
        val coordinator = createCoordinator()
        val sessionId = 42UL
        setSessionState(
            coordinator,
            EditorSessionState(
                sessionId = sessionId,
                activeTargetId = "t1",
                bindingState = WindowBindingState.Attaching("w1", "t1", sessionId),
            ),
        )
        val lease = coordinator.attachSurface("w1", "t2")
        assertNull("attachSurface with wrong targetId must return null", lease)
        assertTrue(
            "Must stay in Attaching",
            coordinator.sessionStateFlow.value.bindingState is WindowBindingState.Attaching,
        )
    }

    @Test
    fun attachSurface_fromAttaching_withWrongSessionId_returnsNull() {
        val coordinator = createCoordinator()
        val sessionId = 42UL
        setSessionState(
            coordinator,
            EditorSessionState(
                sessionId = sessionId,
                activeTargetId = "t1",
                bindingState = WindowBindingState.Attaching("w1", "t1", sessionId),
            ),
        )
        // Change sessionId in state to simulate stale Attaching
        setSessionState(
            coordinator,
            EditorSessionState(
                sessionId = 99UL,
                activeTargetId = "t1",
                bindingState = WindowBindingState.Attaching("w1", "t1", sessionId),
            ),
        )
        val lease = coordinator.attachSurface("w1", "t1")
        assertNull("attachSurface with mismatched sessionId must return null", lease)
    }

    @Test
    fun attachSurface_fromAttaching_withZeroSessionId_returnsNull() {
        val coordinator = createCoordinator()
        setSessionState(
            coordinator,
            EditorSessionState(
                sessionId = 0UL,
                activeTargetId = "t1",
                bindingState = WindowBindingState.Attaching("w1", "t1", 0UL),
            ),
        )
        val lease = coordinator.attachSurface("w1", "t1")
        assertNull("attachSurface with sessionId=0 must return null", lease)
    }

    @Test
    fun attachSurface_fromAttached_isIdempotent_returnsCurrentLease() {
        val coordinator = createCoordinator()
        val sessionId = 42UL
        setSessionState(
            coordinator,
            EditorSessionState(
                sessionId = sessionId,
                activeTargetId = "t1",
                editingState = EditingState.EDITING,
                bindingState = WindowBindingState.Attached("w1", "t1", sessionId),
            ),
        )
        val lease = coordinator.attachSurface("w1", "t1")
        assertNotNull("attachSurface from Attached must return lease (idempotent)", lease)
        assertEquals("t1", lease!!.targetId)
        assertEquals(sessionId, lease.sessionId)
        assertTrue(
            "Must stay Attached",
            coordinator.sessionStateFlow.value.bindingState is WindowBindingState.Attached,
        )
    }

    @Test
    fun attachSurface_fromAttached_withWrongSession_returnsNull() {
        val coordinator = createCoordinator()
        val sessionId = 42UL
        setSessionState(
            coordinator,
            EditorSessionState(
                sessionId = 99UL,
                activeTargetId = "t1",
                editingState = EditingState.EDITING,
                bindingState = WindowBindingState.Attached("w1", "t1", sessionId),
            ),
        )
        val lease = coordinator.attachSurface("w1", "t1")
        assertNull("attachSurface with mismatched sessionId on Attached must return null", lease)
    }

    @Test
    fun shouldShowEditor_requiresAttachingOrAttached() {
        val targetId = "t1"
        assertTrue(
            editorSurfaceMode(
                WindowBindingState.Attaching("w", targetId, 1UL),
                "w",
                targetId,
                isActivePane = false,
            ) == EditorSurfaceMode.EditorHost,
        )
        assertTrue(
            editorSurfaceMode(
                WindowBindingState.Attached("w", targetId, 1UL),
                "w",
                targetId,
                isActivePane = false,
            ) == EditorSurfaceMode.EditorHost,
        )
        assertFalse(
            editorSurfaceMode(
                WindowBindingState.Idle,
                "w",
                targetId,
                isActivePane = false,
            ) == EditorSurfaceMode.EditorHost,
        )
        assertFalse(
            editorSurfaceMode(
                WindowBindingState.Detaching(null),
                "w",
                targetId,
                isActivePane = false,
            ) == EditorSurfaceMode.EditorHost,
        )
        assertFalse(
            editorSurfaceMode(
                WindowBindingState.Detached(targetId, 1UL, null),
                "w",
                targetId,
                isActivePane = false,
            ) == EditorSurfaceMode.EditorHost,
        )
    }
}
