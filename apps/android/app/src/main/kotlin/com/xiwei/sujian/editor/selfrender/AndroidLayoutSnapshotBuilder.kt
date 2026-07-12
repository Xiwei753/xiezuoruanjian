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
        return if (android.os.Build.VERSION.SDK_INT >= 31) {
            buildGlyphIdentityApi31(clusterText, paint, paragraphDirection)
        } else {
            buildConservativeIdentity(clusterText, paint, paragraphDirection)
        }
    }

    /**
     * API 31+ 使用真实 glyph identity。
     *
     * 通过 TextRunShaper.shapeTextRun() 获取 PositionedGlyphs，
     * identity 包含 glyph ids、每个 glyph 的相对 position、
     * font/typeface fingerprint、cluster context range、text direction、
     * Paint shaping fields。
     */
    private fun buildGlyphIdentityApi31(
        clusterText: String,
        paint: TextPaint,
        paragraphDirection: Int
    ): String {
        val isRtl = paragraphDirection == Layout.DIR_RIGHT_TO_LEFT
        val fontFingerprint = "${paint.typeface}:${paint.textSize.toInt()}:${paint.textScaleX.format(2)}:${paint.letterSpacing.format(2)}"

        try {
            val shaped = android.graphics.text.TextRunShaper.shapeTextRun(
                clusterText, 0, clusterText.length,
                0, clusterText.length,
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

            return "g31:$glyphIdBuilder:$posBuilder:$fontFingerprint:$fontBuilder:$isRtl"
        } catch (e: Exception) {
            return buildConservativeIdentity(clusterText, paint, paragraphDirection)
        }
    }

    /**
     * API 24–30 保守 identity。
     *
     * 不伪造 glyphCount = codePointCount。使用完整 shaping context 文本 hash、
     * cluster 文本 hash、字体与 Paint shaping fingerprint、direction、
     * StaticLayout line/context identity、精确 sourceRect/visual width。
     *
     * 只要 context 或断行环境变化，就使用 Crossfade，不冒险 Move。
     */
    private fun buildConservativeIdentity(
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

    fun nextRevisionAndIncrement(): Long {
        val rev = nextRevision
        nextRevision++
        return rev
    }
}
