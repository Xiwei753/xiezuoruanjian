package com.xiwei.sujian.editor.v2.pipeline

import android.graphics.Canvas
import com.xiwei.sujian.editor.v2.render.ComposedFrame
import com.xiwei.sujian.editor.v2.render.EditorFrameComposer
import com.xiwei.sujian.editor.v2.render.AndroidTextRenderer
import com.xiwei.sujian.editor.v2.render.AndroidTextAnimationRenderer

class AndroidRenderRuntime(
    val textRenderer: AndroidTextRenderer,
    val animationRenderer: AndroidTextAnimationRenderer,
    val frameComposer: EditorFrameComposer
) {
    constructor() : this(
        AndroidTextRenderer(),
        AndroidTextAnimationRenderer(),
        EditorFrameComposer()
    )

    fun drawFrame(
        canvas: Canvas,
        layout: android.text.Layout,
        layoutRuntime: AndroidLayoutRuntime,
        visualRuntime: AndroidVisualRuntime,
        mirror: com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror,
        searchHighlightsUtf16: List<Pair<Int, Int>>,
        viewportWidth: Int,
        viewportHeight: Int,
        scrollX: Float,
        scrollY: Float,
        cursorVisible: Boolean,
        selectionAllowed: Boolean
    ) {
        val frameTimeMs = System.nanoTime() / 1_000_000
        val rev = layoutRuntime.getCurrentRevision()
        val effectiveSelStart = if (selectionAllowed) (rev?.selectionStartUtf16 ?: mirror.getSelectionStartUtf16()) else mirror.getCursorUtf16()
        val effectiveSelEnd = if (selectionAllowed) (rev?.selectionEndUtf16 ?: mirror.getSelectionEndUtf16()) else mirror.getCursorUtf16()

        val transaction = visualRuntime.getActiveTransaction()
        val progress = visualRuntime.getTimelineProgress(frameTimeMs)

        visualRuntime.markFirstVisibleFrame(frameTimeMs)

        val composedFrame = frameComposer.compose(
            layout = layout,
            transaction = transaction,
            progress = progress,
            cursorUtf16 = if (cursorVisible) (rev?.cursorUtf16 ?: mirror.getCursorUtf16()) else -1,
            cursorX = rev?.cursorX ?: 0f,
            cursorY = rev?.cursorY ?: 0f,
            cursorHeight = rev?.cursorHeight ?: 0f,
            selectionStartUtf16 = effectiveSelStart,
            selectionEndUtf16 = effectiveSelEnd,
            compositionStartUtf16 = rev?.compositionStartUtf16 ?: -1,
            compositionEndUtf16 = rev?.compositionEndUtf16 ?: -1,
            searchHighlightsUtf16 = searchHighlightsUtf16,
            viewportWidth = viewportWidth,
            viewportHeight = viewportHeight,
            scrollX = scrollX,
            scrollY = scrollY
        )

        renderComposedFrame(canvas, composedFrame)

        visualRuntime.completeIfFinished(frameTimeMs)
    }

    fun renderComposedFrame(canvas: Canvas, frame: ComposedFrame) {
        val layout = frame.layout ?: return
        val transaction = frame.transaction

        textRenderer.drawBackground(canvas)

        if (transaction != null && (transaction.animatedSlices.isNotEmpty() || transaction.blockShifts.isNotEmpty())) {
            textRenderer.drawSearchHighlights(canvas, layout, frame.searchHighlightsUtf16, frame.blockShifts, frame.progress)
            textRenderer.drawSelectionHighlight(canvas, layout, frame.selectionStartUtf16, frame.selectionEndUtf16, frame.blockShifts, frame.progress)
            val animatedRegions = animationRenderer.computeAnimatedSliceRegions(transaction)
            textRenderer.drawStaticTextWithHoles(canvas, layout, animatedRegions, frame.blockShifts, frame.progress)
            animationRenderer.drawAnimatedSlices(canvas, transaction, frame.progress)
            textRenderer.drawPreeditUnderline(canvas, layout, frame.compositionStartUtf16, frame.compositionEndUtf16, frame.blockShifts, frame.progress)

            val ct = transaction.cursorTransition
            if (ct != null && ct.shouldAnimate) {
                animationRenderer.drawAnimatedCursor(canvas, transaction, frame.progress, textRenderer.getCursorPaint())
            } else {
                textRenderer.drawCursor(canvas, frame.cursorUtf16, frame.cursorX, frame.cursorY, frame.cursorHeight)
            }
        } else {
            textRenderer.drawSearchHighlights(canvas, layout, frame.searchHighlightsUtf16)
            textRenderer.drawSelectionHighlight(canvas, layout, frame.selectionStartUtf16, frame.selectionEndUtf16)
            textRenderer.drawStaticText(canvas, layout)
            textRenderer.drawPreeditUnderline(canvas, layout, frame.compositionStartUtf16, frame.compositionEndUtf16)
            textRenderer.drawCursor(canvas, frame.cursorUtf16, frame.cursorX, frame.cursorY, frame.cursorHeight)
        }
    }

    fun setThemeColors(textColor: Int, cursorColor: Int, selectionColor: Int, preeditColor: Int, bgColor: Int) {
        textRenderer.setThemeColors(textColor, cursorColor, selectionColor, preeditColor, bgColor)
    }
}
