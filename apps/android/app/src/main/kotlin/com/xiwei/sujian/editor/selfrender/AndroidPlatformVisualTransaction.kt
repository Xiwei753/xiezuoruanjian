package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import com.xiwei.sujian.model.AnimationModeData
import com.xiwei.sujian.model.SujianCursorRectData

enum class AndroidVisualTransactionState {
    Pending, Prepared, Rendering, Paused, Completed, Cancelled
}

enum class AndroidVisualOperationKind {
    Insert, Delete, Cursor, CompositionUpdate, CompositionCommitOrCancel
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
    val decorationSlices: MutableList<AndroidDecorationSlice>,
    var cursorTransition: AndroidCursorTransition,
    var cancelReason: String? = null,
    val ownedOldRevision: AndroidCompositionVisualRevision? = null,
    val ownedNewRevision: AndroidCompositionVisualRevision? = null
) {
    val timeline: AndroidTimeline = AndroidTimeline(durationMs)

    val progress: Float
        get() {
            if (state == AndroidVisualTransactionState.Paused) {
                return timeline.pausedProgress.coerceIn(0f, 1f)
            }
            if (state != AndroidVisualTransactionState.Rendering) return 0f
            if (durationMs <= 0) return 1f
            return timeline.progress(System.currentTimeMillis())
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
            timeline.recordFirstFrame(System.currentTimeMillis())
        }
    }

    fun pause() {
        if (state == AndroidVisualTransactionState.Rendering) {
            state = AndroidVisualTransactionState.Paused
            timeline.pause(System.currentTimeMillis())
        }
    }

    fun resume() {
        if (state == AndroidVisualTransactionState.Paused) {
            state = AndroidVisualTransactionState.Rendering
            timeline.resume(System.currentTimeMillis())
        }
    }

    fun complete() {
        state = AndroidVisualTransactionState.Completed
        releaseOwnedResources()
    }

    fun cancel(reason: String) {
        cancelReason = reason
        state = AndroidVisualTransactionState.Cancelled
        releaseOwnedResources()
    }

    private fun releaseOwnedResources() {
        ownedOldRevision?.let { rev ->
            if (!rev.isReleased()) rev.release()
        }
        ownedNewRevision?.let { rev ->
            if (!rev.isReleased()) rev.release()
        }
        if (ownedOldRevision == null) {
            for (snapshot in oldLineSnapshots) {
                snapshot.release()
            }
        }
        if (ownedNewRevision == null) {
            for (snapshot in newLineSnapshots) {
                snapshot.release()
            }
        }
    }
}
