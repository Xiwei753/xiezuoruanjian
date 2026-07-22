package com.xiwei.sujian.editor.v2.pipeline

import android.graphics.Canvas
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
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
        layoutRevision: AndroidLayoutRevision?,
        transaction: com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction?,
        timelineProgress: Float,
        searchHighlightsUtf16: List<Pair<Int, Int>>,
        viewportWidth: Int,
        viewportHeight: Int,
        scrollX: Float,
        scrollY: Float,
        cursorVisible: Boolean,
        selectionAllowed: Boolean,
        mirror: com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
    ) {
        val effectiveSelStart = if (selectionAllowed) (layoutRevision?.selectionStartUtf16 ?: mirror.getSelectionStartUtf16()) else mirror.getCursorUtf16()
        val effectiveSelEnd = if (selectionAllowed) (layoutRevision?.selectionEndUtf16 ?: mirror.getSelectionEndUtf16()) else mirror.getCursorUtf16()

        val composedFrame = frameComposer.compose(
            layout = layout,
            transaction = transaction,
            progress = timelineProgress,
            cursorUtf16 = if (cursorVisible) (layoutRevision?.cursorUtf16 ?: mirror.getCursorUtf16()) else -1,
            cursorX = layoutRevision?.cursorX ?: 0f,
            cursorY = layoutRevision?.cursorY ?: 0f,
            cursorHeight = layoutRevision?.cursorHeight ?: 0f,
            selectionStartUtf16 = effectiveSelStart,
            selectionEndUtf16 = effectiveSelEnd,
            compositionStartUtf16 = layoutRevision?.compositionStartUtf16 ?: -1,
            compositionEndUtf16 = layoutRevision?.compositionEndUtf16 ?: -1,
            searchHighlightsUtf16 = searchHighlightsUtf16,
            viewportWidth = viewportWidth,
            viewportHeight = viewportHeight,
            scrollX = scrollX,
            scrollY = scrollY
        )

        renderComposedFrame(canvas, composedFrame)
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
