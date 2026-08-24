package com.xiwei.sujian.feature.editor.window

import android.os.Build
import android.view.Choreographer

/**
 * Window-level VSync 帧钟。
 *
 * 生产路径唯一时间源：通过 Choreographer 注册帧回调，每个真实 VSync 帧推进一次
 * 监听者（动画事务锚定、完成判定）。不允许存在测试专用手动时钟分支 — 动画必须
 * 由真实帧驱动。
 *
 * #637 评论 5386066978 项4 + 评论 5386301277 项2：帧脉冲 + 动画时间抽象。
 * - API 33+ 使用 [Choreographer.postVsyncCallback] + [Choreographer.FrameData]：
 *   动画时间取 `FrameData.getPreferredFrameTimeline().getExpectedPresentationTimeNanos()`，
 *   不取 `FrameData.getFrameTimeNanos()`。Android 官方明确写了 `frameTimeNanos`
 *   不应再用于动画，late frame 后会让动画直接向前跳产生 jank；preferred frame
 *   timeline 的 expected presentation time 才是 AOSP 自己用于推进 animation clocks
 *   的值。
 * - API 30–32 保留 [Choreographer.postFrameCallback] 兼容实现（项目 minSdk=30）。
 * - [requestFrame] 仍只允许挂一个 pending callback，不让每个 listener 自己注册。
 */
class WindowDisplayFrameClock(
    private val poster: FrameCallbackPoster = defaultPoster(),
) {
    interface FrameListener {
        fun needsFrame(): Boolean

        fun onFrame(frameTimeNanos: Long)
    }

    /**
     * #637 评论 5386066978 项4：帧脉冲回调 — 收到的是本帧用于渲染的 animation timestamp。
     */
    fun interface FramePulseCallback {
        fun onFramePulse(animationTimeNanos: Long)
    }

    /**
     * #637 评论 5386066978 项4：帧脉冲 + 动画时间抽象。
     * API 33+ 实现走 [Choreographer.postVsyncCallback]；API 30–32 走
     * [Choreographer.postFrameCallback] 兼容实现。
     * [requestFrame] 只挂一个 pending callback，不让每个 listener 自己注册。
     */
    interface FrameCallbackPoster {
        fun postFramePulse(callback: FramePulseCallback)

        fun removeFramePulse(callback: FramePulseCallback)
    }

    /**
     * API 30–32 兼容实现：用 [Choreographer.postFrameCallback]，
     * `doFrame(frameTimeNanos)` 直接作为动画时间。
     */
    class LegacyChoreographerPoster(private val choreographer: Choreographer) : FrameCallbackPoster {
        private val frameCallbacks = mutableMapOf<FramePulseCallback, Choreographer.FrameCallback>()

        override fun postFramePulse(callback: FramePulseCallback) {
            val fc =
                object : Choreographer.FrameCallback {
                    override fun doFrame(frameTimeNanos: Long) {
                        callback.onFramePulse(frameTimeNanos)
                    }
                }
            frameCallbacks[callback] = fc
            choreographer.postFrameCallback(fc)
        }

        override fun removeFramePulse(callback: FramePulseCallback) {
            val fc = frameCallbacks.remove(callback) ?: return
            choreographer.removeFrameCallback(fc)
        }
    }

    /**
     * API 33+ 实现：用 [Choreographer.postVsyncCallback] + [Choreographer.FrameData]。
     *
     * #637 评论 5386301277 项2：
     * - `postVsyncCallback(callback)` 返回 Unit，公开 API 没有 `VsyncCallbackToken`；
     *   `removeVsyncCallback()` 接收原来的 [Choreographer.VsyncCallback] 对象。因此
     *   必须保存 callback 本身，不能用 token。
     * - 动画时间取 `FrameData.getPreferredFrameTimeline().getExpectedPresentationTimeNanos()`，
     *   不取 `FrameData.getFrameTimeNanos()`。Android 官方明确写了 `frameTimeNanos`
     *   不应再用于动画，late frame 后会让动画直接向前跳产生 jank；AOSP 自己的
     *   `Choreographer.getExpectedPresentationTimeNanos()` 也是从 preferred frame
     *   timeline 取这个值，并注明它应当用于推进 animation clocks。
     */
    class VsyncChoreographerPoster(private val choreographer: Choreographer) : FrameCallbackPoster {
        private val callbacks =
            mutableMapOf<FramePulseCallback, Choreographer.VsyncCallback>()

        override fun postFramePulse(callback: FramePulseCallback) {
            val vsyncCallback =
                Choreographer.VsyncCallback { frameData ->
                    callbacks.remove(callback)
                    callback.onFramePulse(
                        frameData.preferredFrameTimeline.expectedPresentationTimeNanos,
                    )
                }
            callbacks[callback] = vsyncCallback
            choreographer.postVsyncCallback(vsyncCallback)
        }

        override fun removeFramePulse(callback: FramePulseCallback) {
            val vsyncCallback = callbacks.remove(callback) ?: return
            choreographer.removeVsyncCallback(vsyncCallback)
        }
    }

    private val listeners = mutableListOf<FrameListener>()
    private var callbackPosted: Boolean = false

    private val framePulse =
        FramePulseCallback { animationTimeNanos ->
            callbackPosted = false
            val snapshot = listeners.toList()
            for (listener in snapshot) {
                listener.onFrame(animationTimeNanos)
            }
            if (snapshot.any { it.needsFrame() }) {
                requestFrame()
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
        poster.postFramePulse(framePulse)
    }

    fun stop() {
        callbackPosted = false
        poster.removeFramePulse(framePulse)
    }

    fun release() {
        stop()
        synchronized(this) {
            listeners.clear()
        }
    }

    private companion object {
        fun defaultPoster(): FrameCallbackPoster =
            if (Build.VERSION.SDK_INT >= 33) {
                VsyncChoreographerPoster(Choreographer.getInstance())
            } else {
                LegacyChoreographerPoster(Choreographer.getInstance())
            }
    }
}
