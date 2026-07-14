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

/**
 * 一次平台视觉事务持有的全部资源。
 *
 * 统一时钟原则（issue #515）：
 * - [timeline] 是唯一时间源，文字切片、光标、预输入装饰全部消费同一个 progress。
 * - Paused 状态返回暂停瞬间的 progress，不返回 0。
 * - resume 后从暂停进度连续推进。
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
    var cancelReason: String? = null
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
