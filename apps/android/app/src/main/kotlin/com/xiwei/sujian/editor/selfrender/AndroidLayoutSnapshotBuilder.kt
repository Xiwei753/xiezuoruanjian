package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import java.util.Objects

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
            (lineBottom - lineTop).toFloat().coerceAtLeast(0f)
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
                shapingIdentity = buildShapingIdentity(clusterText, textPaint, staticLayout.getParagraphDirection(lineIdx))
            ))

            currentOffset = clusterEnd
        }
        return clusters
    }

    private fun buildShapingIdentity(
        clusterText: String,
        paint: TextPaint,
        paragraphDirection: Int
    ): String {
        val textHash = Objects.hash(clusterText)
        val fontFingerprint = "${paint.typeface}:${paint.textSize.toInt()}:${paint.textScaleX.format(2)}:${paint.letterSpacing.format(2)}"
        val width = paint.measureText(clusterText).toRawBits()
        val glyphCount = clusterText.codePointCount(0, clusterText.length)
        val isRtl = paragraphDirection == Layout.DIR_RIGHT_TO_LEFT
        return "$textHash:$fontFingerprint:$width:$glyphCount:$isRtl"
    }

    private fun Float.format(digits: Int): String = String.format("%.${digits}f", this)

    private fun findClusterBoundary(text: String, start: Int, lineEnd: Int): Int {
        if (start >= text.length) return start
        val breaker = android.icu.text.BreakIterator.getCharacterInstance()
        breaker.setText(text)
        val boundary = breaker.following(start)
        if (boundary == android.icu.text.BreakIterator.DONE || boundary > lineEnd) {
            val codePoint = text.codePointAt(start)
            return (start + Character.charCount(codePoint)).coerceAtMost(lineEnd.coerceAtMost(text.length))
        }
        return boundary.coerceAtMost(lineEnd.coerceAtMost(text.length))
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
