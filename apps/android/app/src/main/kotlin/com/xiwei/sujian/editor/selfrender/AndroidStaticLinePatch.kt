package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF

/**
 * 静态正文层在动画期间不能继续完整绘制受影响行，否则会与 overlay 双绘。
 * patch 负责从已录制行快照中只绘制未被动画切片接管的部分。
 *
 * [visibleSourceRects] 来自行视觉资源局部坐标，不从 UTF-16 offset 在动画帧中重新测量。
 * [destinationDocumentRect] 是文档坐标。
 */
data class AndroidStaticLinePatch(
    val newSnapshotId: AndroidLineSnapshotId?,
    val destinationDocumentRect: RectF,
    val visibleSourceRects: List<RectF>
)
