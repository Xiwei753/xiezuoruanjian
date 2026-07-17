package com.xiwei.sujian.editor.v2.render

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.SliceRole

class AndroidRenderFrame(
    val transaction: PreparedVisualTransaction?,
    val progress: Float,
    val viewportWidth: Int,
    val viewportHeight: Int,
    val scrollX: Float,
    val scrollY: Float,
    val cursorUtf16: Int,
    val cursorX: Float,
    val cursorY: Float,
    val cursorHeight: Float,
    val selectionStartUtf16: Int,
    val selectionEndUtf16: Int,
    val compositionStartUtf16: Int,
    val compositionEndUtf16: Int,
    val searchHighlightsUtf16: List<Pair<Int, Int>>
)

class AndroidRenderer {
    private var backgroundColor: Int = Color.WHITE
    private val cursorPaint = Paint().apply {
        color = Color.BLACK
        strokeWidth = 2f
        isAntiAlias = true
    }
    private val selectionPaint = Paint().apply {
        color = Color.argb(51, 0, 0, 255)
        style = Paint.Style.FILL
        isAntiAlias = true
    }
    private val preeditUnderlinePaint = Paint().apply {
        color = Color.BLACK
        strokeWidth = 2f
        isAntiAlias = true
    }
    private val searchHighlightPaint = Paint().apply {
        color = Color.argb(40, 255, 200, 0)
        style = Paint.Style.FILL
        isAntiAlias = true
    }
    private val slicePaint = Paint().apply {
        isAntiAlias = true
    }

    fun draw(
        canvas: Canvas,
        layout: android.text.Layout,
        frame: AndroidRenderFrame
    ) {
        canvas.drawColor(backgroundColor)
        val transaction = frame.transaction
        if (transaction != null && transaction.animatedSlices.isNotEmpty()) {
            renderSearchHighlights(canvas, layout, frame.searchHighlightsUtf16)
            val animatedRegions = computeAnimatedSliceRegions(transaction)
            renderLayoutWithAnimatedHoles(canvas, layout, animatedRegions)
            renderSelectionDecoration(canvas, layout, transaction)
            renderAnimatedSlices(canvas, layout, transaction, frame.progress)
            renderPreeditDecoration(canvas, layout, transaction)
            renderCursorWithTransition(canvas, layout, frame, transaction)
        } else {
            renderSearchHighlights(canvas, layout, frame.searchHighlightsUtf16)
            renderSelectionHighlight(canvas, layout, frame.selectionStartUtf16, frame.selectionEndUtf16)
            layout.draw(canvas)
            renderPreeditUnderline(canvas, layout, frame.compositionStartUtf16, frame.compositionEndUtf16)
            renderCursor(canvas, layout, frame.cursorUtf16, frame.cursorX, frame.cursorY, frame.cursorHeight)
        }
    }

    private fun renderSearchHighlights(
        canvas: Canvas,
        layout: android.text.Layout,
        highlights: List<Pair<Int, Int>>
    ) {
        if (highlights.isEmpty()) return
        for ((startUtf16, endUtf16) in highlights) {
            val startLine = layout.getLineForOffset(startUtf16)
            val endLine = layout.getLineForOffset(endUtf16)
            for (line in startLine..endLine) {
                val lineStart = if (line == startLine) startUtf16 else layout.getLineStart(line)
                val lineEnd = if (line == endLine) endUtf16 else layout.getLineEnd(line)
                val left = layout.getPrimaryHorizontal(lineStart)
                val right = layout.getPrimaryHorizontal(lineEnd - 1)
                val top = layout.getLineTop(line).toFloat()
                val bottom = layout.getLineBottom(line).toFloat()
                canvas.drawRect(left, top, right, bottom, searchHighlightPaint)
            }
        }
    }

    private fun computeAnimatedSliceRegions(
        transaction: PreparedVisualTransaction
    ): List<android.graphics.RectF> {
        val regions = mutableListOf<android.graphics.RectF>()
        for (slice in transaction.animatedSlices) {
            regions.add(slice.destinationRect)
        }
        return regions
    }

    private fun renderLayoutWithAnimatedHoles(
        canvas: Canvas,
        layout: android.text.Layout,
        animatedLineRegions: List<android.graphics.RectF>
    ) {
        if (animatedLineRegions.isEmpty()) {
            layout.draw(canvas)
            return
        }
        canvas.save()
        for (region in animatedLineRegions) {
            canvas.clipOutRect(region.left, region.top, region.right, region.bottom)
        }
        layout.draw(canvas)
        canvas.restore()
    }

    private fun renderAnimatedSlices(canvas: Canvas, layout: android.text.Layout, transaction: PreparedVisualTransaction, progress: Float) {
        for (slice in transaction.animatedSlices) {
            val snapshot = slice.snapshot ?: continue
            val bitmap = snapshot.bitmap ?: continue
            val alpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * progress

            slicePaint.alpha = (alpha * 255).toInt().coerceIn(0, 255)

            when (slice.role) {
                SliceRole.Move -> {
                    val fromRect = slice.fromDestinationRect ?: slice.destinationRect
                    val currentLeft = fromRect.left + (slice.destinationRect.left - fromRect.left) * progress
                    val currentTop = fromRect.top + (slice.destinationRect.top - fromRect.top) * progress
                    val currentRight = fromRect.right + (slice.destinationRect.right - fromRect.right) * progress
                    val currentBottom = fromRect.bottom + (slice.destinationRect.bottom - fromRect.bottom) * progress
                    val currentDest = android.graphics.RectF(currentLeft, currentTop, currentRight, currentBottom)
                    canvas.drawBitmap(bitmap, slice.sourceRect, currentDest, slicePaint)
                }
                else -> {
                    canvas.drawBitmap(bitmap, slice.sourceRect, slice.destinationRect, slicePaint)
                }
            }
        }
    }

