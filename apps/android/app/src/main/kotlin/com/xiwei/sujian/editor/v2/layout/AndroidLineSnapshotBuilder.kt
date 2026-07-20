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
            val shapingResult = buildShapingFingerprint(clusterText, layout, lineIndex, clusterStartUtf16)

            clusters.add(LineClusterSnapshot(
                clusterId = clusterIdCounter++,
                documentByteStart = clusterStartUtf8,
                documentByteEndExclusive = clusterEndUtf8,
                documentUtf16Start = clusterStartUtf16,
                documentUtf16EndExclusive = clusterEndUtf16,
                sourceRectInLineImage = android.graphics.Rect(sourceLeft.toInt(), sourceTop.toInt(), sourceRight.toInt(), sourceBottom.toInt()),
                visualRectInDocument = android.graphics.RectF(x0, top, x1, bottom),
                shapingFingerprint = shapingResult.first,
                shapingIdentityConfident = shapingResult.second
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


    /**
     * Build a shaping fingerprint for a grapheme cluster.
     *
     * Returns a pair of (fingerprint string, confident boolean).
     *
     * API 31+: Uses TextRunShaper.shapeText → PositionedGlyphs (glyph IDs, fonts, positions).
     * This is reliable for Move vs Crossfade decisions because PositionedGlyphs captures
     * the actual glyph output including contextual shaping, ligatures, and font fallback.
     * [confident] is true only when the full-line-context shaping path succeeds and extracts
     * matching glyphs. The isolated-shaping fallback and hash fallback both return
     * [confident] = false because they cannot guarantee visual identity — isolated shaping
     * misses contextual forms (Arabic, Indic, ligatures), and the hash fallback is too coarse.
     *
     * API < 31: Falls back to codepoint Unicode categories + paint hash + bidi direction
     * + adjacent codepoint context hash. This is a conservative approximation — different
     * shapings may produce the same fingerprint. [confident] is always false, and the
     * animation planner will use Crossfade instead of Move to avoid visual glitches.
     *
     * When either the old or new cluster has [shapingIdentityConfident] == false, the
     * planner must not assume visual identity even if fingerprints match — Crossfade is
     * the only safe choice.
     */
    private fun buildShapingFingerprint(clusterText: String, layout: Layout, lineIndex: Int, clusterStartUtf16: Int): Pair<String, Boolean> {
        if (clusterText.isEmpty()) return Pair("", true)
        val contextHash = computeContextHash(layout, lineIndex, clusterStartUtf16)
        if (android.os.Build.VERSION.SDK_INT >= 31) {
            return buildShapingFingerprintApi31(clusterText, layout, lineIndex, clusterStartUtf16, contextHash)
        }
        val codePoints = clusterText.codePoints().toArray()
        val typeSummary = codePoints.map { Character.getType(it) }.distinct().sorted().joinToString(",")
        val paintHash = layout.paint.hashCode()
        return Pair("${codePoints.joinToString(",")}_${typeSummary}_${paintHash}_${contextHash}", false)
    }

    private fun computeContextHash(layout: Layout, lineIndex: Int, clusterStartUtf16: Int): Int {
        val lineStart = layout.getLineStart(lineIndex)
        val lineEnd = layout.getLineEnd(lineIndex)
        val text = layout.text
        val prevCodePoint = if (clusterStartUtf16 > lineStart && clusterStartUtf16 <= text.length) {
            Character.codePointBefore(text, clusterStartUtf16)
        } else -1
        val clusterCharCount = if (clusterStartUtf16 < text.length) Character.charCount(Character.codePointAt(text, clusterStartUtf16)) else 1
        val nextOffset = clusterStartUtf16 + clusterCharCount
        val nextCodePoint = if (nextOffset < lineEnd && nextOffset < text.length) {
            Character.codePointAt(text, nextOffset)
        } else -1
        val bidiDir = layout.getParagraphDirection(lineIndex)
        var result = 1
        result = 31 * result + prevCodePoint
        result = 31 * result + nextCodePoint
        result = 31 * result + bidiDir
        return result
    }

    /**
     * API 31+ shaping fingerprint using PositionedGlyphs.
     *
     * Shapes the full line text with context so that contextual shaping (Arabic, Indic,
     * ligatures) produces the same glyphs as the actual layout. Then extracts only the
     * glyphs belonging to the target cluster by matching their X positions against the
     * cluster's expected horizontal span within the line.
     *
     * Returns [confident] = true ONLY when the full-line-context shaping path succeeds
     * and extracts at least one matching glyph. This is the only path where the fingerprint
     * captures the true visual output. Falls back to [shapeClusterInIsolationApi31]
     * (confident = false) or hash-based fingerprint (confident = false) on any failure —
     * these fallbacks cannot guarantee visual identity and must not be used for Move decisions.
     */
    private fun buildShapingFingerprintApi31(clusterText: String, layout: Layout, lineIndex: Int, clusterStartUtf16: Int, contextHash: Int): Pair<String, Boolean> {
        try {
            val paint = layout.paint
            val shaperClass = Class.forName("android.text.TextRunShaper")
            val shapeMethod = shaperClass.getMethod(
                "shapeText",
                CharSequence::class.java,
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                android.text.TextPaint::class.java
            )
            val lineStart = layout.getLineStart(lineIndex)
            val lineEnd = layout.getLineEnd(lineIndex)
            val text = layout.text
            val lineText = text.subSequence(lineStart.coerceAtLeast(0), lineEnd.coerceAtMost(text.length))

            val positionedGlyphs = shapeMethod.invoke(
                null,
                lineText, 0, lineText.length,
                layout.getParagraphDirection(lineIndex),
                paint
            ) ?: return shapeClusterInIsolationApi31(clusterText, layout, lineIndex, contextHash, shapeMethod, paint)

            val glyphCountMethod = positionedGlyphs.javaClass.getMethod("getGlyphCount")
            val glyphCount = glyphCountMethod.invoke(positionedGlyphs) as Int

            val getFontMethod = positionedGlyphs.javaClass.getMethod("getFont", Int::class.javaPrimitiveType)
            val getGlyphIdMethod = positionedGlyphs.javaClass.getMethod("getGlyphId", Int::class.javaPrimitiveType)
            val getXMethod = positionedGlyphs.javaClass.getMethod("getX", Int::class.javaPrimitiveType)
            val getYMethod = positionedGlyphs.javaClass.getMethod("getY", Int::class.javaPrimitiveType)

            val clusterX0 = layout.getPrimaryHorizontal(clusterStartUtf16)
            val clusterX1 = if (clusterStartUtf16 + clusterText.length < lineEnd) {
                layout.getPrimaryHorizontal(clusterStartUtf16 + clusterText.length)
            } else {
                layout.getLineRight(lineIndex)
            }

            val sb = StringBuilder()
            var matchedGlyphCount = 0
            for (i in 0 until glyphCount) {
                val glyphX = getXMethod.invoke(positionedGlyphs, i) as Float
                if (glyphX >= clusterX0 - 0.5f && glyphX < clusterX1 + 0.5f) {
                    if (matchedGlyphCount > 0) sb.append("|")
                    val font = getFontMethod.invoke(positionedGlyphs, i)
                    sb.append(font?.hashCode()?.toString() ?: "null")
                    sb.append("_")
                    sb.append(getGlyphIdMethod.invoke(positionedGlyphs, i))
                    sb.append("_")
                    sb.append(glyphX.toInt())
                    sb.append("_")
                    sb.append((getYMethod.invoke(positionedGlyphs, i) as Float).toInt())
                    matchedGlyphCount++
                }
            }

            if (matchedGlyphCount == 0) {
                return shapeClusterInIsolationApi31(clusterText, layout, lineIndex, contextHash, shapeMethod, paint)
            }

            sb.append("_ctx_")
            sb.append(contextHash)
            return Pair(sb.toString(), true)
        } catch (_: Exception) {
            return Pair("${clusterText.hashCode()}_${contextHash}", false)
        }
    }

    /**
     * Fallback when full-line-context shaping fails or glyph extraction matches zero glyphs.
     *
     * Shapes the cluster text alone (without surrounding line context), producing a fingerprint
     * that captures glyph IDs and positions but may miss contextual shaping effects. Returns
     * [confident] = false because isolated shaping cannot reproduce context-dependent forms:
     * Arabic initial/medial/final variants, Indic conjuncts, and ligatures (fi, fl, etc.)
     * all depend on adjacent characters. A false Move (same isolated fingerprint but different
     * actual rendering) would cause visual glitches, so the planner must use Crossfade instead.
     */
    private fun shapeClusterInIsolationApi31(
        clusterText: String,
        layout: Layout,
        lineIndex: Int,
        contextHash: Int,
        shapeMethod: java.lang.reflect.Method,
        paint: android.text.TextPaint
    ): Pair<String, Boolean> {
        try {
            val positionedGlyphs = shapeMethod.invoke(
                null,
                clusterText, 0, clusterText.length,
                layout.getParagraphDirection(lineIndex),
                paint
            ) ?: return Pair("${clusterText.hashCode()}_${contextHash}", false)

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
            sb.append("_isolated_ctx_")
            sb.append(contextHash)
            return Pair(sb.toString(), false)
        } catch (_: Exception) {
            return Pair("${clusterText.hashCode()}_${contextHash}", false)
        }
    }
}
