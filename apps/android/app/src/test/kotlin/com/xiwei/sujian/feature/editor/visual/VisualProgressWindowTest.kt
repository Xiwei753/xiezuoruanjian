package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #637 评论 5386066978 项2：VisualProgressWindow 契约测试。
 *
 * - Full 窗口 map(progress) = progress（新事务首次播放，行为不变）。
 * - continuation 窗口 map(0) = 0、map(end) = 1，已走部分不重新计时。
 * - 边界稳定：globalProgress <= start → 0f，globalProgress >= end → 1f。
 */
class VisualProgressWindowTest {
    @Test
    fun full_mapIsIdentity() {
        val w = VisualProgressWindow.Full
        assertEquals(0f, w.map(0f), 0f)
        assertEquals(0.5f, w.map(0.5f), 0f)
        assertEquals(1f, w.map(1f), 0f)
    }

    @Test
    fun continuedWindow_mapsZeroToZeroAndEndToOne() {
        val w = VisualProgressWindow.Full.continued(0.6f)
        assertEquals(0f, w.start, 0f)
        assertEquals(0.4f, w.end, 0.001f)
        assertEquals("map(0) = 0", 0f, w.map(0f), 0f)
        assertEquals("map(end) = 1", 1f, w.map(0.4f), 0.001f)
        assertEquals("map(0.2) = 0.5（剩余 40% 内走一半）", 0.5f, w.map(0.2f), 0.001f)
    }

    @Test
    fun continuedWindow_consumedFractionZeroOrOneReturnsFull() {
        assertEquals(VisualProgressWindow.Full, VisualProgressWindow.Full.continued(0f))
        assertEquals(VisualProgressWindow.Full, VisualProgressWindow.Full.continued(1f))
        assertEquals(VisualProgressWindow.Full, VisualProgressWindow.Full.continued(-0.1f))
        assertEquals(VisualProgressWindow.Full, VisualProgressWindow.Full.continued(1.5f))
    }

    @Test
    fun map_clampsBelowStartToZero() {
        val w = VisualProgressWindow(start = 0.2f, end = 0.8f)
        assertEquals(0f, w.map(0f), 0f)
        assertEquals(0f, w.map(0.2f), 0f)
    }

    @Test
    fun map_clampsAboveEndToOne() {
        val w = VisualProgressWindow(start = 0.2f, end = 0.8f)
        assertEquals(1f, w.map(0.8f), 0f)
        assertEquals(1f, w.map(1f), 0f)
    }

    @Test
    fun map_degenerateSpanDoesNotDivideByZero() {
        val w = VisualProgressWindow(start = 0.5f, end = 0.5f)
        assertEquals(0f, w.map(0.4f), 0f)
        assertEquals(1f, w.map(0.6f), 0f)
    }

    /**
     * #637 评论 5386066978 项2：rebase 后保持上一帧的运动速度。
     *
     * 旧帧 100ms 走了 60%（60ms）。新事务 rebase continuation 窗口 end = 0.4。
     * 新事务 globalProgress = 0.4 时（40ms 后），localProgress = 1f（完成）。
     * 即原来已走 60% 的字符只用新事务剩余 40ms 完成，不会又慢吞吞跑 100ms。
     */
    @Test
    fun rebaseContinuation_preservesVelocityNotRestartingFullDuration() {
        val consumedFraction = 0.6f
        val window = VisualProgressWindow.Full.continued(consumedFraction)
        val localProgressAt40ms = window.map(0.4f)
        assertEquals(1f, localProgressAt40ms, 0.001f)
        val localProgressAt20ms = window.map(0.2f)
        assertEquals(0.5f, localProgressAt20ms, 0.001f)
    }
}
