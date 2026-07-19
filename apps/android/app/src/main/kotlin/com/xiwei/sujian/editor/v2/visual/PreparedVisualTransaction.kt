package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision

data class PreparedVisualTransaction(
    val transactionId: Long,
    val oldRevision: AndroidLayoutRevision?,
    val newRevision: AndroidLayoutRevision?,
    val staticPatches: List<StaticPatch>,
    val animatedSlices: List<AnimatedSlice>,
    val ownedSnapshotIds: Set<Long>,
    val selectionDecoration: SelectionDecoration?,
    val preeditDecoration: PreeditDecoration?,
    val cursorTransition: CursorTransition?,
    val durationMs: Long
) {
    data class StaticPatch(
        val newSnapshotId: Long,
        val lineIndex: Int,
        val destinationRect: android.graphics.RectF,
        val visibleSourceRects: List<android.graphics.Rect>
    )

    data class AnimatedSlice(
        val role: SliceRole,
        val snapshot: AndroidLineSnapshot?,
        val sourceRect: android.graphics.Rect,
        val destinationRect: android.graphics.RectF,
        val startAlpha: Float,
        val endAlpha: Float,
        val fromDestinationRect: android.graphics.RectF? = null,
        val clusterByteStart: Int = -1,
        val clusterByteEndExclusive: Int = -1
    )

    data class SelectionDecoration(
        val startUtf16: Int,
        val endUtf16: Int,
        val rects: List<android.graphics.RectF>
    )

    data class PreeditDecoration(
        val startUtf16: Int,
        val endUtf16: Int,
        val underlineColor: Int
    )

    data class CursorTransition(
        val fromX: Float,
        val fromY: Float,
        val fromHeight: Float,
        val toX: Float,
        val toY: Float,
        val toHeight: Float,
        val shouldAnimate: Boolean
    )
}

enum class SliceRole {
    Insert, Delete, Move, CrossfadeOld, CrossfadeNew, Static
}

enum class TransactionState {
    Pending, Prepared, Rendering, Paused, Completed, Cancelled
}
