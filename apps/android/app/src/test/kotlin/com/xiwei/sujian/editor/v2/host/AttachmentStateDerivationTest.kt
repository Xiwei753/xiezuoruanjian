package com.xiwei.sujian.editor.v2.host

import com.xiwei.sujian.editor.v2.coordinator.ProjectionSnapshot
import com.xiwei.sujian.editor.v2.coordinator.TargetSnapshot
import com.xiwei.sujian.editor.v2.coordinator.WindowBindingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 六：[attachmentStateFromBinding] 派生契约测试 — 验证规范 [WindowBindingState]
 * 到窗口附着语义的纯函数投影，不依赖 Android instrumentation。
 */
class AttachmentStateDerivationTest {

    private val frame = EditorFrameSnapshot(
        scrollX = 1f, scrollY = 2f,
        viewportWidth = 10, viewportHeight = 20,
        hasActiveAnimation = true,
    )
    private val session = TargetSnapshot("text", 0, 1L, 0, 0)
    private val projection = ProjectionSnapshot(scrollX = 3f, scrollY = 4f)

    @Test
    fun idleMapsToIdle() {
        val s = attachmentStateFromBinding(WindowBindingState.Idle, false, null, null)
        assertEquals(EditorAttachmentState.Idle, s)
    }

    @Test
    fun attachingMapsToAttaching() {
        val s = attachmentStateFromBinding(
            WindowBindingState.Attaching("w", "t", 7UL), false, null, null,
        )
        assertTrue(s is EditorAttachmentState.Attaching)
        val a = s as EditorAttachmentState.Attaching
        assertEquals("w", a.windowId)
        assertEquals("t", a.targetId)
        assertEquals(7UL, a.sessionId)
    }

    @Test
    fun attachedNotPausedMapsToAttached() {
        val s = attachmentStateFromBinding(
            WindowBindingState.Attached("w", "t", 7UL), false, frame, projection,
        )
        assertTrue(s is EditorAttachmentState.Attached)
    }

    @Test
    fun attachedPausedWithFrameMapsToPaused() {
        val s = attachmentStateFromBinding(
            WindowBindingState.Attached("w", "t", 7UL), true, frame, projection,
        )
        assertTrue(s is EditorAttachmentState.Paused)
        val p = s as EditorAttachmentState.Paused
        assertEquals("t", p.targetId)
        assertEquals(7UL, p.sessionId)
        assertEquals(frame, p.frameSnapshot)
        assertTrue("Paused preserves active animation flag", p.frameSnapshot.hasActiveAnimation)
    }

    @Test
    fun attachedPausedWithoutFrameFallsBackToAttached() {
        val s = attachmentStateFromBinding(
            WindowBindingState.Attached("w", "t", 7UL), true, null, null,
        )
        assertTrue("No frame snapshot means still Attached", s is EditorAttachmentState.Attached)
    }

    @Test
    fun detachedMapsToDetachedWithSnapshots() {
        val s = attachmentStateFromBinding(
            WindowBindingState.Detached("t", 7UL, session), false, null, projection,
        )
        assertTrue(s is EditorAttachmentState.Detached)
        val d = s as EditorAttachmentState.Detached
        assertEquals("t", d.targetId)
        assertEquals(7UL, d.sessionId)
        assertEquals(session, d.sessionSnapshot)
        assertEquals(projection, d.projectionSnapshot)
    }

    @Test
    fun detachingMapsToDetachedWithSnapshot() {
        val s = attachmentStateFromBinding(
            WindowBindingState.Detaching(session), false, null, projection,
        )
        assertTrue(s is EditorAttachmentState.Detached)
        val d = s as EditorAttachmentState.Detached
        assertEquals(session, d.sessionSnapshot)
        assertEquals(projection, d.projectionSnapshot)
    }

    @Test
    fun committingAndCancellingRemainAttached() {
        val c = attachmentStateFromBinding(
            WindowBindingState.Committing("t", 7UL), false, null, null,
        )
        assertTrue("Committing is still attached", c is EditorAttachmentState.Attached)
        val ca = attachmentStateFromBinding(
            WindowBindingState.Cancelling("t", 7UL), false, null, null,
        )
        assertTrue("Cancelling is still attached", ca is EditorAttachmentState.Attached)
    }

    @Test
    fun pausedIsNotCompletedOrCancelled() {
        val s = attachmentStateFromBinding(
            WindowBindingState.Attached("w", "t", 7UL), true, frame, null,
        )
        assertTrue(s is EditorAttachmentState.Paused)
        assertFalse(s is EditorAttachmentState.Idle)
        assertFalse(s is EditorAttachmentState.Releasing)
    }
}
