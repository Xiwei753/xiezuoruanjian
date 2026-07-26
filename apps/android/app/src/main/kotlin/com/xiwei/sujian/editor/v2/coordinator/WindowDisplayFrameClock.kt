package com.xiwei.sujian.editor.v2.coordinator

import android.view.Choreographer

class WindowDisplayFrameClock(
    private val poster: FrameCallbackPoster = ChoreographerPoster(Choreographer.getInstance())
) {
    interface FrameListener {
        fun needsFrame(): Boolean
        fun onFrame(frameTimeNanos: Long)
    }

    interface FrameCallbackPoster {
        fun postFrameCallback(callback: Choreographer.FrameCallback)
        fun removeFrameCallback(callback: Choreographer.FrameCallback)
    }

    class ChoreographerPoster(private val choreographer: Choreographer) : FrameCallbackPoster {
        override fun postFrameCallback(callback: Choreographer.FrameCallback) {
            choreographer.postFrameCallback(callback)
        }
        override fun removeFrameCallback(callback: Choreographer.FrameCallback) {
            choreographer.removeFrameCallback(callback)
        }
    }

    class ManualFrameClock : FrameCallbackPoster {
        private var pendingCallback: Choreographer.FrameCallback? = null

        override fun postFrameCallback(callback: Choreographer.FrameCallback) {
            pendingCallback = callback
        }

        override fun removeFrameCallback(callback: Choreographer.FrameCallback) {
            if (pendingCallback === callback) {
                pendingCallback = null
            }
        }

        fun dispatchFrame(frameTimeNanos: Long) {
            val cb = pendingCallback
            pendingCallback = null
            cb?.doFrame(frameTimeNanos)
        }

        fun hasPendingFrame(): Boolean = pendingCallback != null
    }

    private val listeners = mutableListOf<FrameListener>()
    private var callbackPosted: Boolean = false

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            callbackPosted = false
            val snapshot = listeners.toList()
            for (listener in snapshot) {
                listener.onFrame(frameTimeNanos)
            }
            if (snapshot.any { it.needsFrame() }) {
                requestFrame()
            }
        }
    }

    @Synchronized
    fun addListener(listener: FrameListener) {
        if (!listeners.contains(listener)) {
            listeners.add(listener)
        }
    }

    @Synchronized
    fun removeListener(listener: FrameListener) {
        listeners.remove(listener)
    }

    fun requestFrame() {
        if (callbackPosted) return
        callbackPosted = true
        poster.postFrameCallback(frameCallback)
    }

    fun dispatchFrame(frameTimeNanos: Long) {
        val poster = this.poster
        if (poster is ManualFrameClock) {
            poster.dispatchFrame(frameTimeNanos)
        }
    }

    fun isManualClock(): Boolean = poster is ManualFrameClock

    fun stop() {
        callbackPosted = false
        poster.removeFrameCallback(frameCallback)
    }

    fun release() {
        stop()
        synchronized(this) {
            listeners.clear()
        }
    }
}
