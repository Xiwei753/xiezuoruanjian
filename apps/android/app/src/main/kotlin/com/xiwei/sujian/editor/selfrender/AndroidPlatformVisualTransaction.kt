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
    var ownedOldRevision: AndroidCompositionVisualRevision? = null,
    var ownedNewRevision: AndroidCompositionVisualRevision? = null,
    var onTransactionComplete: ((AndroidCompositionVisualRevision, ULong) -> Unit)? = null
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
        check(state != AndroidVisualTransactionState.Completed && state != AndroidVisualTransactionState.Cancelled) {
            "Transaction $key already in terminal state $state"
        }
        state = AndroidVisualTransactionState.Completed
        releaseOldSnapshots()
        val newRev = ownedNewRevision
        ownedOldRevision = null
        ownedNewRevision = null
        if (newRev != null) {
            onTransactionComplete?.invoke(newRev, key)
        } else {
            releaseNewSnapshots()
        }
    }

    fun cancel(reason: String) {
        check(state != AndroidVisualTransactionState.Completed && state != AndroidVisualTransactionState.Cancelled) {
            "Transaction $key already in terminal state $state"
        }
        cancelReason = reason
        state = AndroidVisualTransactionState.Cancelled
        releaseOldSnapshots()
        releaseNewSnapshots()
        ownedOldRevision = null
        ownedNewRevision = null
    }

    private fun releaseOldSnapshots() {
        val oldRev = ownedOldRevision
        if (oldRev != null) {
            oldRev.release()
        } else {
            for (snapshot in oldLineSnapshots) {
                if (!snapshot.isReleased()) snapshot.release()
            }
        }
    }

    private fun releaseNewSnapshots() {
        val newRev = ownedNewRevision
        if (newRev != null) {
            newRev.release()
        } else {
            for (snapshot in newLineSnapshots) {
                if (!snapshot.isReleased()) snapshot.release()
            }
        }
    }

    internal fun releaseSnapshots() {
        releaseOldSnapshots()
        releaseNewSnapshots()
    }

    fun detachOldRevisionForRebase(): AndroidCompositionVisualRevision? {
        val rev = ownedOldRevision
        ownedOldRevision = null
        if (rev != null) {
            oldLineSnapshots.clear()
        }
        return rev
    }

    fun takeNewRevisionForRebase(): AndroidCompositionVisualRevision? {
        val rev = ownedNewRevision
        ownedNewRevision = null
        if (rev != null) {
            newLineSnapshots.clear()
        }
        return rev
    }
}
