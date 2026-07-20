package com.xiwei.sujian.editor.v2.layout

import android.graphics.Bitmap
import android.graphics.Canvas
import android.text.Layout
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import android.icu.text.BreakIterator

class AndroidLineSnapshotBuilder {
    private var snapshotIdCounter: Long = 0L

    private fun nextSnapshotId(): Long {
        snapshotIdCounter++
        return snapshotIdCounter
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

        val graphemeRanges = computeGraphemeRanges(lineText)

        var clusterIdCounter = 0L

        for ((start, end) in graphemeRanges) {
            val clusterStartUtf16 = lineStartUtf16 + start
            val clusterEndUtf16 = lineStartUtf16 + end

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

            val localStart = start.coerceIn(0, lineText.length)
            val localEnd = end.coerceIn(0, lineText.length)
            val clusterText = lineText.substring(localStart, localEnd)
            val shapingFp = buildShapingFingerprint(clusterText, layout, lineIndex, clusterStartUtf16)
            val isConfident = android.os.Build.VERSION.SDK_INT >= 31

            clusters.add(LineClusterSnapshot(
                clusterId = clusterIdCounter++,
                documentByteStart = clusterStartUtf8,
                documentByteEndExclusive = clusterEndUtf8,
                documentUtf16Start = clusterStartUtf16,
                documentUtf16EndExclusive = clusterEndUtf16,
                sourceRectInLineImage = android.graphics.Rect(sourceLeft.toInt(), sourceTop.toInt(), sourceRight.toInt(), sourceBottom.toInt()),
                visualRectInDocument = android.graphics.RectF(x0, top, x1, bottom),
                shapingFingerprint = shapingFp,
                shapingIdentityConfident = isConfident
            ))
        }

        return clusters
    }

    private fun computeGraphemeRanges(text: String): List<Pair<Int, Int>> {
        if (text.isEmpty()) return emptyList()

        val ranges = mutableListOf<Pair<Int, Int>>()
        val iter = BreakIterator.getCharacterInstance()
        iter.setText(text)

        var start = iter.first()
        if (start != BreakIterator.DONE) {
            var end = iter.next()
            while (end != BreakIterator.DONE) {
                if (start < end) {
                    ranges.add(Pair(start, end))
                }
                start = end
                end = iter.next()
            }
        }

        return ranges
    }


    private fun buildShapingFingerprint(clusterText: String, layout: Layout, lineIndex: Int, clusterStartUtf16: Int): String {
        if (clusterText.isEmpty()) return ""
        if (android.os.Build.VERSION.SDK_INT >= 31) {
            return buildShapingFingerprintApi31(clusterText, layout, lineIndex, clusterStartUtf16)
        }
        val codePoints = clusterText.codePoints().toArray()
        val typeSummary = codePoints.map { Character.getType(it) }.distinct().sorted().joinToString(",")
        val paintHash = layout.paint?.hashCode() ?: 0
        val bidiDir = if (clusterStartUtf16 < layout.getLineEnd(lineIndex)) {
            layout.getParagraphDirection(lineIndex)
        } else {
            Layout.DIR_LEFT_TO_RIGHT
        }
        return "${codePoints.joinToString(",")}_${typeSummary}_${paintHash}_${bidiDir}"
    }

    private fun buildShapingFingerprintApi31(clusterText: String, layout: Layout, lineIndex: Int, clusterStartUtf16: Int): String {
        try {
            val paint = layout.paint ?: return clusterText.hashCode().toString()
            val shaperClass = Class.forName("android.text.TextRunShaper")
            val shapeMethod = shaperClass.getMethod(
                "shapeText",
                CharSequence::class.java,
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                android.text.TextPaint::class.java
            )
            val positionedGlyphs = shapeMethod.invoke(
                null,
                clusterText, 0, clusterText.length,
                layout.getParagraphDirection(lineIndex),
                paint
            ) ?: return clusterText.hashCode().toString()

            val glyphCountMethod = positionedGlyphs.javaClass.getMethod("getGlyphCount")
            val glyphCount = glyphCountMethod.invoke(positionedGlyphs) as Int

            val getFontMethod = positionedGlyphs.javaClass.getMethod("getFont", Int::class.javaPrimitiveType)
            val getGlyphIdMethod = positionedGlyphs.javaClass.getMethod("getGlyphId", Int::class.javaPrimitiveType)
            val getXMethod = positionedGlyphs.javaClass.getMethod("getX", Int::class.javaPrimitiveType)
            val getYMethod = positionedGlyphs.javaClass.getMethod("getY", Int::class.javaPrimitiveType)

            val sb = StringBuilder()
            for (i in 0 until glyphCount) {
                if (i > 0) sb.append("|")
                val font = getFontMethod.invoke(positionedGlyphs, i)
                sb.append(font?.hashCode()?.toString() ?: "null")
                sb.append("_")
                sb.append(getGlyphIdMethod.invoke(positionedGlyphs, i))
                sb.append("_")
                sb.append((getXMethod.invoke(positionedGlyphs, i) as Float).toInt())
                sb.append("_")
                sb.append((getYMethod.invoke(positionedGlyphs, i) as Float).toInt())
            }
            return sb.toString()
        } catch (_: Exception) {
            return clusterText.hashCode().toString()
        }
    }
}
