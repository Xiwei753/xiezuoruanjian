package com.xiwei.sujian.editor.v2

import com.xiwei.sujian.editor.v2.visual.AnimationTimeline
import com.xiwei.sujian.editor.v2.visual.TransactionState
import org.junit.Assert.*
import org.junit.Test

class AnimationTimelineTest {

    @Test
    fun initialProgressIsZero() {
        val timeline = AnimationTimeline(100)
        assertEquals(0f, timeline.progress(0), 0.01f)
    }

    @Test
    fun progressAdvancesAfterFirstFrame() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        val p = timeline.progress(1050)
        assertEquals(0.5f, p, 0.01f)
    }

    @Test
    fun staleFrameTimeBeforeSubmissionDoesNotAnchorTimelineInThePast() {
        // A stale pending frame time from the previous animation must not anchor a newly
        // submitted timeline before its submission: the first visible frame can only be
        // stamped at or after the submission moment.
        val timeline = AnimationTimeline(100, submittedAtMs = 320)
        timeline.markFirstVisibleFrame(256)
        assertEquals(320L, timeline.getFirstVisibleFrameTimeMs())
        assertEquals(0f, timeline.progress(320), 0.01f)
        assertEquals(0.4f, timeline.progress(360), 0.01f)
    }

    @Test
    fun frameTimeAfterSubmissionAnchorsNormally() {
        val timeline = AnimationTimeline(100, submittedAtMs = 320)
        timeline.markFirstVisibleFrame(350)
        assertEquals(350L, timeline.getFirstVisibleFrameTimeMs())
        assertEquals(0.5f, timeline.progress(400), 0.01f)
    }

    @Test
    fun progressReachesOneAtDuration() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        val p = timeline.progress(1100)
        assertEquals(1f, p, 0.01f)
    }

    @Test
    fun progressClampsToOne() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        val p = timeline.progress(2000)
        assertEquals(1f, p, 0.01f)
    }

    @Test
    fun zeroDurationReturnsOne() {
        val timeline = AnimationTimeline(0)
        timeline.markFirstVisibleFrame(1000)
        assertEquals(1f, timeline.progress(1000), 0.01f)
    }

    @Test
    fun pauseFreezesProgress() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        val pBefore = timeline.progress(1025)
        timeline.pause(1025)
        val pAfter = timeline.progress(1050)
        assertEquals(pBefore, pAfter, 0.001f)
        assertTrue(timeline.isPaused())
    }

    @Test
    fun resumeContinuesFromPausedProgress() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        timeline.pause(1025)
        timeline.resume(1050)
        assertFalse(timeline.isPaused())
        assertTrue(timeline.progress(1050) > 0f)
    }

    @Test
    fun completeSetsState() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        timeline.complete()
        assertEquals(TransactionState.Completed, timeline.getState())
    }

    @Test
    fun cancelSetsState() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        timeline.cancel()
        assertEquals(TransactionState.Cancelled, timeline.getState())
    }

    @Test
    fun isCompletedReturnsTrueWhenProgressReachesOne() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        assertTrue(timeline.isCompleted(1100))
        assertFalse(timeline.isCompleted(1050))
    }

    @Test
    fun currentVisualFrameReturnsFrameForPendingState() {
        val timeline = AnimationTimeline(100)
        val frame = timeline.currentVisualFrame(0)
        assertNotNull("Pending state must return a valid frame", frame)
        assertEquals(TransactionState.Pending, frame!!.state)
        assertEquals(0f, frame.progress, 0.01f)
    }

    @Test
    fun currentVisualFrameReturnsFrameForRenderingState() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        val frame = timeline.currentVisualFrame(1050)
        assertNotNull(frame)
        assertEquals(TransactionState.Rendering, frame!!.state)
    }

    @Test
    fun currentVisualFrameReturnsNullForCompletedState() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        timeline.complete()
        assertNull(timeline.currentVisualFrame(1050))
    }

    @Test
    fun currentVisualFrameReturnsNullForCancelledState() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        timeline.cancel()
        assertNull(timeline.currentVisualFrame(1050))
    }
}
