package com.xiwei.sujian.editor.v2.host

import com.xiwei.sujian.editor.v2.coordinator.ProjectionSnapshot
import com.xiwei.sujian.editor.v2.coordinator.TargetSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 六：EditorAttachmentState sealed 状态机契约测试 — 验证状态区分和数据保持。
 */
class EditorAttachmentStateTest {

    @Test
    fun idleIsObjectSingleton() {
        val a = EditorAttachmentState.Idle
        val b = EditorAttachmentState.Idle
        assertTrue("Idle is singleton", a === b)
    }

    @Test
    fun attachingCarriesWindowTargetSession() {
        val state = EditorAttachmentState.Attaching("w1", "t1", 42UL)
        assertEquals("w1", state.windowId)
        assertEquals("t1", state.targetId)
        assertEquals(42UL, state.sessionId)
    }

    @Test
    fun attachedCarriesWindowTargetSession() {
        val state = EditorAttachmentState.Attached("w1", "t1", 42UL)
        assertEquals("w1", state.windowId)
        assertEquals("t1", state.targetId)
        assertEquals(42UL, state.sessionId)
    }

    @Test
    fun pausedCarriesFrameSnapshot() {
        val frame = EditorFrameSnapshot(
            scrollX = 10f, scrollY = 20f,
            viewportWidth = 100, viewportHeight = 200,
            hasActiveAnimation = true,
        )
        val state = EditorAttachmentState.Paused("t1", 42UL, frame)
        assertEquals("t1", state.targetId)
        assertEquals(42UL, state.sessionId)
        assertEquals(frame, state.frameSnapshot)
        assertTrue("Paused preserves active animation flag", state.frameSnapshot.hasActiveAnimation)
    }

    @Test
    fun detachedCarriesSessionAndProjectionSnapshots() {
        val session = TargetSnapshot("text", 0, 1L, 0, 0)
        val projection = ProjectionSnapshot(scrollX = 5f, scrollY = 10f)
        val state = EditorAttachmentState.Detached("t1", 42UL, session, projection)
        assertEquals("t1", state.targetId)
        assertEquals(42UL, state.sessionId)
        assertEquals(session, state.sessionSnapshot)
        assertEquals(projection, state.projectionSnapshot)
    }

    @Test
    fun detachedAllowsNullSnapshots() {
        val state = EditorAttachmentState.Detached("t1", 42UL, null)
        assertNull(state.sessionSnapshot)
        assertNull(state.projectionSnapshot)
    }

    @Test
    fun releasingIsObjectSingleton() {
        val a = EditorAttachmentState.Releasing
        val b = EditorAttachmentState.Releasing
        assertTrue("Releasing is singleton", a === b)
    }

    @Test
    fun distinctStatesAreNotEqual() {
        val idle = EditorAttachmentState.Idle
        val attaching = EditorAttachmentState.Attaching("w1", "t1", 1UL)
        val attached = EditorAttachmentState.Attached("w1", "t1", 1UL)
        val releasing = EditorAttachmentState.Releasing

        assertTrue("Idle != Attaching", idle != attaching)
        assertTrue("Attaching != Attached", attaching != attached)
        assertTrue("Attached != Releasing", attached != releasing)
        assertTrue("Idle != Releasing", idle != releasing)
    }
}
