package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

class AndroidLayoutSnapshotBuilder(
    private val layout: SujianEditorLayout,
    private val textPaint: TextPaint
) {
    private val TAG = "AndroidSnapshotBuilder"
    private var nextRevision: Long = 1L

    fun currentRevision(): Long = nextRevision

    fun buildLineSnapshots(
        text: String,
        affectedLineRange: IntRange,
        revision: Long,
        textColor: Int
    ): List<AndroidLineSnapshot> {
        val staticLayout = layout.getLayout(text)
        val result = mutableListOf<AndroidLineSnapshot>()

        val startLine = affectedLineRange.first.coerceIn(0, staticLayout.lineCount - 1)
        val endLine = affectedLineRange.last.coerceIn(0, staticLayout.lineCount - 1)

        for (lineIdx in startLine..endLine) {
            val snapshot = buildLineSnapshot(text, staticLayout, lineIdx, revision, textColor)
            if (snapshot != null) {
                result.add(snapshot)
            }
        }
        return result
    }

    fun buildAllLineSnapshots(
        text: String,
        revision: Long,
        textColor: Int
    ): List<AndroidLineSnapshot> {
        val staticLayout = layout.getLayout(text)
        val result = mutableListOf<AndroidLineSnapshot>()

        for (lineIdx in 0 until staticLayout.lineCount) {
            val snapshot = buildLineSnapshot(text, staticLayout, lineIdx, revision, textColor)
            if (snapshot != null) {
                result.add(snapshot)
            }
        }
        return result
    }

    private fun buildLineSnapshot(
        text: String,
        staticLayout: Layout,
        lineIdx: Int,
        revision: Long,
        textColor: Int
    ): AndroidLineSnapshot? {
        if (lineIdx < 0 || lineIdx >= staticLayout.lineCount) return null

        val lineStart = staticLayout.getLineStart(lineIdx)
        val lineEnd = staticLayout.getLineEnd(lineIdx)
        val lineTop = staticLayout.getLineTop(lineIdx)
        val lineBottom = staticLayout.getLineBottom(lineIdx)
        val lineLeft = staticLayout.getLineLeft(lineIdx)
        val lineRight = staticLayout.getLineRight(lineIdx)
        val baseline = staticLayout.getLineBaseline(lineIdx)

        val documentRect = RectF(
            lineLeft, lineTop.toFloat(),
            lineRight, lineBottom.toFloat()
        )

        val lineImageLocalSize = RectF(
            0f, 0f,
            (lineRight - lineLeft).coerceAtLeast(0f),
            (lineBottom - lineTop).coerceAtLeast(0f)
        )

        val visualResource = AndroidLineVisualResourceFactory.create(lineIdx)
        visualResource.record(staticLayout, lineIdx, textPaint, textColor, 0, 0)

        val clusters = buildClusterSnapshots(text, staticLayout, lineIdx, lineStart, lineEnd)

        val byteStart = SujianEditorBuffer.utf16ToUtf8(text, lineStart)
        val byteEnd = SujianEditorBuffer.utf16ToUtf8(text, lineEnd.coerceAtMost(text.length))

        return AndroidLineSnapshot(
            id = AndroidLineSnapshotId(revision, lineIdx),
            revision = revision,
            paragraphId = findParagraphId(text, lineStart),
            visualLineOrdinal = lineIdx,
            documentByteStart = byteStart,
            documentByteEnd = byteEnd,
            platformTextStart = lineStart,
            platformTextEnd = lineEnd,
            documentRect = documentRect,
            baseline = baseline.toFloat(),
            lineImageLocalSize = lineImageLocalSize,
            clusters = clusters,
            visualResource = visualResource
        )
    }

    private fun buildClusterSnapshots(
        text: String,
        staticLayout: Layout,
        lineIdx: Int,
        lineStart: Int,
        lineEnd: Int
    ): List<AndroidClusterSnapshot> {
        val clusters = mutableListOf<AndroidClusterSnapshot>()
        if (lineStart >= lineEnd || text.isEmpty()) return clusters

        var currentOffset = lineStart
        while (currentOffset < lineEnd.coerceAtMost(text.length)) {
            val clusterEnd = findClusterBoundary(text, currentOffset, lineEnd)
            val clusterText = text.substring(currentOffset, clusterEnd.coerceAtMost(text.length))

            val x = staticLayout.getPrimaryHorizontal(currentOffset)
            val nextX = if (clusterEnd < text.length) {
                staticLayout.getPrimaryHorizontal(clusterEnd)
            } else {
                x + textPaint.measureText(clusterText)
            }
            val baseline = staticLayout.getLineBaseline(lineIdx).toFloat()
            val ascent = staticLayout.getLineAscent(lineIdx).toFloat()
            val descent = staticLayout.getLineDescent(lineIdx).toFloat()

            val visualRect = RectF(
                x, baseline + ascent,
                nextX, baseline + descent
            )

            val sourceRect = RectF(
                x - staticLayout.getLineLeft(lineIdx),
                0f,
                nextX - staticLayout.getLineLeft(lineIdx),
                (baseline + descent - (baseline + ascent)).coerceAtLeast(0f)
            )

            val byteStart = SujianEditorBuffer.utf16ToUtf8(text, currentOffset)
            val byteEnd = SujianEditorBuffer.utf16ToUtf8(text, clusterEnd.coerceAtMost(text.length))

            clusters.add(AndroidClusterSnapshot(
                documentByteStart = byteStart,
                documentByteEnd = byteEnd,
                platformTextStart = currentOffset,
                platformTextEnd = clusterEnd,
                sourceRectInLineSnapshot = sourceRect,
                visualRectInDocument = visualRect,
                textDirection = if (staticLayout.getParagraphDirection(lineIdx) == Layout.DIR_RIGHT_TO_LEFT) 1 else 0,
                shapingIdentity = null
            ))

            currentOffset = clusterEnd
        }
        return clusters
    }

    private fun findClusterBoundary(text: String, start: Int, lineEnd: Int): Int {
        if (start >= text.length) return start
        val codePoint = text.codePointAt(start)
        val charCount = Character.charCount(codePoint)
        var end = start + charCount

        while (end < lineEnd.coerceAtMost(text.length)) {
            val nextCp = text.codePointAt(end)
            val nextType = Character.getType(nextCp)
            if (nextType == Character.NON_SPACING_MARK.toInt() ||
                nextType == Character.COMBINING_SPACING_MARK.toInt() ||
                nextType == Character.ENCLOSING_MARK.toInt() ||
                nextCp == 0x200D ||
                nextCp in 0xFE00..0xFE0F ||
                nextCp in 0xE0100..0xE01EF
            ) {
                end += Character.charCount(nextCp)
            } else {
                break
            }
        }
        return end
    }

    private fun findParagraphId(text: String, offset: Int): Int {
        var paragraphId = 0
        for (i in 0 until offset.coerceAtMost(text.length)) {
            if (text[i] == '\n') paragraphId++
        }
        return paragraphId
    }

    fun nextRevisionAndIncrement(): Long {
        val rev = nextRevision
        nextRevision++
        return rev
    }
}
