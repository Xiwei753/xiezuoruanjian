package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.WindowDisplayFrameClock
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 七：单一 VSync 帧驱动行为测试 — 用假 [WindowDisplayFrameClock.FrameCallbackPoster]
 * 驱动一帧并确认只更新一次。
 *
 * #637 评论 5386066978 项4：FrameCallbackPoster 已改为帧脉冲 + 动画时间抽象
 * （postFramePulse/removeFramePulse + FramePulseCallback）。本测试用假 poster
 * 驱动，不依赖真实 Choreographer。
 *
 * 静态结构约束（字段/方法存在性）已移入
 * [com.xiwei.sujian.arch.EditorFrameClockArchitectureTest]；本文件只保留运行时行为：
 * - requestFrame 后 poster 收到一次 postFramePulse；
 * - onFramePulse 推进所有 listener 一次，needsFrame=true 时自动请求下一帧，false 时不请求；
 * - stop/release 后不再 post。
 */
class SingleFrameDriveTest {
    /** 假 FrameCallbackPoster — 记录 post/remove 调用次数，不依赖真实 Choreographer。 */
    private class FakeFrameCallbackPoster : WindowDisplayFrameClock.FrameCallbackPoster {
        var postedCount = 0
        var removedCount = 0
        var lastPostedCallback: WindowDisplayFrameClock.FramePulseCallback? = null

        override fun postFramePulse(callback: WindowDisplayFrameClock.FramePulseCallback) {
            postedCount++
            lastPostedCallback = callback
        }

        override fun removeFramePulse(callback: WindowDisplayFrameClock.FramePulseCallback) {
            removedCount++
        }
    }

    /** 记录 onFrame 调用次数与帧时间。 */
    private class CountingListener(
        private val needsFrameProvider: () -> Boolean,
    ) : WindowDisplayFrameClock.FrameListener {
        var frameCount = 0
        var lastFrameTimeNanos = -1L

        override fun needsFrame(): Boolean = needsFrameProvider()

        override fun onFrame(frameTimeNanos: Long) {
            frameCount++
            lastFrameTimeNanos = frameTimeNanos
        }
    }

    @Test
    fun requestFrame_postsAtMostOnceUntilFrameFires() {
        val poster = FakeFrameCallbackPoster()
        val clock = WindowDisplayFrameClock(poster)
        assertEquals(0, poster.postedCount)

        clock.requestFrame()
        assertEquals("requestFrame 必须 post 一次", 1, poster.postedCount)

        clock.requestFrame()
        clock.requestFrame()
        assertEquals("callbackPosted 期间不得重复 post", 1, poster.postedCount)

        poster.lastPostedCallback!!.onFramePulse(1_000_000L)
        clock.requestFrame()
        assertEquals("onFramePulse 后可再次 post", 2, poster.postedCount)

        clock.stop()
    }

    @Test
    fun doFrame_advancesAllListenersExactlyOnce() {
        val poster = FakeFrameCallbackPoster()
        val clock = WindowDisplayFrameClock(poster)

        val listener = CountingListener(needsFrameProvider = { false })
        clock.addListener(listener)
        clock.requestFrame()
        assertEquals("listener 在 onFramePulse 前不得被调用", 0, listener.frameCount)

        poster.lastPostedCallback!!.onFramePulse(42_000_000L)
        assertEquals("onFramePulse 必须推进 listener 恰好一次", 1, listener.frameCount)
        assertEquals(42_000_000L, listener.lastFrameTimeNanos)

        assertEquals("needsFrame=false 时不得自动请求下一帧", 1, poster.postedCount)

        clock.release()
    }

    @Test
    fun doFrame_requestsNextFrameWhenAnyListenerNeedsFrame() {
        val poster = FakeFrameCallbackPoster()
        val clock = WindowDisplayFrameClock(poster)

        val needsMore = CountingListener(needsFrameProvider = { true })
        val done = CountingListener(needsFrameProvider = { false })
        clock.addListener(needsMore)
        clock.addListener(done)
        clock.requestFrame()
        assertEquals(1, poster.postedCount)

        poster.lastPostedCallback!!.onFramePulse(100L)
        assertEquals("needsFrame=true 时必须自动请求下一帧", 2, poster.postedCount)
        assertEquals(1, needsMore.frameCount)
        assertEquals(1, done.frameCount)

        clock.release()
    }

    @Test
    fun stop_removesCallbackAndPreventsFurtherPosts() {
        val poster = FakeFrameCallbackPoster()
        val clock = WindowDisplayFrameClock(poster)
        clock.requestFrame()
        assertEquals(1, poster.postedCount)

        clock.stop()
        assertTrue("stop 必须 removeFramePulse", poster.removedCount >= 1)

        val before = poster.postedCount
        clock.requestFrame()
        assertEquals("stop 后可重新 requestFrame", before + 1, poster.postedCount)
    }

    @Test
    fun release_clearsListenersAndStopsCallback() {
        val poster = FakeFrameCallbackPoster()
        val clock = WindowDisplayFrameClock(poster)
        val listener = CountingListener(needsFrameProvider = { false })
        clock.addListener(listener)
        clock.requestFrame()

        clock.release()
        assertTrue("release 必须 removeFramePulse", poster.removedCount >= 1)

        poster.lastPostedCallback?.onFramePulse(1L)
        assertEquals("release 后 listener 不得被推进", 0, listener.frameCount)
    }

    @Test
    fun addListener_isIdempotent() {
        val poster = FakeFrameCallbackPoster()
        val clock = WindowDisplayFrameClock(poster)
        val listener = CountingListener(needsFrameProvider = { false })

        clock.addListener(listener)
        clock.addListener(listener)
        clock.addListener(listener)
        clock.requestFrame()
        poster.lastPostedCallback!!.onFramePulse(1L)
        assertEquals("重复 addListener 不得重复推进", 1, listener.frameCount)

        clock.release()
    }

    @Test
    fun removeListener_stopsDeliveringFrames() {
        val poster = FakeFrameCallbackPoster()
        val clock = WindowDisplayFrameClock(poster)
        val listener = CountingListener(needsFrameProvider = { false })
        clock.addListener(listener)
        clock.requestFrame()
        poster.lastPostedCallback!!.onFramePulse(1L)
        assertEquals(1, listener.frameCount)

        clock.removeListener(listener)
        clock.requestFrame()
        poster.lastPostedCallback!!.onFramePulse(2L)
        assertEquals("removeListener 后不得再推进", 1, listener.frameCount)

        clock.release()
    }
}
