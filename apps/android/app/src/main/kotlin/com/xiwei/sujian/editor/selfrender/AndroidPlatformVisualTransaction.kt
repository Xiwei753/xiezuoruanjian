package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import com.xiwei.sujian.model.AnimationModeData
import com.xiwei.sujian.model.SujianCursorRectData

/**
 * 平台视觉事务状态机：
 *
 * ```
 * Pending → Prepared → Rendering ↔ Paused → Completed
 *    └──── 任一未终态 ────→ Cancelled
 * ```
 *
 * - [markPrepared]：前置状态必须为 Pending。
 * - [markRendering]：前置状态必须为 Prepared，同时记录 [firstRenderFrameMs]。
 * - [pause]：前置状态必须为 Rendering。
 * - [resume]：前置状态必须为 Paused，累加暂停时长后恢复。
 * - [complete]/[cancel]：释放 old/new snapshots；重复释放由 snapshot/resource 层保证安全。
 */
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

/**
 * 一次平台视觉事务持有的全部资源。
 *
 * [firstRenderFrameMs] 是动画可见计时起点，不能用事务创建时间替代——
 * 事务可能在 Pending/Prepared 状态停留多帧，此时用户看不到动画。
 *
 * [accumulatedPausedDurationMs]：暂停期间不计入动画进度，
 * 恢复后 progress 从暂停点连续推进。
 *
 * [progress] 在 Paused 状态不应推进，恢复后连续。
 */
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

    internal fun releaseSnapshots() {
        for (snapshot in oldLineSnapshots) {
            snapshot.release()
        }
        for (snapshot in newLineSnapshots) {
            snapshot.release()
        }
    }
}
