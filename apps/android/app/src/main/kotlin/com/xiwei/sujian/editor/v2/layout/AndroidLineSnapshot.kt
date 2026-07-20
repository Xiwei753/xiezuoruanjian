package com.xiwei.sujian.editor.v2.layout

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
    val shapingIdentityConfident: Boolean = true
)

data class AndroidLineSnapshot(
    val snapshotId: Long,
    val bitmap: Bitmap?,
    val lineIndex: Int,
    val sourceRect: android.graphics.Rect,
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
    val lineHeight: Float = 0f
)
