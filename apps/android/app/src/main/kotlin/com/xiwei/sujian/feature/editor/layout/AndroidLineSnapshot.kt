package com.xiwei.sujian.feature.editor.layout

import android.graphics.Bitmap

data class LineClusterSnapshot(
    val clusterId: Long,
    /** Inclusive UTF-8 byte offset in the document. */
    val documentByteStart: Int,
    /** Exclusive UTF-8 byte offset in the document (half-open: [start, end)). */
    val documentByteEndExclusive: Int,
    /** Inclusive UTF-16 offset in the document. */
    val documentUtf16Start: Int,
    /** Exclusive UTF-16 offset in the document (half-open: [start, end)). */
    val documentUtf16EndExclusive: Int,
    val sourceRectInLineImage: android.graphics.Rect,
    val visualRectInDocument: android.graphics.RectF,
    /** Fingerprint of the platform shaping result (glyph IDs, positions, font, direction).
     *  Used to decide Move vs Crossfade: same fingerprint + confident → Move, else Crossfade. */
    val shapingFingerprint: String,
    /** Whether [shapingFingerprint] was built from PositionedGlyphs (API 31+).
     *  On API < 31 the fingerprint uses codepoint types + paint hash, which is less precise;
     *  when either old or new cluster lacks confidence, the pair must use Crossfade instead
     *  of Move to avoid visual glitches from false fingerprint matches. */
    val shapingIdentityConfident: Boolean = false,
)

data class AndroidLineSnapshot(
    val snapshotId: Long,
    val bitmap: Bitmap?,
    val lineIndex: Int,
    /** Whole-line crop region inside [bitmap]: always (0, 0, bitmapWidth, bitmapHeight).
     *  Used for whole-line CrossfadeOld/CrossfadeNew slices when no per-cluster matching
     *  is available (e.g. LineReflow mode when a line has no cluster matches). For
     *  per-cluster animation, [LineClusterSnapshot.sourceRectInLineImage] provides the
     *  sub-region crop for each grapheme cluster within the same Bitmap. Both rects are in
     *  Bitmap pixel coordinates; the corresponding document-coordinate destination is
     *  [destinationRect] (whole-line) or [LineClusterSnapshot.visualRectInDocument] (per-cluster). */
    val sourceRect: android.graphics.Rect,
    /** Whole-line destination rect in document coordinates (no scroll offset).
     *  Uses floating-point [android.graphics.RectF] rather than integer [android.graphics.Rect]
     *  because layout coordinates are sub-pixel — truncating to integers would cause 1px
     *  alignment drift between the snapshot and the live layout. Canvas.drawBitmap handles
     *  pixel snapping at render time via its own anti-aliasing and rounding, so the source
     *  (integer pixel) → destination (float layout) mapping remains geometrically correct. */
    val destinationRect: android.graphics.RectF,
    val clusters: List<LineClusterSnapshot> = emptyList(),
    /** Inclusive UTF-8 byte offset of this line's start in the document. */
    val documentByteStart: Int = 0,
    /** Exclusive UTF-8 byte offset of this line's end in the document. */
    val documentByteEndExclusive: Int = 0,
    /** Inclusive UTF-16 offset of this line's start in the document. */
    val documentUtf16Start: Int = 0,
    /** Exclusive UTF-16 offset of this line's end in the document. */
    val documentUtf16EndExclusive: Int = 0,
    val baseline: Float = 0f,
    val lineHeight: Float = 0f,
)
