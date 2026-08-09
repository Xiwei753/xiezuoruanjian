package com.xiwei.sujian.feature.editor.session

import android.view.Choreographer
import com.xiwei.sujian.feature.editor.window.WindowDisplayFrameClock
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 七：单一 VSync 帧驱动行为测试 — 用假 [WindowDisplayFrameClock.FrameCallbackPoster]
 * 驱动一帧并确认只更新一次。
 *
 * 静态结构约束（字段/方法存在性）已移入
 * [com.xiwei.sujian.arch.EditorFrameClockArchitectureTest]；本文件只保留运行时行为：
 * - requestFrame 后 poster 收到一次 postFrameCallback；
 * - doFrame 推进所有 listener 一次，needsFrame=true 时自动请求下一帧，false 时不请求；
 * - stop/release 后不再 post。
 */
class SingleFrameDriveTest {
    /** 假 FrameCallbackPoster — 记录 post/remove 调用次数，不依赖真实 Choreographer。 */
    private class FakeFrameCallbackPoster : WindowDisplayFrameClock.FrameCallbackPoster {
        var postedCount = 0
        var removedCount = 0
        var lastPostedCallback: Choreographer.FrameCallback? = null

        override fun postFrameCallback(callback: Choreographer.FrameCallback) {
            postedCount++
            lastPostedCallback = callback
        }

        override fun removeFrameCallback(callback: Choreographer.FrameCallback) {
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
        // 初始无 post。
        assertEquals(0, poster.postedCount)

        clock.requestFrame()
        assertEquals("requestFrame 必须 post 一次", 1, poster.postedCount)

        // 已 post 但未 doFrame 时，再次 requestFrame 不得重复 post（去抖）。
        clock.requestFrame()
        clock.requestFrame()
        assertEquals("callbackPosted 期间不得重复 post", 1, poster.postedCount)

        // doFrame 后 callbackPosted 复位，可再次 post。
        poster.lastPostedCallback!!.doFrame(1_000_000L)
        clock.requestFrame()
        assertEquals("doFrame 后可再次 post", 2, poster.postedCount)

        clock.stop()
    }

    @Test
    fun doFrame_advancesAllListenersExactlyOnce() {
        val poster = FakeFrameCallbackPoster()
        val clock = WindowDisplayFrameClock(poster)

        val listener = CountingListener(needsFrameProvider = { false })
        clock.addListener(listener)
        clock.requestFrame()
        assertEquals("listener 在 doFrame 前不得被调用", 0, listener.frameCount)

        // 单次 doFrame 推进 listener 恰好一次。
        poster.lastPostedCallback!!.doFrame(42_000_000L)
        assertEquals("doFrame 必须推进 listener 恰好一次", 1, listener.frameCount)
        assertEquals(42_000_000L, listener.lastFrameTimeNanos)

        // listener.needsFrame()=false → 不请求下一帧。
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

        // 任一 listener needsFrame=true → doFrame 自动请求下一帧。
        poster.lastPostedCallback!!.doFrame(100L)
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
        assertTrue("stop 必须 removeFrameCallback", poster.removedCount >= 1)

        // stop 后 requestFrame 可重新 post（callbackPosted 已复位）。
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
        assertTrue("release 必须 removeFrameCallback", poster.removedCount >= 1)

        // release 后 listener 已清除 — 即使 doFrame 也不会推进。
        // （lastPostedCallback 仍指向已 remove 的 callback，调用它不应推进已移除的 listener。）
        poster.lastPostedCallback?.doFrame(1L)
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
        poster.lastPostedCallback!!.doFrame(1L)
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
        poster.lastPostedCallback!!.doFrame(1L)
        assertEquals(1, listener.frameCount)

        clock.removeListener(listener)
        clock.requestFrame()
        poster.lastPostedCallback!!.doFrame(2L)
        assertEquals("removeListener 后不得再推进", 1, listener.frameCount)

        clock.release()
    }
}
