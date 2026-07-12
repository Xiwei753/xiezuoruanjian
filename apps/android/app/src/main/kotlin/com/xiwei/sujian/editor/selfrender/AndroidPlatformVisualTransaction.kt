package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import com.xiwei.sujian.model.AnimationModeData
import com.xiwei.sujian.model.SujianCursorRectData

enum class AndroidVisualTransactionState {
    Pending, Prepared, Rendering, Paused, Completed, Cancelled
}

enum class AndroidVisualOperationKind {
    Insert, Delete, Cursor
}

data class AndroidCursorTransition(
    val isSnap: Boolean,
    val oldRect: RectF?,
    val newRect: RectF?,
    val durationMs: Long
) {
    companion object {
        fun snap(rect: RectF): AndroidCursorTransition =
            AndroidCursorTransition(isSnap = true, oldRect = rect, newRect = rect, durationMs = 0)

        fun tween(oldRect: RectF, newRect: RectF, durationMs: Long): AndroidCursorTransition =
            AndroidCursorTransition(isSnap = false, oldRect = oldRect, newRect = newRect, durationMs = durationMs)
    }
}

data class AndroidPlatformVisualTransaction(
    val key: ULong,
    var state: AndroidVisualTransactionState,
    val operationKind: AndroidVisualOperationKind,
    val animationMode: AnimationModeData,
    val durationMs: Long,
    val oldRevision: Long,
    val newRevision: Long,
    val slices: MutableList<AndroidAnimatedSlice>,
    val oldLineSnapshots: MutableList<AndroidLineSnapshot>,
    val newLineSnapshots: MutableList<AndroidLineSnapshot>,
    val staticLinePatches: MutableList<AndroidStaticLinePatch>,
    var cursorTransition: AndroidCursorTransition,
    var startTimeMs: Long = 0L,
    var firstRenderFrameMs: Long = 0L,
    var accumulatedPausedDurationMs: Long = 0L,
    var pauseStartMs: Long = 0L,
    var cancelReason: String? = null
) {
    val progress: Float
        get() {
            if (state != AndroidVisualTransactionState.Rendering) return 0f
            if (durationMs <= 0) return 1f
            val now = System.currentTimeMillis()
            val effectiveStart = firstRenderFrameMs
            if (effectiveStart <= 0) return 0f
            val elapsed = (now - effectiveStart - accumulatedPausedDurationMs).coerceAtLeast(0L)
            return (elapsed.toFloat() / durationMs.toFloat()).coerceIn(0f, 1f)
        }

    val isFinished: Boolean
        get() = progress >= 1f || state == AndroidVisualTransactionState.Completed || state == AndroidVisualTransactionState.Cancelled

    fun markPrepared() {
        if (state == AndroidVisualTransactionState.Pending) {
            state = AndroidVisualTransactionState.Prepared
        }
    }

    fun markRendering() {
        if (state == AndroidVisualTransactionState.Prepared) {
            state = AndroidVisualTransactionState.Rendering
            val now = System.currentTimeMillis()
            if (firstRenderFrameMs <= 0L) {
                firstRenderFrameMs = now
            }
            startTimeMs = now
        }
    }

    fun pause() {
        if (state == AndroidVisualTransactionState.Rendering) {
            state = AndroidVisualTransactionState.Paused
            pauseStartMs = System.currentTimeMillis()
        }
    }

    fun resume() {
        if (state == AndroidVisualTransactionState.Paused) {
            state = AndroidVisualTransactionState.Rendering
            if (pauseStartMs > 0) {
                accumulatedPausedDurationMs += System.currentTimeMillis() - pauseStartMs
                pauseStartMs = 0L
            }
        }
    }

    fun complete() {
        state = AndroidVisualTransactionState.Completed
        releaseSnapshots()
    }

    fun cancel(reason: String) {
        cancelReason = reason
        state = AndroidVisualTransactionState.Cancelled
        releaseSnapshots()
    }

    private fun releaseSnapshots() {
        for (snapshot in oldLineSnapshots) {
            snapshot.release()
        }
        for (snapshot in newLineSnapshots) {
            snapshot.release()
        }
    }
}
