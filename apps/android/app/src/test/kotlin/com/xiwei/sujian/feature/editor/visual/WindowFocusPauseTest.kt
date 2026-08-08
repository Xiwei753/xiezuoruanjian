package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 六/七：窗口焦点暂停契约测试 — 验证 pause/resume 不取消事务，
 * 动画从暂停帧继续而非从头开始。
 */
class WindowFocusPauseTest {
    @Test
    fun pausePreservesProgressAndResumeContinues() {
        val timeline = AnimationTimeline(durationMs = 100L, submittedAtMs = 0L)
        timeline.markFirstVisibleFrame(0L)

        val progressBefore = timeline.progress(50L)
        assertEquals(0.5f, progressBefore, 0.01f)

        timeline.pause(50L)
        assertTrue("Timeline is paused", timeline.isPaused())
        assertEquals("Paused progress equals pre-pause", progressBefore, timeline.progress(60L), 0.01f)
        assertEquals("Paused progress stays constant", progressBefore, timeline.progress(100L), 0.01f)

        timeline.resume(100L)
        assertFalse("Timeline is not paused after resume", timeline.isPaused())
        val progressAfter = timeline.progress(100L)
        assertEquals("Resume continues from paused progress", progressBefore, progressAfter, 0.01f)
    }

    @Test
    fun pauseDoesNotCompleteOrCancelTransaction() {
        val timeline = AnimationTimeline(durationMs = 100L, submittedAtMs = 0L)
        timeline.markFirstVisibleFrame(0L)

        timeline.pause(30L)
        val state = timeline.getState()
        assertEquals(TransactionState.Paused, state)
        assertFalse("Paused is not Completed", state == TransactionState.Completed)
        assertFalse("Paused is not Cancelled", state == TransactionState.Cancelled)
    }

    @Test
    fun resumeAfterPauseReachesCompletion() {
        val timeline = AnimationTimeline(durationMs = 100L, submittedAtMs = 0L)
        timeline.markFirstVisibleFrame(0L)

        timeline.pause(40L)
        assertEquals(0.4f, timeline.progress(40L), 0.01f)

        timeline.resume(40L)
        assertTrue(
            "After resume, progress continues past pause point",
            timeline.progress(100L) > 0.4f,
        )
        assertTrue(
            "Eventually reaches completion",
            timeline.progress(200L) >= 1f,
        )
    }

    @Test
    fun pauseBeforeFirstVisibleFrameResetsToPending() {
        val timeline = AnimationTimeline(durationMs = 100L)
        assertEquals(TransactionState.Pending, timeline.getState())

        timeline.pause(50L)
        timeline.resume(60L)
        assertEquals("Resets to Pending", TransactionState.Pending, timeline.getState())

        timeline.markFirstVisibleFrame(60L)
        assertEquals(TransactionState.Rendering, timeline.getState())
        assertEquals(0f, timeline.progress(60L), 0.01f)
    }
}
