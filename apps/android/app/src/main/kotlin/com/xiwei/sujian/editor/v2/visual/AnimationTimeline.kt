package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot

class AnimationTimeline(
    private val durationMs: Long
) {
    private var firstVisibleFrameTimeMs: Long? = null
    private var pauseStartedAtMs: Long? = null
    private var accumulatedPausedDurationMs: Long = 0
    private var pausedProgress: Float = 0f

    fun progress(frameTimeMs: Long): Float {
        val start = firstVisibleFrameTimeMs ?: return 0f
        if (pauseStartedAtMs != null) return pausedProgress
        if (durationMs == 0L) return 1f

        val effectiveElapsed = frameTimeMs - start - accumulatedPausedDurationMs
        val p = effectiveElapsed.toFloat() / durationMs.toFloat()
        return p.coerceIn(0f, 1f)
    }

    fun markFirstVisibleFrame(frameTimeMs: Long) {
        if (firstVisibleFrameTimeMs == null) {
            firstVisibleFrameTimeMs = frameTimeMs
        }
    }

    fun pause(frameTimeMs: Long) {
        if (pauseStartedAtMs != null) return
        pausedProgress = progress(frameTimeMs)
        pauseStartedAtMs = frameTimeMs
    }

    fun resume(frameTimeMs: Long) {
        if (pauseStartedAtMs == null) return
        if (firstVisibleFrameTimeMs == null) {
            pauseStartedAtMs = null
            pausedProgress = 0f
            return
        }
        val newStart = frameTimeMs - (pausedProgress * durationMs).toLong()
        firstVisibleFrameTimeMs = newStart
        accumulatedPausedDurationMs = 0
        pauseStartedAtMs = null
        pausedProgress = 0f
    }

    fun isPaused(): Boolean = pauseStartedAtMs != null

    fun isCompleted(frameTimeMs: Long): Boolean = progress(frameTimeMs) >= 1f
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
}
