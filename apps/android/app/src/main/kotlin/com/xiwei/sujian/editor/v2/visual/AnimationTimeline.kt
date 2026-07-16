package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot

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
}

class VisualResourceStore {
    private val snapshots = mutableMapOf<Long, AndroidLineSnapshot>()

    fun put(snapshot: AndroidLineSnapshot) {
        snapshots[snapshot.snapshotId] = snapshot
    }

    fun get(snapshotId: Long): AndroidLineSnapshot? = snapshots[snapshotId]

    fun release(snapshotId: Long) {
        val snapshot = snapshots.remove(snapshotId)
        snapshot?.bitmap?.recycle()
    }

    fun releaseAll() {
        snapshots.values.forEach { it.bitmap?.recycle() }
        snapshots.clear()
    }

    fun transferOwnership(fromSnapshotId: Long, toSnapshotId: Long): Boolean {
        val snapshot = snapshots.remove(fromSnapshotId) ?: return false
        snapshots[toSnapshotId] = snapshot
        return true
    }
}
