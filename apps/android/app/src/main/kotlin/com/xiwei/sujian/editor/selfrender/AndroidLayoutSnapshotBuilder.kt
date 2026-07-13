package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import java.util.Objects

/**
 * 从同一个 [Layout]/[StaticLayout] revision 捕获不可变行快照。
 *
 * 这是 Android 文字动画唯一的排版视觉来源。动画帧只消费已录制的
 * [AndroidLineVisualResource]，不会重新调用文字排版。
 *
 * 坐标约定：
 * - 文档范围使用 UTF-8 byte offset，平台范围使用 UTF-16 offset。
 * - [Layout.getPrimaryHorizontal]、line left/top/baseline 产生的坐标
 *   分别变换到行局部（sourceRect）与文档坐标（visualRect）。
 * - revision 只标识同一批布局视觉结果，不是正文版本号的替代品。
 */
class AndroidLayoutSnapshotBuilder(
    private val layout: SujianEditorLayout,
    private val textPaint: TextPaint
) {
    private val TAG = "AndroidSnapshotBuilder"
    private var currentCommittedRevision: Long = 0L
    private var nextRevision: Long = 1L

    fun currentCommittedRevision(): Long = currentCommittedRevision

    fun allocateNextRevision(): Long {
        val rev = nextRevision
        nextRevision++
        return rev
    }

    fun commitRevision(rev: Long) {
        currentCommittedRevision = rev
    }

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

        // UTF-16 → UTF-8 byte offset 的转换边界：
        // Android Layout API 全部使用 UTF-16 offset（getLineStart/End, getPrimaryHorizontal），
        // 跨平台事务和 byte range 使用 UTF-8，必须在此处转换。
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

            val lineLeft = staticLayout.getLineLeft(lineIdx)
            val lineTop = staticLayout.getLineTop(lineIdx)

            val visualLeft = kotlin.math.min(x, nextX)
            val visualRight = kotlin.math.max(x, nextX)
            val visualTop = baseline + ascent
            val visualBottom = baseline + descent

            val visualRect = RectF(
                visualLeft, visualTop,
                visualRight, visualBottom
            )

            val sourceRect = RectF(
                visualLeft - lineLeft,
                visualTop - lineTop,
                visualRight - lineLeft,
                visualBottom - lineTop
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
                shapingIdentity = buildShapingIdentity(clusterText, textPaint, staticLayout.getParagraphDirection(lineIdx), currentOffset, clusterEnd, lineIdx, staticLayout, text)
            ))

            currentOffset = clusterEnd
        }
        return clusters
    }

    private fun buildShapingIdentity(
        clusterText: String,
        paint: TextPaint,
        paragraphDirection: Int,
        clusterStartUtf16: Int,
        clusterEndUtf16: Int,
        lineIdx: Int,
        staticLayout: Layout,
        fullText: String
    ): String {
        return if (android.os.Build.VERSION.SDK_INT >= 31) {
            buildGlyphIdentityApi31(fullText, paint, paragraphDirection, clusterStartUtf16, clusterEndUtf16)
        } else {
            buildConservativeIdentity(clusterText, paint, paragraphDirection, lineIdx, staticLayout, fullText, clusterStartUtf16, clusterEndUtf16)
        }
    }

    private fun buildGlyphIdentityApi31(
        fullText: String,
        paint: TextPaint,
        paragraphDirection: Int,
        clusterStartUtf16: Int,
        clusterEndUtf16: Int
    ): String {
        val isRtl = paragraphDirection == Layout.DIR_RIGHT_TO_LEFT
        val fontFingerprint = "${paint.typeface}:${paint.textSize.toInt()}:${paint.textScaleX.format(2)}:${paint.letterSpacing.format(2)}"
        val clusterLen = clusterEndUtf16 - clusterStartUtf16

        var contextStart = (clusterStartUtf16 - 64).coerceAtLeast(0)
        var contextEnd = (clusterEndUtf16 + 64).coerceAtMost(fullText.length)
        var searchStart = clusterStartUtf16 - 1
        while (searchStart >= contextStart) {
            if (fullText[searchStart] == '\n') {
                contextStart = searchStart + 1
                break
            }
            searchStart--
        }
        var searchEnd = clusterEndUtf16
        while (searchEnd < contextEnd) {
            if (fullText[searchEnd] == '\n') {
                contextEnd = searchEnd
                break
            }
            searchEnd++
        }
        val contextLen = contextEnd - contextStart
        val contextText = fullText.substring(contextStart, contextEnd)
        val contextHash = Objects.hash(contextText)
        val clusterOffsetInContext = clusterStartUtf16 - contextStart

        try {
            val shaped = android.graphics.text.TextRunShaper.shapeTextRun(
                fullText, clusterStartUtf16, clusterLen,
                contextStart, contextLen,
                0f, 0f, isRtl, paint
            )

            val glyphCount = shaped.glyphCount()
            val glyphIdBuilder = StringBuilder()
            val posBuilder = StringBuilder()
            val fontBuilder = StringBuilder()

            for (i in 0 until glyphCount) {
                if (i > 0) {
                    glyphIdBuilder.append(",")
                    posBuilder.append(",")
                    fontBuilder.append("|")
                }
                glyphIdBuilder.append(shaped.getGlyphId(i).toString())
                posBuilder.append(String.format("%.1f,%.1f", shaped.getGlyphX(i), shaped.getGlyphY(i)))
                fontBuilder.append(shaped.getFont(i).toString())
            }

            return "g31:$glyphIdBuilder:$posBuilder:$fontFingerprint:$fontBuilder:$isRtl:ctxH[$contextHash]:cOff[$clusterOffsetInContext,$clusterLen]"
        } catch (e: Exception) {
            val clusterText = fullText.substring(clusterStartUtf16, clusterEndUtf16.coerceAtMost(fullText.length))
            return buildConservativeIdentityFallback(clusterText, paint, paragraphDirection)
        }
    }

    private fun buildConservativeIdentity(
        clusterText: String,
        paint: TextPaint,
        paragraphDirection: Int,
        lineIdx: Int,
        staticLayout: Layout,
        fullText: String,
        clusterStartUtf16: Int,
        clusterEndUtf16: Int
    ): String {
        val contextStart = (clusterStartUtf16 - 32).coerceAtLeast(0)
        val contextEnd = (clusterEndUtf16 + 32).coerceAtMost(fullText.length)
        val contextText = fullText.substring(contextStart, contextEnd)
        val textHash = Objects.hash(clusterText)
        val contextHash = Objects.hash(contextText)
        val fontFingerprint = "${paint.typeface}:${paint.textSize.toInt()}:${paint.textScaleX.format(2)}:${paint.letterSpacing.format(2)}"
        val width = paint.measureText(clusterText).toRawBits()
        val isRtl = paragraphDirection == Layout.DIR_RIGHT_TO_LEFT
        val clusterOffsetInContext = clusterStartUtf16 - contextStart
        val clusterLen = clusterEndUtf16 - clusterStartUtf16
        return "c:$textHash:$contextHash:$fontFingerprint:$width:$isRtl:cOff[$clusterOffsetInContext,$clusterLen]"
    }

    private fun buildConservativeIdentityFallback(
        clusterText: String,
        paint: TextPaint,
        paragraphDirection: Int
    ): String {
        val textHash = Objects.hash(clusterText)
        val fontFingerprint = "${paint.typeface}:${paint.textSize.toInt()}:${paint.textScaleX.format(2)}:${paint.letterSpacing.format(2)}"
        val width = paint.measureText(clusterText).toRawBits()
        val isRtl = paragraphDirection == Layout.DIR_RIGHT_TO_LEFT
        return "c:$textHash:$fontFingerprint:$width:$isRtl"
    }

    private fun Float.format(digits: Int): String = String.format("%.${digits}f", this)

    // BreakIterator 用于 grapheme/cluster 边界，不能用单个 UTF-16 code unit
    // 或 code point 代替：ZWJ emoji、组合字符、ligature 的边界必须由
    // ICU BreakIterator 判定。
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

    @Deprecated("Use allocateNextRevision() + commitRevision() instead", ReplaceWith("allocateNextRevision().also { commitRevision(it) }"))
    fun nextRevisionAndIncrement(): Long {
        val rev = allocateNextRevision()
        commitRevision(rev)
        return rev
    }
}
