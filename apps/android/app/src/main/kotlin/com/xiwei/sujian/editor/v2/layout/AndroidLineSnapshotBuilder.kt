package com.xiwei.sujian.editor.v2.layout

import android.graphics.Bitmap

class AndroidLineSnapshot(
    val snapshotId: Long,
    val bitmap: Bitmap?,
    val lineIndex: Int,
    val sourceRect: android.graphics.Rect,
    val destinationRect: android.graphics.RectF
)

class AndroidLineSnapshotBuilder {
    fun buildSnapshots(
        layout: android.text.Layout?,
        revision: AndroidLayoutRevision?,
        startIndex: Int,
        endIndex: Int
    ): List<AndroidLineSnapshot> {
        if (layout == null || revision == null) return emptyList()

        val snapshots = mutableListOf<AndroidLineSnapshot>()
        for (i in startIndex.coerceAtLeast(0) until endIndex.coerceAtMost(layout.lineCount)) {
            val left = layout.getLineLeft(i)
            val right = layout.getLineRight(i)
            val top = layout.getLineTop(i)
            val bottom = layout.getLineBottom(i)

            val width = (right - left).toInt().coerceAtLeast(1)
            val height = (bottom - top).toInt().coerceAtLeast(1)

            val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
            val canvas = android.graphics.Canvas(bitmap)
            canvas.translate(-left, -top.toFloat())
            layout.draw(canvas)

            snapshots.add(AndroidLineSnapshot(
                snapshotId = System.nanoTime(),
                bitmap = bitmap,
                lineIndex = i,
                sourceRect = android.graphics.Rect(0, 0, width, height),
                destinationRect = android.graphics.RectF(left, top.toFloat(), right, bottom.toFloat())
            ))
        }
        return snapshots
    }
}