    private fun renderCursorTransition(canvas: Canvas, transaction: PreparedVisualTransaction, progress: Float) {
        val ct = transaction.cursorTransition ?: return
        if (!ct.shouldAnimate) return

        val currentX = ct.fromX + (ct.toX - ct.fromX) * progress
        val currentY = ct.fromY + (ct.toY - ct.fromY) * progress
        val currentHeight = ct.fromHeight + (ct.toHeight - ct.fromHeight) * progress

        canvas.drawRect(currentX, currentY, currentX + 2f, currentY + currentHeight, cursorPaint)
    }

    private fun renderCursorWithTransition(
        canvas: Canvas,
        layout: android.text.Layout,
        frame: AndroidRenderFrame,
        transaction: PreparedVisualTransaction
    ) {
        val ct = transaction.cursorTransition
        if (ct != null && ct.shouldAnimate) {
            val progress = frame.progress
            val currentX = ct.fromX + (ct.toX - ct.fromX) * progress
            val currentY = ct.fromY + (ct.toY - ct.fromY) * progress
            val currentHeight = ct.fromHeight + (ct.toHeight - ct.fromHeight) * progress
            canvas.drawRect(currentX, currentY, currentX + 2f, currentY + currentHeight, cursorPaint)
        } else {
            renderCursor(canvas, layout, frame.cursorUtf16, frame.cursorX, frame.cursorY, frame.cursorHeight)
        }
    }

    private fun renderCursor(canvas: Canvas, layout: android.text.Layout, cursorUtf16: Int, cursorX: Float, cursorY: Float, cursorHeight: Float) {
        if (cursorUtf16 < 0) return
        canvas.drawRect(cursorX, cursorY, cursorX + 2f, cursorY + cursorHeight, cursorPaint)
    }

    private fun renderSelectionDecoration(
        canvas: Canvas,
        layout: android.text.Layout,
        transaction: PreparedVisualTransaction
    ) {
        val sel = transaction.selectionDecoration ?: return
        for (rect in sel.rects) {
            canvas.drawRect(rect, selectionPaint)
        }
    }

    private fun renderPreeditDecoration(
        canvas: Canvas,
        layout: android.text.Layout,
        transaction: PreparedVisualTransaction
    ) {
        val preedit = transaction.preeditDecoration ?: return
        val startLine = layout.getLineForOffset(preedit.startUtf16)
        val endLine = layout.getLineForOffset(preedit.endUtf16)
        for (line in startLine..endLine) {
            val lineStart = if (line == startLine) preedit.startUtf16 else layout.getLineStart(line)
            val lineEnd = if (line == endLine) preedit.endUtf16 else layout.getLineEnd(line)
            val startX = layout.getPrimaryHorizontal(lineStart)
            val endX = layout.getPrimaryHorizontal(lineEnd - 1)
            val bottom = layout.getLineBottom(line).toFloat()
            canvas.drawLine(startX, bottom, endX, bottom, preeditUnderlinePaint)
        }
    }

    private fun renderSelectionHighlight(canvas: Canvas, layout: android.text.Layout, selStart: Int, selEnd: Int) {
        if (selStart == selEnd) return

        val startLine = layout.getLineForOffset(selStart)
        val endLine = layout.getLineForOffset(selEnd)
        for (line in startLine..endLine) {
            val lineStart = if (line == startLine) selStart else layout.getLineStart(line)
            val lineEnd = if (line == endLine) selEnd else layout.getLineEnd(line)
            val top = layout.getLineTop(line).toFloat()
            val bottom = layout.getLineBottom(line).toFloat()
            canvas.drawRect(
                layout.getLineLeft(line), top,
                layout.getLineRight(line), bottom,
                selectionPaint
            )
        }
    }

    private fun renderPreeditUnderline(canvas: Canvas, layout: android.text.Layout, compStart: Int, compEnd: Int) {
        if (compStart < 0 || compEnd < 0 || compStart >= compEnd) return
        val startLine = layout.getLineForOffset(compStart)
        val endLine = layout.getLineForOffset(compEnd)
        for (line in startLine..endLine) {
            val lineStart = if (line == startLine) compStart else layout.getLineStart(line)
            val lineEnd = if (line == endLine) compEnd else layout.getLineEnd(line)
            val startX = layout.getPrimaryHorizontal(lineStart)
            val endX = layout.getPrimaryHorizontal(lineEnd - 1)
            val bottom = layout.getLineBottom(line).toFloat()
            canvas.drawLine(startX, bottom, endX, bottom, preeditUnderlinePaint)
        }
    }

    fun setThemeColors(textColor: Int, cursorColor: Int, selectionColor: Int, preeditColor: Int, bgColor: Int = Color.WHITE) {
        cursorPaint.color = cursorColor
        selectionPaint.color = selectionColor
        preeditUnderlinePaint.color = preeditColor
        backgroundColor = bgColor
    }
}
