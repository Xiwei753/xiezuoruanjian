package com.xiwei.sujian.editor.v2.layout

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
        val endsWithHardBreak: Boolean = false
    )
}
