package com.xiwei.sujian.editor.v2.layout

import android.graphics.Bitmap
import android.graphics.Canvas
import android.text.Layout

class AndroidLineSnapshotBuilder {
    fun buildSnapshots(
        layout: Layout?,
        revision: AndroidLayoutRevision?,
        startIndex: Int,
        endIndex: Int
    ): List<AndroidLineSnapshot> {
        if (layout == null || revision == null) return emptyList()

        val snapshots = mutableListOf<AndroidLineSnapshot>()
        val safeStart = startIndex.coerceAtLeast(0)
        val safeEnd = endIndex.coerceAtMost(layout.lineCount)

        for (i in safeStart until safeEnd) {
            val lineRange = revision.lineRanges.getOrNull(i) ?: continue

            val left = lineRange.left
            val right = lineRange.right
            val top = lineRange.top
            val bottom = lineRange.bottom

            val width = (right - left).toInt().coerceAtLeast(1)
            val height = (bottom - top).toInt().coerceAtLeast(1)

            val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(bitmap)
            canvas.translate(-left, -top)
            layout.draw(canvas)

            snapshots.add(AndroidLineSnapshot(
                snapshotId = System.nanoTime() + i,
                bitmap = bitmap,
                lineIndex = i,
                sourceRect = android.graphics.Rect(0, 0, width, height),
                destinationRect = android.graphics.RectF(left, top, right, bottom)
            ))
        }
        return snapshots
    }

    fun buildSnapshotForLine(
        layout: Layout?,
        lineIndex: Int,
        revision: AndroidLayoutRevision?
    ): AndroidLineSnapshot? {
        if (layout == null || revision == null) return null
        if (lineIndex < 0 || lineIndex >= layout.lineCount) return null
        return buildSnapshots(layout, revision, lineIndex, lineIndex + 1).firstOrNull()
    }
}
