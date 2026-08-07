package com.xiwei.sujian.editor.v2.layout

import android.graphics.Bitmap
import android.graphics.Canvas
import android.icu.text.BreakIterator
import android.text.Layout
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.projection.DisplayTextProjection

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
        endIndex: Int,
        projection: DisplayTextProjection? = null,
    ): List<AndroidLineSnapshot> {
        if (layout == null || revision == null) return emptyList()

        val snapshots = mutableListOf<AndroidLineSnapshot>()
        val safeStart = startIndex.coerceAtLeast(0)
        val safeEnd = endIndex.coerceAtMost(layout.lineCount)

        for (i in safeStart until safeEnd) {
            val snapshot = buildSnapshotForLine(layout, i, revision, projection)
            if (snapshot != null) {
                snapshots.add(snapshot)
            }
        }
        return snapshots
    }

    fun buildSnapshotForLine(
        layout: Layout?,
        lineIndex: Int,
        revision: AndroidLayoutRevision?,
        projection: DisplayTextProjection? = null,
    ): AndroidLineSnapshot? {
        if (layout == null || revision == null) return null
        if (lineIndex < 0 || lineIndex >= layout.lineCount) return null

        val lineRange = revision.lineRanges.getOrNull(lineIndex) ?: return null

        val left = lineRange.left
        val right = lineRange.right
        val top = lineRange.top
        val bottom = lineRange.bottom

        val width = kotlin.math.ceil(right - left).toInt().coerceAtLeast(1)
        val height = kotlin.math.ceil(bottom - top).toInt().coerceAtLeast(1)

        // Bitmap dimensions use ceil() to ensure the bitmap covers the full sub-pixel
        // extent of the line. Source rects (in buildClustersForLine) use floor/ceil and
        // clamp to [0, bitmapWidth/Height], so ceil guarantees no cluster sourceRect
        // overflows the bitmap even when line width/height has a fractional part.
        // Geometric consistency: Bitmap = ceil(right-left) × ceil(bottom-top);
        // sourceRect = floor(left) to ceil(right) clamped to [0, bitmapSize];
        // destinationRect = exact floating-point layout coordinates. This three-layer
        // convention ensures that (a) the Bitmap is never smaller than any sourceRect,
        // (b) sourceRect pixel coordinates never exceed Bitmap dimensions, and
        // (c) the rendering canvas maps sourceRect pixels to destinationRect layout coords.
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        // Translate so that document coordinate (left, top) maps to Bitmap pixel (0, 0).
        // layout.draw() uses document coordinates, so this shift ensures the line's content
        // lands inside the Bitmap rather than at a large positive offset.
        canvas.translate(-left, -top)
        // clipRect is in the post-translate coordinate system, so (left, top, right, bottom)
        // maps to Bitmap pixels (0, 0, right-left, bottom-top) — the full Bitmap area.
        // This clip prevents layout.draw() from rendering outside the line's bounds (e.g.
        // when a trailing space or line spacing extends beyond the measured line rect).
        canvas.clipRect(left, top, right, bottom)
        layout.draw(canvas)

        val snapshotId = nextSnapshotId()

        val lineStartRealUtf16 =
            if (projection != null) {
                projection.displayUtf16ToRealUtf16(lineRange.startUtf16)
            } else {
                lineRange.startUtf16
            }
        val lineEndRealUtf16 =
            if (projection != null) {
                projection.displayUtf16ToRealUtf16(lineRange.endUtf16)
            } else {
                lineRange.endUtf16
            }

        return AndroidLineSnapshot(
            snapshotId = snapshotId,
            bitmap = bitmap,
            lineIndex = lineIndex,
            sourceRect = android.graphics.Rect(0, 0, width, height),
            destinationRect = android.graphics.RectF(left, top, right, bottom),
            clusters = emptyList(),
            documentByteStart = lineRange.startUtf8,
            documentByteEndExclusive = lineRange.endUtf8,
            documentUtf16Start = lineStartRealUtf16,
            documentUtf16EndExclusive = lineEndRealUtf16,
            baseline = lineRange.baseline,
            lineHeight = bottom - top,
        )
    }

    /**
     * Build a line snapshot including per-cluster geometry and shaping fingerprints.
     *
     * Cluster data (sourceRectInLineImage, visualRectInDocument, shapingFingerprint,
     * shapingIdentityConfident) is required by the animation planner to generate
     * Insert/Delete/Move/Crossfade slices at cluster granularity. Without cluster data,
     * the planner can only produce whole-line crossfade, which is visually coarse for
     * mid-paragraph edits where only a few characters change.
     *
     * The returned snapshot's [clusters] list is ordered by document position within the line.
     * Each cluster represents one grapheme cluster (Unicode boundary), which is the smallest
     * animation unit — splitting below grapheme level would break combining marks and ligatures.
     *
     * Design rationale for grapheme-cluster granularity: finer granularity (individual
     * codepoints) would animate combining marks separately from their base characters,
     * producing visual artifacts (e.g. an accent fading in after its letter). Coarser
     * granularity (word or line) would prevent per-character Move/Crossfade decisions,
     * causing unnecessary whole-line crossfade for single-character edits. Grapheme
     * clusters are the natural visual unit that users perceive as "one character".
     */
    fun buildSnapshotForLineWithClusters(
        layout: Layout?,
        lineIndex: Int,
        revision: AndroidLayoutRevision?,
        mirror: DisplayTextMirror,
        projection: DisplayTextProjection? = null,
    ): AndroidLineSnapshot? {
        val snapshot = buildSnapshotForLine(layout, lineIndex, revision, projection) ?: return null
        if (layout == null) return snapshot

        val lineRange = revision?.lineRanges?.getOrNull(lineIndex) ?: return snapshot
        val clusters = buildClustersForLine(layout, lineIndex, lineRange, mirror, projection)

        return snapshot.copy(clusters = clusters)
    }

    private fun buildClustersForLine(
        layout: Layout,
        lineIndex: Int,
        lineRange: AndroidLayoutRevision.LineRange,
        mirror: DisplayTextMirror,
        projection: DisplayTextProjection? = null,
    ): List<LineClusterSnapshot> {
        val clusters = mutableListOf<LineClusterSnapshot>()
        val text = mirror.getText()
        val effectiveProjection = projection ?: DisplayTextProjection.identity(text)

        val lineStartDisplayUtf16 = layout.getLineStart(lineIndex)
        val lineEndDisplayUtf16 = layout.getLineEnd(lineIndex)

        val lineStartRealUtf16 = effectiveProjection.displayUtf16ToRealUtf16(lineStartDisplayUtf16)
        val lineEndRealUtf16 = effectiveProjection.displayUtf16ToRealUtf16(lineEndDisplayUtf16)

        val safeLineStart = lineStartRealUtf16.coerceIn(0, text.length)
        val safeLineEnd = lineEndRealUtf16.coerceIn(safeLineStart, text.length)
        val lineText = text.substring(safeLineStart, safeLineEnd)

        val graphemeRanges = computeGraphemeRanges(lineText)

        var clusterIdCounter = 0L

        for ((start, end) in graphemeRanges) {
            val clusterStartRealUtf16 = lineStartRealUtf16 + start
            val clusterEndRealUtf16 = lineStartRealUtf16 + end

            val clusterStartDisplayUtf16 = effectiveProjection.realUtf16ToDisplayUtf16(clusterStartRealUtf16)
            val clusterEndDisplayUtf16 = effectiveProjection.realUtf16ToDisplayUtf16(clusterEndRealUtf16)

            val clusterStartUtf8 = effectiveProjection.realUtf16ToRealUtf8(clusterStartRealUtf16)
            val clusterEndUtf8 = effectiveProjection.realUtf16ToRealUtf8(clusterEndRealUtf16)

            val x0 = layout.getPrimaryHorizontal(clusterStartDisplayUtf16)
            val x1 =
                if (clusterEndDisplayUtf16 < layout.getLineEnd(lineIndex)) {
                    layout.getPrimaryHorizontal(clusterEndDisplayUtf16)
                } else {
                    layout.getLineRight(lineIndex)
                }

            // RTL normalization: getPrimaryHorizontal(clusterEndUtf16) can be less than
            // getPrimaryHorizontal(clusterStartUtf16) in RTL text. Using min/max ensures
            // visualLeft <= visualRight and sourceLeft <= sourceRight, preventing zero-width
            // or inverted rects that would cause the animation renderer to skip the slice.
            // Without this normalization, RTL clusters would produce sourceRect.right ==
            // sourceRect.left (zero width) because coerceAtLeast(sourceLeft) would collapse
            // the rect, and the renderer silently skips zero-width slices.
            val visualLeft = kotlin.math.min(x0, x1)
            val visualRight = kotlin.math.max(x0, x1)

            val top = layout.getLineTop(lineIndex).toFloat()
            val bottom = layout.getLineBottom(lineIndex).toFloat()

            val sourceLeft = (visualLeft - lineRange.left).coerceAtLeast(0f)
            // coerceAtLeast(sourceLeft) ensures sourceRight >= sourceLeft after RTL
            // normalization. This is a pre-condition for the floor/ceil/clamp logic below:
            // sourceRectRight is coerced to at least sourceRectLeft + 1 (minimum 1px width),
            // which requires sourceRight >= sourceLeft. Without this guarantee, a negative
            // sourceRight - sourceLeft would produce sourceRectLeft > sourceRectRight after
            // floor/ceil, violating the minimum-width invariant.
            val sourceRight = (visualRight - lineRange.left).coerceAtLeast(sourceLeft)
            val sourceTop = 0f
            val sourceBottom = bottom - top

            val bitmapWidth = kotlin.math.ceil(lineRange.right - lineRange.left).toInt().coerceAtLeast(1)
            val bitmapHeight = kotlin.math.ceil(lineRange.bottom - lineRange.top).toInt().coerceAtLeast(1)

            // Source rect geometry: floor for left/top, ceil for right/bottom, then clamp
            // to [0, bitmapWidth/Height]. This ensures:
            // 1. The source rect covers the full visual extent of the cluster (ceil rounds up
            //    the sub-pixel boundary that floor might miss).
            // 2. No sourceRect coordinate exceeds the Bitmap dimensions (which use ceil),
            //    preventing Canvas.drawBitmap from reading out-of-bounds pixels.
            // 3. sourceRectLeft is at least sourceRectLeft + 1 (minimum 1px width) so that
            //    zero-width clusters (e.g. RTL boundary edge cases) are not silently dropped.
            // Clamping left/top to dimension - 1 keeps the coerceIn range for right/bottom
            // non-empty when a cluster sits at the exact right/bottom edge.
            val sourceRectLeft = kotlin.math.floor(sourceLeft).toInt().coerceIn(0, (bitmapWidth - 1).coerceAtLeast(0))
            val sourceRectTop = kotlin.math.floor(sourceTop).toInt().coerceIn(0, (bitmapHeight - 1).coerceAtLeast(0))
            val sourceRectRight = kotlin.math.ceil(sourceRight).toInt().coerceIn(sourceRectLeft + 1, bitmapWidth)
            val sourceRectBottom = kotlin.math.ceil(sourceBottom).toInt().coerceIn(sourceRectTop + 1, bitmapHeight)

            val localStart = start.coerceIn(0, lineText.length)
            val localEnd = end.coerceIn(0, lineText.length)
            val clusterText = lineText.substring(localStart, localEnd)
            val shapingResult = buildShapingFingerprint(clusterText, layout, lineIndex, clusterStartDisplayUtf16)

            clusters.add(
                LineClusterSnapshot(
                    clusterId = clusterIdCounter++,
                    documentByteStart = clusterStartUtf8,
                    documentByteEndExclusive = clusterEndUtf8,
                    documentUtf16Start = clusterStartRealUtf16,
                    documentUtf16EndExclusive = clusterEndRealUtf16,
                    sourceRectInLineImage =
                        android.graphics.Rect(
                            sourceRectLeft,
                            sourceRectTop,
                            sourceRectRight,
                            sourceRectBottom,
                        ),
                    visualRectInDocument = android.graphics.RectF(visualLeft, top, visualRight, bottom),
                    shapingFingerprint = shapingResult.first,
                    shapingIdentityConfident = shapingResult.second,
                ),
            )
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
     * API 31+: Uses TextRunShaper.shapeTextRun → PositionedGlyphs (glyph IDs, fonts).
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
    private fun buildShapingFingerprint(
        clusterText: String,
        layout: Layout,
        lineIndex: Int,
        clusterStartUtf16: Int,
    ): Pair<String, Boolean> {
        if (clusterText.isEmpty()) return Pair("", true)
        val contextHash = computeContextHash(layout, lineIndex, clusterStartUtf16)
        if (android.os.Build.VERSION.SDK_INT >= 31) {
            return buildShapingFingerprintApi31(clusterText, layout, lineIndex, clusterStartUtf16, contextHash)
        }
        val codePoints = clusterText.codePoints().toArray()
        val typeSummary = codePoints.map { Character.getType(it) }.distinct().sorted().joinToString(",")
        val paintHash = layout.paint.hashCode()
        return Pair("${codePoints.joinToString(",")}_${typeSummary}_${paintHash}_$contextHash", false)
    }

    /**
     * Compute a hash of the shaping context around a cluster for fingerprinting.
     *
     * Includes the bidi direction of the cluster itself via [Layout.isRtlCharAt],
     * NOT the paragraph direction ([Layout.getParagraphDirection]). This distinction
     * is critical for mixed-direction lines: an LTR paragraph with embedded Arabic
     * has paragraph direction LTR but the Arabic cluster's direction is RTL. Using
     * the paragraph direction would cause different bidi runs to share the same
     * direction hash, producing false fingerprint matches and incorrect Move slices.
     */
    private fun computeContextHash(
        layout: Layout,
        lineIndex: Int,
        clusterStartUtf16: Int,
    ): Int {
        val lineStart = layout.getLineStart(lineIndex)
        val lineEnd = layout.getLineEnd(lineIndex)
        val text = layout.text
        val prevCodePoint =
            if (clusterStartUtf16 > lineStart && clusterStartUtf16 <= text.length) {
                Character.codePointBefore(text, clusterStartUtf16)
            } else {
                -1
            }
        val clusterCharCount =
            if (clusterStartUtf16 < text.length) {
                Character.charCount(
                    Character.codePointAt(text, clusterStartUtf16),
                )
            } else {
                1
            }
        val nextOffset = clusterStartUtf16 + clusterCharCount
        val nextCodePoint =
            if (nextOffset < lineEnd && nextOffset < text.length) {
                Character.codePointAt(text, nextOffset)
            } else {
                -1
            }
        val bidiDir =
            if (clusterStartUtf16 < text.length) {
                if (layout.isRtlCharAt(clusterStartUtf16)) 1 else 0
            } else {
                layout.getParagraphDirection(lineIndex)
            }
        var result = 1
        result = 31 * result + prevCodePoint
        result = 31 * result + nextCodePoint
        result = 31 * result + bidiDir
        return result
    }

    /**
     * API 31+ shaping fingerprint using PositionedGlyphs.
     *
     * Calls the public API [android.graphics.text.TextRunShaper.shapeTextRun] directly
     * with context limited to the cluster's bidi run. The target range is the cluster;
     * the context range is the contiguous bidi run that contains the cluster (scanned via
     * [Layout.isRtlCharAt]). This ensures correct contextual shaping for mixed-direction
     * lines (e.g. LTR paragraph with embedded Arabic) — shaping with full-line context
     * but the wrong direction would produce incorrect glyphs.
     *
     * [isRtl] is taken from [Layout.isRtlCharAt] (per-cluster bidi direction), not
     * [Layout.getParagraphDirection] (paragraph-level direction). In a mixed bidi line,
     * the paragraph direction is constant but individual runs alternate — using the
     * paragraph direction would shape Arabic text with LTR direction or vice versa,
     * producing incorrect glyphs while still returning confident=true.
     *
     * Returns [confident] = true when the shaping path succeeds and produces at least
     * one glyph AND the context is correctly bounded to the bidi run. Falls back to
     * hash-based fingerprint (confident = false) on any failure.
     */
    private fun buildShapingFingerprintApi31(
        clusterText: String,
        layout: Layout,
        lineIndex: Int,
        clusterStartUtf16: Int,
        contextHash: Int,
    ): Pair<String, Boolean> {
        if (android.os.Build.VERSION.SDK_INT < 31) {
            return Pair("${clusterText.hashCode()}_$contextHash", false)
        }
        try {
            val paint = layout.paint
            val lineStart = layout.getLineStart(lineIndex)
            val lineEnd = layout.getLineEnd(lineIndex)
            val text = layout.text
            val lineText = text.subSequence(lineStart.coerceAtLeast(0), lineEnd.coerceAtMost(text.length))

            val clusterLocalStart = (clusterStartUtf16 - lineStart).coerceIn(0, lineText.length)
            val clusterCount = clusterText.length.coerceIn(0, lineText.length - clusterLocalStart)
            val isRtl =
                if (clusterStartUtf16 < text.length) {
                    layout.isRtlCharAt(clusterStartUtf16)
                } else {
                    layout.getParagraphDirection(lineIndex) == Layout.DIR_RIGHT_TO_LEFT
                }

            val (bidiRunLocalStart, bidiRunLocalEnd) =
                findBidiRunBounds(
                    layout,
                    lineIndex,
                    clusterStartUtf16,
                    lineStart,
                    lineEnd,
                )

            val positionedGlyphs =
                android.graphics.text.TextRunShaper.shapeTextRun(
                    lineText,
                    clusterLocalStart,
                    clusterCount,
                    bidiRunLocalStart,
                    bidiRunLocalEnd - bidiRunLocalStart,
                    0f,
                    0f,
                    isRtl,
                    paint,
                )

            val glyphCount = positionedGlyphs.glyphCount()
            if (glyphCount == 0) {
                return Pair("${clusterText.hashCode()}_$contextHash", false)
            }

            val sb = StringBuilder()
            for (i in 0 until glyphCount) {
                if (i > 0) sb.append("|")
                val font = positionedGlyphs.getFont(i)
                sb.append(font.hashCode().toString())
                sb.append("_")
                sb.append(positionedGlyphs.getGlyphId(i))
            }

            sb.append("_ctx_")
            sb.append(contextHash)
            return Pair(sb.toString(), true)
        } catch (_: Exception) {
            return Pair("${clusterText.hashCode()}_$contextHash", false)
        }
    }

    /**
     * Find the UTF-16 bounds of the bidi run containing [clusterStartUtf16] within the
     * given visual line. Scans forward and backward from the cluster using
     * [Layout.isRtlCharAt] to find where the text direction changes. Returns local
     * offsets relative to [lineStart].
     *
     * This limits the shaping context to the actual bidi run, so that clusters in a
     * mixed-direction line (e.g. LTR paragraph with embedded Arabic) are shaped with
     * the correct context and direction, enabling [shapingIdentityConfident] = true
     * even in mixed bidi lines.
     *
     * Uses [Layout.isRtlCharAt] (per-character bidi direction), not
     * [Layout.getParagraphDirection] (paragraph-level direction). In a mixed bidi line,
     * the paragraph direction is constant but individual runs alternate — using the
     * paragraph direction would merge adjacent opposite-direction runs into one context,
     * producing incorrect shaping and false [shapingIdentityConfident] = true.
     *
     * Scan termination: the scan stops at direction changes (isRtlCharAt returns a
     * different value), not at the line boundary. This is correct because a single
     * visual line can contain multiple bidi runs — the shaping context must not cross
     * a direction boundary, as that would cause TextRunShaper to produce incorrect
     * glyph positions for the target cluster.
     */
    private fun findBidiRunBounds(
        layout: Layout,
        lineIndex: Int,
        clusterStartUtf16: Int,
        lineStart: Int,
        lineEnd: Int,
    ): Pair<Int, Int> {
        val text = layout.text
        val isRtl =
            if (clusterStartUtf16 < text.length) {
                layout.isRtlCharAt(clusterStartUtf16)
            } else {
                layout.getParagraphDirection(lineIndex) == Layout.DIR_RIGHT_TO_LEFT
            }
        var runStart = clusterStartUtf16
        while (runStart > lineStart) {
            val prev = runStart - 1
            if (prev < text.length && layout.isRtlCharAt(prev) != isRtl) break
            runStart = prev
        }
        var runEnd = clusterStartUtf16 + 1
        while (runEnd < lineEnd && runEnd < text.length) {
            if (layout.isRtlCharAt(runEnd) != isRtl) break
            runEnd++
        }
        return Pair(runStart - lineStart, runEnd - lineStart)
    }
}
