package com.xiwei.sujian.editor.v2.layout

/**
 * Immutable snapshot of the Android Layout state at a point in time.
 *
 * [revisionId] is the layout engine's local monotonic counter, incremented on every
 * [AndroidLayoutEngine.buildRevision] call. [editorRevision] is the Rust EditorKernel's
 * edit revision that this layout was built from. They differ because layout may be
 * rebuilt without a new edit (e.g. width change), and an edit may not change layout.
 *
 * [DisplayTextMirror] is the text truth; [AndroidLayoutRevision] is its visual projection.
 * requestLayout() must be called after mirror updates to produce a consistent revision.
 */
data class AndroidLayoutRevision(
    val revisionId: Long,
    val editorRevision: Long,
    val widthFingerprint: Float,
    val fontFingerprint: String,
    val lineCount: Int,
    val lineRanges: List<LineRange>,
    val cursorUtf8: Int,
    val cursorUtf16: Int,
    val cursorX: Float,
    val cursorY: Float,
    val cursorHeight: Float,
    val selectionAnchorUtf8: Int,
    val selectionHeadUtf8: Int,
    val selectionAnchorUtf16: Int,
    val selectionHeadUtf16: Int,
    val compositionStartUtf16: Int,
    val compositionEndUtf16: Int,
    val snapshotHandles: List<Long>
) {
    val selectionStartUtf8: Int get() = minOf(selectionAnchorUtf8, selectionHeadUtf8)
    val selectionEndUtf8: Int get() = maxOf(selectionAnchorUtf8, selectionHeadUtf8)
    val selectionStartUtf16: Int get() = minOf(selectionAnchorUtf16, selectionHeadUtf16)
    val selectionEndUtf16: Int get() = maxOf(selectionAnchorUtf16, selectionHeadUtf16)

    /** Byte ranges are half-open: [startUtf8, endUtf8). UTF-8 byte offsets. */
    data class LineRange(
        val startUtf8: Int,
        val endUtf8: Int,
        val startUtf16: Int,
        val endUtf16: Int,
        val top: Float,
        val bottom: Float,
        val baseline: Float,
        val left: Float,
        val right: Float,
        /** Whether this visual line ends with a hard `\n` paragraph break.
         *  Set during revision construction by inspecting the source text character at
         *  [endUtf16 - 1], not by detecting byte gaps between adjacent visual lines.
         *  Android Layout's visual line byte ranges are contiguous even across `\n`,
         *  so byte-gap-based detection would never identify a paragraph boundary.
         *  Used by the animation planner to stop reflow scanning — text reflow cannot
         *  propagate into the next paragraph. */
        val endsWithHardBreak: Boolean = false,
        /** Monotonic paragraph identifier. Lines within the same paragraph share the same
         *  [paragraphId]; the first line after a hard break increments it. Used by
         *  [AndroidVisualPlanner.computeAffectedLines] to align old/new lines by paragraph
         *  rather than by global lineIndex — when a paragraph gains or loses visual lines,
         *  global lineIndex alignment would truncate the affected range prematurely.
         *
         *  NOT a stable identity across edits: inserting or deleting a hard break renumbers
         *  all subsequent paragraphs. Paragraph alignment across old/new revisions uses
         *  offset-map matching (via [AndroidVisualPlanner.buildOffsetMapper]) rather than
         *  paragraphId, so the same text paragraph is correctly paired even after hard-break
         *  insertion/deletion. paragraphId is only used for grouping lines within a single
         *  revision. */
        val paragraphId: Int = 0,
        /** Zero-based index of this visual line within its paragraph. The first visual line
         *  of each paragraph has [paragraphLocalLineIndex] = 0. Used alongside [paragraphId]
         *  for old/new line alignment in the animation planner. */
        val paragraphLocalLineIndex: Int = 0
    )
}
