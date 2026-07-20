package com.xiwei.sujian.editor.v2.render

import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction

/**
 * Assembles a [ComposedFrame] from layout, animation transaction, progress, and
 * decoration state (cursor, selection, composition, search highlights, viewport).
 * The composed frame is then passed to the renderers for drawing.
 *
 * Rendering pipeline:
 * 1. [AndroidTextRenderer.drawStaticTextWithHoles] draws the base static text, clipping
 *    out animated-slice holes and block-shift regions.
 * 2. [AndroidTextRenderer.drawStaticTextWithHoles] re-draws each block-shifted region
 *    with interpolated Y translation (merged adjacent paragraphs share one draw call).
 * 3. [AndroidTextAnimationRenderer.drawAnimatedSlices] draws animated slices on top.
 * 4. [AndroidTextAnimationRenderer.drawAnimatedCursor] draws the animated cursor.
 *
 * [blockShifts] originate from [AndroidVisualPlanner.computeAffectedLines] — they
 * represent paragraphs after the edit paragraph whose Y geometry shifted but whose
 * text content is identical.
 */
class EditorFrameComposer {
    /**
     * Combine layout, animation, and decoration state into a single frame descriptor.
     * Cursor/selection/composition offsets use UTF-16 (Android Layout convention);
     * animation byte ranges use UTF-8 half-open intervals.
     */
    fun compose(
        layout: android.text.Layout?,
        transaction: PreparedVisualTransaction?,
        progress: Float,
        cursorUtf16: Int,
        cursorX: Float,
        cursorY: Float,
        cursorHeight: Float,
        selectionStartUtf16: Int,
        selectionEndUtf16: Int,
        compositionStartUtf16: Int,
        compositionEndUtf16: Int,
        searchHighlightsUtf16: List<Pair<Int, Int>>,
        viewportWidth: Int,
        viewportHeight: Int,
        scrollX: Float,
        scrollY: Float
    ): ComposedFrame {
        return ComposedFrame(
            layout = layout,
            transaction = transaction,
            progress = progress,
            cursorUtf16 = cursorUtf16,
            cursorX = cursorX,
            cursorY = cursorY,
            cursorHeight = cursorHeight,
            selectionStartUtf16 = selectionStartUtf16,
            selectionEndUtf16 = selectionEndUtf16,
            compositionStartUtf16 = compositionStartUtf16,
            compositionEndUtf16 = compositionEndUtf16,
            searchHighlightsUtf16 = searchHighlightsUtf16,
            viewportWidth = viewportWidth,
            viewportHeight = viewportHeight,
            scrollX = scrollX,
            scrollY = scrollY,
            blockShifts = transaction?.blockShifts ?: emptyList()
        )
    }
}

/**
 * Immutable frame descriptor consumed by the renderers.
 * UTF-16 offsets are for Android Layout API; animation internals use UTF-8.
 */
data class ComposedFrame(
    val layout: android.text.Layout?,
    val transaction: PreparedVisualTransaction?,
    val progress: Float,
    val cursorUtf16: Int,
    val cursorX: Float,
    val cursorY: Float,
    val cursorHeight: Float,
    val selectionStartUtf16: Int,
    val selectionEndUtf16: Int,
    val compositionStartUtf16: Int,
    val compositionEndUtf16: Int,
    val searchHighlightsUtf16: List<Pair<Int, Int>>,
    val viewportWidth: Int,
    val viewportHeight: Int,
    val scrollX: Float,
    val scrollY: Float,
    val blockShifts: List<PreparedVisualTransaction.BlockShift> = emptyList()
)
