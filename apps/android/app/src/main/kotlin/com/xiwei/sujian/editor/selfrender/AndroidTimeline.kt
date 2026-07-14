package com.xiwei.sujian.editor.selfrender

/**
 * 统一事务时钟 — 所有视觉事务（文字切片、光标、预输入装饰）共用同一个 Timeline。
 *
 * 设计原则（来自 issue #515）：
 * - Choreographer / onDraw 只负责请求帧，不得给光标维护独立开始时间。
 * - Paused 状态必须返回暂停瞬间的 progress，不能返回 0。
 * - resume 后从暂停进度继续。
 *
 * @param durationMs 动画总时长（毫秒）
 */
class AndroidTimeline(
    val durationMs: Long
) {
    var firstVisibleFrameTimeMs: Long = 0L
        private set

    var pauseStartedAtMs: Long = 0L
        private set

    var accumulatedPausedDurationMs: Long = 0L
        private set

    var pausedProgress: Float = 0f
        private set

    val isStarted: Boolean get() = firstVisibleFrameTimeMs > 0L

    var isPaused: Boolean = false
        private set

    fun recordFirstFrame(nowMs: Long) {
        if (firstVisibleFrameTimeMs <= 0L) {
            firstVisibleFrameTimeMs = nowMs
        }
    }

    fun pause(nowMs: Long) {
        if (isPaused) return
        isPaused = true
        pauseStartedAtMs = nowMs
        pausedProgress = computeRawProgress(nowMs)
    }

    fun resume(nowMs: Long) {
        if (!isPaused) return
        isPaused = false
        if (pauseStartedAtMs > 0L) {
            accumulatedPausedDurationMs += nowMs - pauseStartedAtMs
            pauseStartedAtMs = 0L
        }
    }

    fun progress(nowMs: Long): Float {
        if (durationMs <= 0L) return 1f
        if (!isStarted) return 0f
        if (isPaused) return pausedProgress.coerceIn(0f, 1f)
        return computeRawProgress(nowMs).coerceIn(0f, 1f)
    }

    private fun computeRawProgress(nowMs: Long): Float {
        val effectiveStart = firstVisibleFrameTimeMs
        if (effectiveStart <= 0L) return 0f
        val elapsed = (nowMs - effectiveStart - accumulatedPausedDurationMs).coerceAtLeast(0L)
        return elapsed.toFloat() / durationMs.toFloat()
    }
}
