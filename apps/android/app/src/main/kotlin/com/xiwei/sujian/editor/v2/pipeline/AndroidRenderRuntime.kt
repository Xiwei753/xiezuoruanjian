package com.xiwei.sujian.editor.v2.pipeline

import android.graphics.Canvas
import com.xiwei.sujian.editor.v2.render.ComposedFrame
import com.xiwei.sujian.editor.v2.render.EditorFrameComposer
import com.xiwei.sujian.editor.v2.render.AndroidTextRenderer
import com.xiwei.sujian.editor.v2.render.AndroidTextAnimationRenderer

class AndroidRenderRuntime(
    private val textRenderer: AndroidTextRenderer,
    private val animationRenderer: AndroidTextAnimationRenderer,
    private val frameComposer: EditorFrameComposer
) {
    constructor() : this(
        AndroidTextRenderer(),
        AndroidTextAnimationRenderer(),
        EditorFrameComposer()
    )

    fun drawFromFrameState(canvas: Canvas, frameState: FrameState) {
        val input = frameState.renderInput
        val effectiveSelStart = if (input.selectionAllowed) (input.layoutRevision?.selectionStartUtf16 ?: input.mirror.getSelectionStartUtf16()) else input.mirror.getCursorUtf16()
        val effectiveSelEnd = if (input.selectionAllowed) (input.layoutRevision?.selectionEndUtf16 ?: input.mirror.getSelectionEndUtf16()) else input.mirror.getCursorUtf16()

        val composedFrame = frameComposer.compose(
            layout = input.layout,
            transaction = input.transaction,
            progress = input.timelineProgress,
            cursorUtf16 = if (input.cursorVisible) (input.layoutRevision?.cursorUtf16 ?: input.mirror.getCursorUtf16()) else -1,
            cursorX = input.layoutRevision?.cursorX ?: 0f,
            cursorY = input.layoutRevision?.cursorY ?: 0f,
            cursorHeight = input.layoutRevision?.cursorHeight ?: 0f,
            selectionStartUtf16 = effectiveSelStart,
            selectionEndUtf16 = effectiveSelEnd,
            compositionStartUtf16 = input.layoutRevision?.compositionStartUtf16 ?: -1,
            compositionEndUtf16 = input.layoutRevision?.compositionEndUtf16 ?: -1,
            searchHighlightsUtf16 = input.searchHighlightsUtf16,
            viewportWidth = input.viewportWidth,
            viewportHeight = input.viewportHeight,
            scrollX = input.scrollX,
            scrollY = input.scrollY
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
