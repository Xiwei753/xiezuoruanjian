package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #623 评论 1：AnimationTimeSource 语义分离契约测试。
 *
 * nowNanos() 只表示"此刻的 monotonic time"（System.nanoTime()），不返回缓存帧；
 * lastFrameTimeNanos() 返回最近一次真实 VSync 帧时间。
 */
class AnimationTimeSourceTest {
    @Test
    fun nowNanos_returnsCurrentMonotonicTimeNotCachedFrame() {
        val source = ChoreographerAnimationTimeSource()
        val frameNanos = 1_000_000_000L
        source.onFrameTimeNanos(frameNanos)

        val systemBefore = System.nanoTime()
        val nowNanos = source.nowNanos()
        val systemAfter = System.nanoTime()

        assertTrue(
            "nowNanos() should return System.nanoTime(), not cached frame $frameNanos; got $nowNanos",
            nowNanos >= systemBefore && nowNanos <= systemAfter,
        )
        assertTrue(
            "nowNanos() must not return cached frame time after onFrameTimeNanos",
            nowNanos != frameNanos,
        )
    }

    @Test
    fun nowNanos_isMonotonicAcrossMultipleCalls() {
        val source = ChoreographerAnimationTimeSource()
        source.onFrameTimeNanos(500_000_000L)

        val first = source.nowNanos()
        val second = source.nowNanos()
        assertTrue(
            "nowNanos() should be monotonic: first=$first, second=$second",
            second >= first,
        )
    }

    @Test
    fun lastFrameTimeNanos_returnsNullBeforeFirstFrame() {
        val source = ChoreographerAnimationTimeSource()
        assertNull(
            "lastFrameTimeNanos() should be null before any frame is received",
            source.lastFrameTimeNanos(),
        )
    }

    @Test
    fun lastFrameTimeNanos_returnsCachedFrameAfterOnFrame() {
        val source = ChoreographerAnimationTimeSource()
        val frameNanos = 2_000_000_000L
        source.onFrameTimeNanos(frameNanos)
        assertEquals(
            "lastFrameTimeNanos() should return the frame time set by onFrameTimeNanos",
            frameNanos,
            source.lastFrameTimeNanos(),
        )
    }

    @Test
    fun lastFrameTimeNanos_updatesToLatestFrame() {
        val source = ChoreographerAnimationTimeSource()
        source.onFrameTimeNanos(1_000_000L)
        source.onFrameTimeNanos(2_000_000L)
        source.onFrameTimeNanos(3_000_000L)
        assertEquals(
            "lastFrameTimeNanos() should return the most recent frame time",
            3_000_000L,
            source.lastFrameTimeNanos(),
        )
    }

    @Test
    fun nowNanos_andLastFrameTimeNanos_areIndependent() {
        val source = ChoreographerAnimationTimeSource()
        val frameNanos = 10_000_000_000L
        source.onFrameTimeNanos(frameNanos)

        val nowNanos = source.nowNanos()
        val lastFrame = source.lastFrameTimeNanos()

        assertNotNull(lastFrame)
        assertTrue(
            "nowNanos() (current time) and lastFrameTimeNanos() (cached frame) must be independent",
            nowNanos != lastFrame,
        )
    }
}
