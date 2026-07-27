package com.xiwei.sujian.editor.v2.projection

import android.text.TextPaint
import com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class TargetDisplayRuntimeTimeSourceTest {

    @Test
    fun targetDisplayRuntimeAcceptsManualTimeSource() {
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello", 5)
        val paint = TextPaint()
        paint.textSize = 40f
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = TargetDisplayRuntime(mirror, paint, manualTimeSource, transactionIdSource)
        assertNotNull(runtime)
    }

    @Test
    fun targetDisplayRuntimeDefaultConstructorStillWorks() {
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello", 5)
        val paint = TextPaint()
        paint.textSize = 40f
        val runtime = TargetDisplayRuntime(mirror, paint)
        assertNotNull(runtime)
    }

    @Test
    fun manualTimeSourceControlsDrawFrameTime() {
        val manualTimeSource = ManualAnimationTimeSource()
        manualTimeSource.advanceByMs(32)
        assertEquals(32_000_000L, manualTimeSource.nowNanos())
    }

    @Test
    fun choreographerTimeSourcePropagatesFrameTimeThroughOnFrame() {
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello", 5)
        val paint = TextPaint()
        paint.textSize = 40f
        val timeSource = ChoreographerAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = TargetDisplayRuntime(mirror, paint, timeSource, transactionIdSource)
        val manualClock = WindowDisplayFrameClock.ManualFrameClock()
        val frameClock = WindowDisplayFrameClock(manualClock)
        runtime.setFrameClock(frameClock)
        val frameTimeNanos = 1_000_000_000L
        manualClock.dispatchFrame(frameTimeNanos)
        assertEquals("ChoreographerAnimationTimeSource should have received frame time from onFrame", frameTimeNanos, timeSource.nowNanos())
    }

    @Test
    fun choreographerTimeSourceUpdatesAcrossMultipleFrames() {
        val timeSource = ChoreographerAnimationTimeSource()
        timeSource.onFrameTimeNanos(1_000_000_000L)
        assertEquals(1_000_000_000L, timeSource.nowNanos())
        timeSource.onFrameTimeNanos(1_016_000_000L)
        assertEquals(1_016_000_000L, timeSource.nowNanos())
        timeSource.onFrameTimeNanos(1_032_000_000L)
        assertEquals(1_032_000_000L, timeSource.nowNanos())
    }

    @Test
    fun targetDisplayRuntimeOnFrameUpdatesChoreographerTimeSource() {
        val mirror = DisplayTextMirror()
        mirror.loadText("Test", 4)
        val paint = TextPaint()
        paint.textSize = 40f
        val timeSource = ChoreographerAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = TargetDisplayRuntime(mirror, paint, timeSource, transactionIdSource)
        runtime.setViewportSize(200, 200)
        val manualClock = WindowDisplayFrameClock.ManualFrameClock()
        val frameClock = WindowDisplayFrameClock(manualClock)
        runtime.setFrameClock(frameClock)
        val frame1 = 1_000_000_000L
        manualClock.dispatchFrame(frame1)
        assertEquals(frame1, timeSource.nowNanos())
        val frame2 = 1_016_000_000L
        manualClock.dispatchFrame(frame2)
        assertEquals(frame2, timeSource.nowNanos())
    }
}
