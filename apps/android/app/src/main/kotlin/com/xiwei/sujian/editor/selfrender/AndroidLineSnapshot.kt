package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF

data class AndroidLineSnapshotId(
    val revision: Long,
    val visualLineOrdinal: Int
)

data class AndroidClusterSnapshot(
    val documentByteStart: Int,
    val documentByteEnd: Int,
    val platformTextStart: Int,
    val platformTextEnd: Int,
    val sourceRectInLineSnapshot: RectF,
    val visualRectInDocument: RectF,
    val textDirection: Int,
    val shapingIdentity: String?
)

data class AndroidLineSnapshot(
    val id: AndroidLineSnapshotId,
    val revision: Long,
    val paragraphId: Int,
    val visualLineOrdinal: Int,
    val documentByteStart: Int,
    val documentByteEnd: Int,
    val platformTextStart: Int,
    val platformTextEnd: Int,
    val documentRect: RectF,
    val baseline: Float,
    val lineImageLocalSize: RectF,
    val clusters: List<AndroidClusterSnapshot>,
    val visualResource: AndroidLineVisualResource?
) {
    fun release() {
        visualResource?.release()
    }
}
