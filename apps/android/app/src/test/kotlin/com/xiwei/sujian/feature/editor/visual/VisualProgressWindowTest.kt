package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #637 评论 5386066978 项2 / 评论 5386573878：VisualProgressWindow 契约测试。
 *
 * - Full 窗口 map(progress) = progress（新事务首次播放，行为不变）。
 * - remainingFractionAt + fromRemainingFraction：连续 rebase 保持匀速，不反复减速。
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
    fun fromRemainingFraction_buildsContinuationWindow() {
        // 旧动画走 60% → 剩 0.4 → 新窗口 [0, 0.4]
        val w = VisualProgressWindow.fromRemainingFraction(0.4f)
        assertEquals(0f, w.start, 0f)
        assertEquals(0.4f, w.end, 0.001f)
        assertEquals("map(0) = 0", 0f, w.map(0f), 0f)
        assertEquals("map(end) = 1", 1f, w.map(0.4f), 0.001f)
        assertEquals("map(0.2) = 0.5（剩余 40% 内走一半）", 0.5f, w.map(0.2f), 0.001f)
    }

    @Test
    fun fromRemainingFraction_zeroOrNegativeReturnsFull() {
        assertEquals(VisualProgressWindow.Full, VisualProgressWindow.fromRemainingFraction(0f))
        assertEquals(VisualProgressWindow.Full, VisualProgressWindow.fromRemainingFraction(-0.1f))
    }

    @Test
    fun fromRemainingFraction_clampsAboveOne() {
        val w = VisualProgressWindow.fromRemainingFraction(1.5f)
        assertEquals(1f, w.end, 0f)
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
     * 旧帧 100ms 走了 60%（60ms）。新事务 continuation 窗口 end = 0.4。
     * 新事务 globalProgress = 0.4 时（40ms 后），localProgress = 1f（完成）。
     * 即原来已走 60% 的字符只用新事务剩余 40ms 完成，不会又慢吞吞跑 100ms。
     */
    @Test
    fun rebaseContinuation_preservesVelocityNotRestartingFullDuration() {
        val remaining = VisualProgressWindow.Full.remainingFractionAt(0.6f)
        val window = VisualProgressWindow.fromRemainingFraction(remaining)
        val localProgressAt40ms = window.map(0.4f)
        assertEquals(1f, localProgressAt40ms, 0.001f)
        val localProgressAt20ms = window.map(0.2f)
        assertEquals(0.5f, localProgressAt20ms, 0.001f)
    }

    /**
     * #637 评论 5386573878 核心问题：双重 rebase 不能把剩余时间重新放大。
     *
     * 第一次 rebase：旧 Full 走到 0.6 → remaining = 0.4 → 新窗口 [0, 0.4]。
     * 第二次 rebase：新窗口 [0, 0.4] 走到 0.2 → remaining = 0.4 - 0.2 = 0.2
     *   （不是旧 localProgress 方案的 0.5）→ 第三事务窗口 [0, 0.2]。
     * 第三次 rebase：[0, 0.2] 走到 0.1 → remaining = 0.1 → 第四事务窗口 [0, 0.1]。
     *
     * 旧 localProgress 方案在第二次会得 localProgress = 0.2/0.4 = 0.5，
     * 下一次 end = 0.5，剩余从 20ms 被放大回 50ms — 连续 rebase 反复减速。
     */
    @Test
    fun doubleRebase_doesNotReinflateRemainingDuration() {
        // 第一次 rebase：Full 走到 0.6
        val remaining1 = VisualProgressWindow.Full.remainingFractionAt(0.6f)
        assertEquals(0.4f, remaining1, 0.001f)
        val window1 = VisualProgressWindow.fromRemainingFraction(remaining1)
        assertEquals(0.4f, window1.end, 0.001f)

        // 第二次 rebase：window1 [0, 0.4] 走到 0.2
        val remaining2 = window1.remainingFractionAt(0.2f)
        assertEquals("第二次 rebase 必须剩 0.2，不能变 0.5", 0.2f, remaining2, 0.001f)
        val window2 = VisualProgressWindow.fromRemainingFraction(remaining2)
        assertEquals(0.2f, window2.end, 0.001f)

        // 第三次 rebase：window2 [0, 0.2] 走到 0.1
        val remaining3 = window2.remainingFractionAt(0.1f)
        assertEquals(0.1f, remaining3, 0.001f)
        val window3 = VisualProgressWindow.fromRemainingFraction(remaining3)
        assertEquals(0.1f, window3.end, 0.001f)
    }

    /**
     * #637 评论 5386573878：三重 rebase 后运动速度与原始一致。
     *
     * 原始 100ms。第一次 rebase 后剩 40ms，第二次后剩 20ms，第三次后剩 10ms。
     * 每次新事务在剩余窗口内匀速完成，不会因 localProgress 被放大。
     */
    @Test
    fun tripleRebase_remainingMonotonicallyShrinks() {
        var window = VisualProgressWindow.Full
        // 走 60% 后 rebase
        window = VisualProgressWindow.fromRemainingFraction(window.remainingFractionAt(0.6f))
        assertEquals(0.4f, window.end, 0.001f)
        // 新事务走 50%（global 0.2）后 rebase
        window = VisualProgressWindow.fromRemainingFraction(window.remainingFractionAt(0.2f))
        assertEquals(0.2f, window.end, 0.001f)
        // 再走 50%（global 0.1）后 rebase
        window = VisualProgressWindow.fromRemainingFraction(window.remainingFractionAt(0.1f))
        assertEquals(0.1f, window.end, 0.001f)
    }

    @Test
    fun remainingFractionAt_belowStartReturnsFullSpan() {
        val w = VisualProgressWindow(start = 0.2f, end = 0.8f)
        assertEquals(0.6f, w.remainingFractionAt(0f), 0.001f)
        assertEquals(0.6f, w.remainingFractionAt(0.2f), 0.001f)
    }

    @Test
    fun remainingFractionAt_aboveEndReturnsZero() {
        val w = VisualProgressWindow(start = 0.2f, end = 0.8f)
        assertEquals(0f, w.remainingFractionAt(0.8f), 0f)
        assertEquals(0f, w.remainingFractionAt(1f), 0f)
    }
}
