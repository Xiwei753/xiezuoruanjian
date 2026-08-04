package com.xiwei.sujian.editor.v2.coordinator

import android.view.Choreographer

/**
 * Window-level VSync 帧钟。
 *
 * 生产路径唯一时间源：通过 [Choreographer.postFrameCallback] 注册帧回调，
 * 每个真实 VSync 帧推进一次监听者（动画事务锚定、完成判定）。
 * 不允许存在测试专用手动时钟分支 — 动画必须由真实帧驱动。
 */
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
