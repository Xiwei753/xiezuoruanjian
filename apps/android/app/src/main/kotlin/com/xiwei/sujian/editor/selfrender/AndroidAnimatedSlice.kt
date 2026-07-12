package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF

/**
 * 动画切片角色，与 Linux `AnimatedSliceKind` 使用同一套语义。
 *
 * - [Insert]：新快照视觉从光标附近进入目标位置（透明→不透明）。
 * - [Delete]：旧快照视觉向删除后的光标位置收缩并消失（不透明→透明）。
 * - [Move]：shaping identity 等价时复用旧视觉资源做几何移动。
 * - [CrossfadeOld]/[CrossfadeNew]：shaping 变化时必须成对出现，
 *   分别引用 old/new snapshot；旧视觉淡出、新视觉淡入。
 */
enum class AndroidAnimatedSliceRole {
    Insert, Delete, Move, CrossfadeOld, CrossfadeNew
}

/**
 * 一次动画切片的完整描述。
 *
 * 坐标契约：
 * - [sourceRect]：[sourceSnapshotId] 对应视觉资源内的裁剪区域（行局部坐标）。
 * - [fromDocumentRect]/[toDocumentRect]：文档坐标，不包含当前滚动偏移。
 * - [documentByteStart]/[documentByteEnd]：用于事务冲突判断和静态层隐藏，不参与逐帧排版。
 */
data class AndroidAnimatedSlice(
    val id: ULong,
    val role: AndroidAnimatedSliceRole,
    val sourceSnapshotId: AndroidLineSnapshotId?,
    val sourceRect: RectF,
    var fromDocumentRect: RectF,
    val toDocumentRect: RectF,
    var opacityFrom: Float,
    val opacityTo: Float,
    var scaleFrom: Float,
    val scaleTo: Float,
    val documentByteStart: Int,
    val documentByteEnd: Int,
    val shapingIdentity: String
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

    fun rebaseFrom(currentRect: RectF, currentAlpha: Int) {
        fromDocumentRect = currentRect
        opacityFrom = currentAlpha / 255f
        scaleFrom = 1f
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
            shapingIdentity: String
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
            shapingIdentity: String
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
            shapingIdentity: String
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
            shapingIdentity: String
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
