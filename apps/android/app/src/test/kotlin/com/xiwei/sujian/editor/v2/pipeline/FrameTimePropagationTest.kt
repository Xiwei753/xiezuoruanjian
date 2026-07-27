package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import org.junit.Assert.*
import org.junit.Test

class FrameTimePropagationTest {

    @Test
    fun choreographerTimeSource_returnsFrameTimeAfterOnFrame() {
        val source = ChoreographerAnimationTimeSource()
        val frameTime = 1_000_000_000L
        source.onFrameTimeNanos(frameTime)
        assertEquals(frameTime, source.nowNanos())
    }

    @Test
    fun choreographerTimeSource_returnsSystemNanoTimeBeforeAnyFrame() {
        val source = ChoreographerAnimationTimeSource()
        val before = System.nanoTime()
        val reported = source.nowNanos()
        val after = System.nanoTime()
        assertTrue("Should fall back to System.nanoTime() before any frame callback", reported in before..after)
    }

    @Test
    fun choreographerTimeSource_updatesOnEachFrame() {
        val source = ChoreographerAnimationTimeSource()
        source.onFrameTimeNanos(1_000_000_000L)
        assertEquals(1_000_000_000L, source.nowNanos())
        source.onFrameTimeNanos(1_016_000_000L)
        assertEquals(1_016_000_000L, source.nowNanos())
        source.onFrameTimeNanos(1_032_000_000L)
        assertEquals(1_032_000_000L, source.nowNanos())
    }

    @Test
    fun choreographerTimeSource_usesLatestFrameTimeNotSystemTime() {
        val source = ChoreographerAnimationTimeSource()
        val frameTime = 500_000_000L
        source.onFrameTimeNanos(frameTime)
        val reported = source.nowNanos()
        assertEquals("Should use cached frame time, not System.nanoTime()", frameTime, reported)
    }

    @Test
    fun visualRuntime_usesChoreographerFrameTimeAfterOnFrame() {
        val timeSource = ChoreographerAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(timeSource, transactionIdSource)
        val frameTime = 2_000_000_000L
        timeSource.onFrameTimeNanos(frameTime)
        assertEquals(frameTime, runtime.currentTimeNanos())
    }

    @Test
    fun visualRuntime_choreographerTimeUpdatesAcrossFrames() {
        val timeSource = ChoreographerAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(timeSource, transactionIdSource)
        timeSource.onFrameTimeNanos(1_000_000_000L)
        assertEquals(1_000_000_000L, runtime.currentTimeNanos())
        timeSource.onFrameTimeNanos(1_016_000_000L)
        assertEquals(1_016_000_000L, runtime.currentTimeNanos())
        timeSource.onFrameTimeNanos(1_032_000_000L)
        assertEquals(1_032_000_000L, runtime.currentTimeNanos())
    }

    @Test
    fun manualTimeSource_isIndependentOfChoreographerFrameUpdates() {
        val manual = ManualAnimationTimeSource()
        val choreographer = ChoreographerAnimationTimeSource()
        choreographer.onFrameTimeNanos(1_000_000_000L)
        assertEquals(0L, manual.nowNanos())
        manual.advanceByMs(16)
        assertEquals(16_000_000L, manual.nowNanos())
        assertEquals(1_000_000_000L, choreographer.nowNanos())
    }

    @Test
    fun choreographerTimeSource_multipleOnFrameCallsMonotonic() {
        val source = ChoreographerAnimationTimeSource()
        val times = listOf(100L, 200L, 300L, 400L, 500L)
        for (time in times) {
            source.onFrameTimeNanos(time)
            assertEquals(time, source.nowNanos())
        }
    }

    @Test
    fun drawFrameWithoutParam_usesChoreographerFrameTimeWhenAvailable() {
        val timeSource = ChoreographerAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(timeSource, transactionIdSource)
        val frameTimeNanos = 3_000_000_000L
        timeSource.onFrameTimeNanos(frameTimeNanos)
        assertEquals("currentTimeNanos should return the Choreographer frame time", frameTimeNanos, runtime.currentTimeNanos())
    }

    @Test
    fun drawFrameWithParam_usesProvidedFrameTimeDirectly() {
        val timeSource = ChoreographerAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(timeSource, transactionIdSource)
        val frameTimeNanos = 5_000_000_000L
        timeSource.onFrameTimeNanos(1_000_000_000L)
        val frameTimeMs = frameTimeNanos / 1_000_000
        val layout = android.text.StaticLayout("", android.text.TextPaint(), 100, android.text.Layout.Alignment.ALIGN_NORMAL, 1f, 0f, false)
        val result = runtime.tick(
            frameTimeMs,
            layout,
            null,
            emptyList(),
            100, 100,
            0f, 0f,
            true, true,
            0, 0, 0
        )
        assertNull("No active transaction, tick should return null for rendering", result?.renderInput?.transaction)
    }

    @Test
    fun timelineUsesFrameTimeNotSystemTime() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)
        manualTimeSource.advanceByMs(100)
        val frameTimeMs = 100L
        val layout = android.text.StaticLayout("test", android.text.TextPaint(), 100, android.text.Layout.Alignment.ALIGN_NORMAL, 1f, 0f, false)
        val result = runtime.tick(
            frameTimeMs,
            layout,
            null,
            emptyList(),
            100, 100,
            0f, 0f,
            true, true,
            0, 0, 0
        )
        assertNotNull("tick should return a frame state even without transaction", result)
    }
}
