package com.xiwei.sujian.editor.v2.render

import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction

class EditorFrameComposer {
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
            scrollY = scrollY
        )
    }
}

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
    val scrollY: Float
)
