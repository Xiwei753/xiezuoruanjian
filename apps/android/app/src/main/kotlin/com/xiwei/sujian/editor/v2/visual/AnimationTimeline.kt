package com.xiwei.sujian.editor.v2.visual

class AnimationTimeline(
    private val durationMs: Long
) {
    private var firstVisibleFrameTimeMs: Long? = null
    private var pauseStartedAtMs: Long? = null
    private var accumulatedPausedDurationMs: Long = 0
    private var pausedProgress: Float = 0f
    private var state: TransactionState = TransactionState.Pending

    fun progress(frameTimeMs: Long): Float {
        val start = firstVisibleFrameTimeMs ?: return 0f
        if (state == TransactionState.Paused) return pausedProgress
        if (durationMs == 0L) return 1f

        val effectiveElapsed = frameTimeMs - start - accumulatedPausedDurationMs
        val p = effectiveElapsed.toFloat() / durationMs.toFloat()
        return p.coerceIn(0f, 1f)
    }

    fun markFirstVisibleFrame(frameTimeMs: Long) {
        if (firstVisibleFrameTimeMs == null) {
            firstVisibleFrameTimeMs = frameTimeMs
            state = TransactionState.Rendering
        }
    }

    fun pause(frameTimeMs: Long) {
        if (state == TransactionState.Paused) return
        pausedProgress = progress(frameTimeMs)
        pauseStartedAtMs = frameTimeMs
        state = TransactionState.Paused
    }

    fun resume(frameTimeMs: Long) {
        if (state != TransactionState.Paused) return
        if (firstVisibleFrameTimeMs == null) {
            pauseStartedAtMs = null
            pausedProgress = 0f
            state = TransactionState.Pending
            return
        }
        val newStart = frameTimeMs - (pausedProgress * durationMs).toLong()
        firstVisibleFrameTimeMs = newStart
        accumulatedPausedDurationMs = 0
        pauseStartedAtMs = null
        pausedProgress = 0f
        state = TransactionState.Rendering
    }

    fun complete() {
        state = TransactionState.Completed
    }

    fun cancel() {
        state = TransactionState.Cancelled
    }

    fun isPaused(): Boolean = state == TransactionState.Paused

    fun isCompleted(frameTimeMs: Long): Boolean = progress(frameTimeMs) >= 1f

    fun getState(): TransactionState = state

    fun currentVisualFrame(frameTimeMs: Long): VisualFrameSnapshot? {
        if (state != TransactionState.Rendering && state != TransactionState.Paused) return null
        val p = progress(frameTimeMs)
        return VisualFrameSnapshot(progress = p, state = state)
    }
}

data class VisualFrameSnapshot(
    val progress: Float,
    val state: TransactionState,
    val sliceVisualStates: List<SliceVisualState> = emptyList(),
    val cursorRect: android.graphics.RectF? = null
)

data class SliceVisualState(
    val snapshotId: Long,
    val role: SliceRole,
    val lineIndex: Int,
    val documentByteStart: Int = -1,
    val documentByteEndExclusive: Int = -1,
    val clusterByteStart: Int = -1,
    val clusterByteEndExclusive: Int = -1,
    val currentLeft: Float,
    val currentTop: Float,
    val currentRight: Float,
    val currentBottom: Float,
    val currentAlpha: Float,
    val destinationLeft: Float = currentLeft,
    val destinationTop: Float = currentTop,
    val destinationRight: Float = currentRight,
    val destinationBottom: Float = currentBottom
)
