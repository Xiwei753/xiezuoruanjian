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
            val snapshot = buildSnapshotForLine(layout, i, revision)
            if (snapshot != null) {
                snapshots.add(snapshot)
            }
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

        val lineRange = revision.lineRanges.getOrNull(lineIndex) ?: return null

        val left = lineRange.left
        val right = lineRange.right
        val top = lineRange.top
        val bottom = lineRange.bottom

        val width = (right - left).toInt().coerceAtLeast(1)
        val height = (bottom - top).toInt().coerceAtLeast(1)

        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        canvas.translate(-left, -top)
        canvas.clipRect(left, top, right, bottom)
        layout.draw(canvas)

        return AndroidLineSnapshot(
            snapshotId = System.nanoTime() + lineIndex,
            bitmap = bitmap,
            lineIndex = lineIndex,
            sourceRect = android.graphics.Rect(0, 0, width, height),
            destinationRect = android.graphics.RectF(left, top, right, bottom)
        )
    }
}
