package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF

enum class AndroidAnimatedSliceRole {
    Insert, Delete, Move, CrossfadeOld, CrossfadeNew, SnapshotOld, SnapshotNew
}

data class AndroidAnimatedSlice(
    val id: ULong,
    val role: AndroidAnimatedSliceRole,
    val sourceSnapshotId: AndroidLineSnapshotId?,
    val sourceRect: RectF,
    val fromDocumentRect: RectF,
    val toDocumentRect: RectF,
    val opacityFrom: Float,
    val opacityTo: Float,
    val scaleFrom: Float,
    val scaleTo: Float,
    val documentByteStart: Int,
    val documentByteEnd: Int,
    val shapingIdentity: String?
) {
    fun computeFrame(progress: Float, easedProgress: Float): SliceFrame {
        val x = fromDocumentRect.left + (toDocumentRect.left - fromDocumentRect.left) * easedProgress
        val y = fromDocumentRect.top + (toDocumentRect.top - fromDocumentRect.top) * easedProgress
        val w = fromDocumentRect.width() + (toDocumentRect.width() - fromDocumentRect.width()) * easedProgress
        val h = fromDocumentRect.height() + (toDocumentRect.height() - fromDocumentRect.height()) * easedProgress
        val alpha = (opacityFrom + (opacityTo - opacityFrom) * easedProgress) * 255f
        val scale = scaleFrom + (scaleTo - scaleFrom) * easedProgress
        return SliceFrame(
            destinationRect = RectF(x, y, x + w, y + h),
            alpha = alpha.toInt().coerceIn(0, 255),
            scale = scale
        )
    }

    data class SliceFrame(
        val destinationRect: RectF,
        val alpha: Int,
        val scale: Float
    )

    companion object {
        fun insertFadeIn(
            id: ULong,
            snapshotId: AndroidLineSnapshotId?,
            sourceRect: RectF,
            fromRect: RectF,
            toRect: RectF,
            byteStart: Int,
            byteEnd: Int,
            shapingIdentity: String? = null
        ): AndroidAnimatedSlice = AndroidAnimatedSlice(
            id = id,
            role = AndroidAnimatedSliceRole.Insert,
            sourceSnapshotId = snapshotId,
            sourceRect = sourceRect,
            fromDocumentRect = fromRect,
            toDocumentRect = toRect,
            opacityFrom = 0f,
            opacityTo = 1f,
            scaleFrom = 1f,
            scaleTo = 1f,
            documentByteStart = byteStart,
            documentByteEnd = byteEnd,
            shapingIdentity = shapingIdentity
        )

        fun deleteFadeOut(
            id: ULong,
            snapshotId: AndroidLineSnapshotId?,
            sourceRect: RectF,
            fromRect: RectF,
            toRect: RectF,
            byteStart: Int,
            byteEnd: Int,
            shapingIdentity: String? = null
        ): AndroidAnimatedSlice = AndroidAnimatedSlice(
            id = id,
            role = AndroidAnimatedSliceRole.Delete,
            sourceSnapshotId = snapshotId,
            sourceRect = sourceRect,
            fromDocumentRect = fromRect,
            toDocumentRect = toRect,
            opacityFrom = 1f,
            opacityTo = 0f,
            scaleFrom = 1f,
            scaleTo = 0.7f,
            documentByteStart = byteStart,
            documentByteEnd = byteEnd,
            shapingIdentity = shapingIdentity
        )

        fun reflowMove(
            id: ULong,
            snapshotId: AndroidLineSnapshotId?,
            sourceRect: RectF,
            fromRect: RectF,
            toRect: RectF,
            byteStart: Int,
            byteEnd: Int,
            shapingIdentity: String? = null
        ): AndroidAnimatedSlice = AndroidAnimatedSlice(
            id = id,
            role = AndroidAnimatedSliceRole.Move,
            sourceSnapshotId = snapshotId,
            sourceRect = sourceRect,
            fromDocumentRect = fromRect,
            toDocumentRect = toRect,
            opacityFrom = 1f,
            opacityTo = 1f,
            scaleFrom = 1f,
            scaleTo = 1f,
            documentByteStart = byteStart,
            documentByteEnd = byteEnd,
            shapingIdentity = shapingIdentity
        )

        fun crossfade(
            id: ULong,
            role: AndroidAnimatedSliceRole,
            snapshotId: AndroidLineSnapshotId?,
            sourceRect: RectF,
            fromRect: RectF,
            toRect: RectF,
            byteStart: Int,
            byteEnd: Int,
            shapingIdentity: String? = null
        ): AndroidAnimatedSlice = AndroidAnimatedSlice(
            id = id,
            role = role,
            sourceSnapshotId = snapshotId,
            sourceRect = sourceRect,
            fromDocumentRect = fromRect,
            toDocumentRect = toRect,
            opacityFrom = if (role == AndroidAnimatedSliceRole.CrossfadeOld) 1f else 0f,
            opacityTo = if (role == AndroidAnimatedSliceRole.CrossfadeOld) 0f else 1f,
            scaleFrom = 1f,
            scaleTo = 1f,
            documentByteStart = byteStart,
            documentByteEnd = byteEnd,
            shapingIdentity = shapingIdentity
        )
    }
}
