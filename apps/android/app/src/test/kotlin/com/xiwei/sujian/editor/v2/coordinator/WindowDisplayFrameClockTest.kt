package com.xiwei.sujian.editor.v2.coordinator

import android.view.Choreographer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test

class WindowDisplayFrameClockTest {

    private val postedCallbacks = mutableListOf<Choreographer.FrameCallback>()
    private val removedCallbacks = mutableListOf<Choreographer.FrameCallback>()
    private lateinit var clock: WindowDisplayFrameClock

    private val stubPoster = object : WindowDisplayFrameClock.FrameCallbackPoster {
        override fun postFrameCallback(callback: Choreographer.FrameCallback) {
            postedCallbacks.add(callback)
        }
        override fun removeFrameCallback(callback: Choreographer.FrameCallback) {
            removedCallbacks.add(callback)
        }
    }

    @Before
    fun setUp() {
        postedCallbacks.clear()
        removedCallbacks.clear()
        clock = WindowDisplayFrameClock(stubPoster)
    }

    @After
    fun tearDown() {
        clock.release()
    }

    private fun firePostedCallbacks(frameTimeNanos: Long = System.nanoTime()) {
        val callbacks = postedCallbacks.toList()
        postedCallbacks.clear()
        for (cb in callbacks) {
            cb.doFrame(frameTimeNanos)
        }
    }

    @Test
    fun requestFramePostsExactlyOnceWhenCalledMultipleTimes() {
        var frameCount = 0
        val listener = object : WindowDisplayFrameClock.FrameListener {
            override fun needsFrame(): Boolean = false
            override fun onFrame(frameTimeNanos: Long) { frameCount++ }
        }
        clock.addListener(listener)
        clock.requestFrame()
        clock.requestFrame()
        clock.requestFrame()
        assertEquals("only one callback should be posted", 1, postedCallbacks.size)
        firePostedCallbacks()
        assertEquals("callback should fire exactly once", 1, frameCount)
    }

    @Test
    fun doFrameResetsCallbackPostedAndRePostsWhenListenerNeedsFrame() {
        var frameCount = 0
        var needsFrameCount = 0
        val listener = object : WindowDisplayFrameClock.FrameListener {
            override fun needsFrame(): Boolean {
                needsFrameCount++
                return needsFrameCount <= 2
            }
            override fun onFrame(frameTimeNanos: Long) { frameCount++ }
        }
        clock.addListener(listener)
        clock.requestFrame()
        assertEquals(1, postedCallbacks.size)
        firePostedCallbacks()
        assertEquals("first frame received", 1, frameCount)
        assertEquals("auto-reposted because needsFrame=true", 1, postedCallbacks.size)
        firePostedCallbacks()
        assertEquals("second frame received", 2, frameCount)
    }

    @Test
    fun doFrameDoesNotRePostWhenNoListenerNeedsFrame() {
        var frameCount = 0
        val listener = object : WindowDisplayFrameClock.FrameListener {
            override fun needsFrame(): Boolean = false
            override fun onFrame(frameTimeNanos: Long) { frameCount++ }
        }
        clock.addListener(listener)
        clock.requestFrame()
        firePostedCallbacks()
        assertEquals(1, frameCount)
        assertEquals("no more callbacks posted", 0, postedCallbacks.size)
    }

    @Test
    fun stopPreventsPendingCallbackFromFiring() {
        var frameCount = 0
        val listener = object : WindowDisplayFrameClock.FrameListener {
            override fun needsFrame(): Boolean = true
            override fun onFrame(frameTimeNanos: Long) { frameCount++ }
        }
        clock.addListener(listener)
        clock.requestFrame()
        assertEquals(1, postedCallbacks.size)
        clock.stop()
        assertEquals("callback should have been removed", 1, removedCallbacks.size)
        postedCallbacks.clear()
        assertEquals("no frame should fire after stop()", 0, frameCount)
    }

    @Test
    fun requestFrameAfterStopPostsAgain() {
        var frameCount = 0
        val listener = object : WindowDisplayFrameClock.FrameListener {
            override fun needsFrame(): Boolean = false
            override fun onFrame(frameTimeNanos: Long) { frameCount++ }
        }
        clock.addListener(listener)
        clock.requestFrame()
        clock.stop()
        postedCallbacks.clear()
        removedCallbacks.clear()
        clock.requestFrame()
        assertEquals("should post again after stop", 1, postedCallbacks.size)
        firePostedCallbacks()
        assertEquals("should fire after re-requesting post stop", 1, frameCount)
    }

    @Test
    fun multipleListenersEachReceiveOnFrame() {
        var countA = 0
        var countB = 0
        val listenerA = object : WindowDisplayFrameClock.FrameListener {
            override fun needsFrame(): Boolean = false
            override fun onFrame(frameTimeNanos: Long) { countA++ }
        }
        val listenerB = object : WindowDisplayFrameClock.FrameListener {
            override fun needsFrame(): Boolean = false
            override fun onFrame(frameTimeNanos: Long) { countB++ }
        }
        clock.addListener(listenerA)
        clock.addListener(listenerB)
        clock.requestFrame()
        firePostedCallbacks()
        assertEquals(1, countA)
        assertEquals(1, countB)
    }

    @Test
    fun removeListenerStopsReceivingFrames() {
        var frameCount = 0
        val listener = object : WindowDisplayFrameClock.FrameListener {
            override fun needsFrame(): Boolean = false
            override fun onFrame(frameTimeNanos: Long) { frameCount++ }
        }
        clock.addListener(listener)
        clock.requestFrame()
        firePostedCallbacks()
        assertEquals(1, frameCount)
        clock.removeListener(listener)
        clock.requestFrame()
        firePostedCallbacks()
        assertEquals("no more frames after removal", 1, frameCount)
    }

    @Test
    fun releaseClearsListenersAndStopsClock() {
        var frameCount = 0
        val listener = object : WindowDisplayFrameClock.FrameListener {
            override fun needsFrame(): Boolean = true
            override fun onFrame(frameTimeNanos: Long) { frameCount++ }
        }
        clock.addListener(listener)
        clock.requestFrame()
        clock.release()
        postedCallbacks.clear()
        assertEquals("no frames after release", 0, frameCount)
    }

    @Test
    fun callbackPostedFlagPreventsDuplicatePostInSameFrame() {
        val listener = object : WindowDisplayFrameClock.FrameListener {
            override fun needsFrame(): Boolean = false
            override fun onFrame(frameTimeNanos: Long) {}
        }
        clock.addListener(listener)
        clock.requestFrame()
        assertEquals(1, postedCallbacks.size)
        clock.requestFrame()
        clock.requestFrame()
        assertEquals("still only one callback despite additional requestFrame calls", 1, postedCallbacks.size)
    }
}
