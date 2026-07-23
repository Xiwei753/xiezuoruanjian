package com.xiwei.sujian.editor.v2.coordinator

import android.view.Choreographer

class WindowDisplayFrameClock {
    interface FrameListener {
        fun needsFrame(): Boolean
        fun onFrame(frameTimeNanos: Long)
    }

    private val choreographer = Choreographer.getInstance()
    private val listeners = mutableListOf<FrameListener>()
    private var isTicking: Boolean = false

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!isTicking) return
            val snapshot = listeners.toList()
            for (listener in snapshot) {
                listener.onFrame(frameTimeNanos)
            }
            if (snapshot.any { it.needsFrame() }) {
                choreographer.postFrameCallback(this)
            } else {
                isTicking = false
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
        if (!isTicking) {
            isTicking = true
        }
        choreographer.postFrameCallback(frameCallback)
    }

    fun stop() {
        isTicking = false
        choreographer.removeFrameCallback(frameCallback)
    }

    fun release() {
        stop()
        synchronized(this) {
            listeners.clear()
        }
    }
}
