package com.xiwei.sujian.editor.v2.layout

import android.graphics.Bitmap
import android.graphics.Canvas
import android.text.Layout
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

class AndroidLineSnapshotBuilder {
    private var snapshotIdCounter: Long = 0L

    private fun nextSnapshotId(): Long {
        snapshotIdCounter++
        return System.nanoTime() + snapshotIdCounter
    }

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

        val snapshotId = nextSnapshotId()

        return AndroidLineSnapshot(
            snapshotId = snapshotId,
            bitmap = bitmap,
            lineIndex = lineIndex,
            sourceRect = android.graphics.Rect(0, 0, width, height),
            destinationRect = android.graphics.RectF(left, top, right, bottom),
            clusters = emptyList(),
            documentByteStart = lineRange.startUtf8,
            documentByteEndExclusive = lineRange.endUtf8,
            documentUtf16Start = lineRange.startUtf16,
            documentUtf16EndExclusive = lineRange.endUtf16,
            baseline = lineRange.baseline,
            lineHeight = bottom - top
        )
    }

    fun buildSnapshotForLineWithClusters(
        layout: Layout?,
        lineIndex: Int,
        revision: AndroidLayoutRevision?,
        mirror: DisplayTextMirror
    ): AndroidLineSnapshot? {
        val snapshot = buildSnapshotForLine(layout, lineIndex, revision) ?: return null
        if (layout == null) return snapshot

        val lineRange = revision?.lineRanges?.getOrNull(lineIndex) ?: return snapshot
        val clusters = buildClustersForLine(layout, lineIndex, lineRange, mirror)

        return snapshot.copy(clusters = clusters)
    }

    private fun buildClustersForLine(
        layout: Layout,
        lineIndex: Int,
        lineRange: AndroidLayoutRevision.LineRange,
        mirror: DisplayTextMirror
    ): List<LineClusterSnapshot> {
        val clusters = mutableListOf<LineClusterSnapshot>()
        val text = mirror.getText()
        val indexMap = AndroidTextIndexMap(mirror)

        val lineStartUtf16 = layout.getLineStart(lineIndex)
        val lineEndUtf16 = layout.getLineEnd(lineIndex)

        val lineText = text.substring(lineStartUtf16.coerceAtMost(text.length), lineEndUtf16.coerceAtMost(text.length))

        var clusterStartUtf16 = lineStartUtf16
        var clusterIdCounter = 0L

        var i = 0
        while (i < lineText.length) {
            val char = lineText[i]
            val isSurrogatePair = char.isHighSurrogate() && i + 1 < lineText.length && lineText[i + 1].isLowSurrogate()
            val clusterLenUtf16 = if (isSurrogatePair) 2 else 1
            val clusterEndUtf16 = clusterStartUtf16 + clusterLenUtf16

            val clusterStartUtf8 = indexMap.utf16ToUtf8(clusterStartUtf16)
            val clusterEndUtf8 = indexMap.utf16ToUtf8(clusterEndUtf16)

            val x0 = layout.getPrimaryHorizontal(clusterStartUtf16)
            val x1 = if (clusterEndUtf16 < layout.getLineEnd(lineIndex)) {
                layout.getPrimaryHorizontal(clusterEndUtf16)
            } else {
                layout.getLineRight(lineIndex)
            }

            val top = layout.getLineTop(lineIndex).toFloat()
            val bottom = layout.getLineBottom(lineIndex).toFloat()

            val sourceLeft = (x0 - lineRange.left).coerceAtLeast(0f)
            val sourceRight = (x1 - lineRange.left).coerceAtLeast(sourceLeft)
            val sourceTop = 0f
            val sourceBottom = bottom - top

            val shapingFp = buildShapingFingerprint(lineText, i, isSurrogatePair)

            clusters.add(LineClusterSnapshot(
                clusterId = clusterIdCounter++,
                documentByteStart = clusterStartUtf8,
                documentByteEndExclusive = clusterEndUtf8,
                documentUtf16Start = clusterStartUtf16,
                documentUtf16EndExclusive = clusterEndUtf16,
                sourceRectInLineImage = android.graphics.Rect(sourceLeft.toInt(), sourceTop.toInt(), sourceRight.toInt(), sourceBottom.toInt()),
                visualRectInDocument = android.graphics.RectF(x0, top, x1, bottom),
                shapingFingerprint = shapingFp
            ))

            clusterStartUtf16 = clusterEndUtf16
            i += clusterLenUtf16
        }

        return clusters
    }

    private fun buildShapingFingerprint(text: String, startIdx: Int, isSurrogatePair: Boolean): String {
        val char = text[startIdx]
        val codePoint = if (isSurrogatePair && startIdx + 1 < text.length) {
            Character.toCodePoint(char, text[startIdx + 1])
        } else {
            char.code
        }
        val type = Character.getType(codePoint)
        return "${codePoint}_${type}"
    }
}
