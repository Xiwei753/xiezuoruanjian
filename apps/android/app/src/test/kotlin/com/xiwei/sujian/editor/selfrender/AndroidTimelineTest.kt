package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test

class AndroidTimelineTest {

    @Test
    fun progress_beforeStart_returnsZero() {
        val timeline = AndroidTimeline(160L)
        assertEquals(0f, timeline.progress(1000L), 0.001f)
    }

    @Test
    fun progress_zeroDuration_returnsOne() {
        val timeline = AndroidTimeline(0L)
        timeline.recordFirstFrame(1000L)
        assertEquals(1f, timeline.progress(1000L), 0.001f)
    }

    @Test
    fun progress_afterStart_advances() {
        val timeline = AndroidTimeline(160L)
        timeline.recordFirstFrame(1000L)
        val p = timeline.progress(1080L)
        assertTrue("Progress should be around 0.5", p in 0.4f..0.6f)
    }

    @Test
    fun progress_afterComplete_returnsOne() {
        val timeline = AndroidTimeline(160L)
        timeline.recordFirstFrame(1000L)
        assertEquals(1f, timeline.progress(1200L), 0.001f)
    }

    @Test
    fun paused_returnsPausedProgress_notZero() {
        val timeline = AndroidTimeline(160L)
        timeline.recordFirstFrame(1000L)
        timeline.pause(1080L)
        assertTrue("Paused progress should be around 0.5", timeline.pausedProgress in 0.4f..0.6f)
        assertTrue("Should be paused", timeline.isPaused)
        val p = timeline.progress(2000L)
        assertTrue("Paused progress should not be 0", p > 0.3f)
    }

    @Test
    fun resume_continuesFromPausedProgress() {
        val timeline = AndroidTimeline(160L)
        timeline.recordFirstFrame(1000L)
        timeline.pause(1080L)
        val pausedP = timeline.pausedProgress
        timeline.resume(2000L)
        val p = timeline.progress(2000L)
        assertTrue("Resume should continue from paused progress", p >= pausedP - 0.01f)
    }

    @Test
    fun pauseResume_accumulatesPausedDuration() {
        val timeline = AndroidTimeline(160L)
        timeline.recordFirstFrame(1000L)
        timeline.pause(1040L)
        timeline.resume(1140L)
        assertEquals(100L, timeline.accumulatedPausedDurationMs)
        val p = timeline.progress(1180L)
        assertTrue("Progress should account for paused duration", p in 0.2f..0.5f)
    }
}

class AndroidPlatformVisualTransactionPausedTest {

    @Test
    fun pausedState_returnsPausedProgress_notZero() {
        val tx = AndroidPlatformVisualTransaction(
            key = 1u,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Insert,
            animationMode = com.xiwei.sujian.model.AnimationModeData.GlyphAnimation,
            durationMs = 160L,
            oldRevision = 0L,
            newRevision = 1L,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF(0f, 0f, 0f, 0f))
        )

        tx.markPrepared()
        tx.markRendering()

        tx.timeline.recordFirstFrame(1000L)

        tx.pause()
        val pausedProgress = tx.progress
        assertTrue("Paused progress must not be 0", pausedProgress > 0f)
        assertTrue("Paused progress must not be 1", pausedProgress < 1f)
    }

    @Test
    fun pendingState_returnsZeroProgress() {
        val tx = AndroidPlatformVisualTransaction(
            key = 1u,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Insert,
            animationMode = com.xiwei.sujian.model.AnimationModeData.GlyphAnimation,
            durationMs = 160L,
            oldRevision = 0L,
            newRevision = 1L,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF(0f, 0f, 0f, 0f))
        )

        assertEquals(0f, tx.progress, 0.001f)
    }
}
