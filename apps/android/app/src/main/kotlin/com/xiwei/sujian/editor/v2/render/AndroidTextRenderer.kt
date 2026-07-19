package com.xiwei.sujian.editor.v2.render

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint

class AndroidTextRenderer {
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

    fun drawBackground(canvas: Canvas) {
        canvas.drawColor(backgroundColor)
    }

    fun drawSearchHighlights(canvas: Canvas, layout: android.text.Layout, highlights: List<Pair<Int, Int>>) {
        if (highlights.isEmpty()) return
        for ((startUtf16, endUtf16) in highlights) {
            val startLine = layout.getLineForOffset(startUtf16)
            val endLine = layout.getLineForOffset(endUtf16)
            for (line in startLine..endLine) {
                val hlStart = if (line == startLine) startUtf16 else layout.getLineStart(line)
                val hlEnd = if (line == endLine) endUtf16 else layout.getLineEnd(line)
                val left = layout.getPrimaryHorizontal(hlStart)
                val right = layout.getPrimaryHorizontal(hlEnd - 1)
                val top = layout.getLineTop(line).toFloat()
                val bottom = layout.getLineBottom(line).toFloat()
                canvas.drawRect(left, top, right, bottom, searchHighlightPaint)
            }
        }
    }

    fun drawSelectionHighlight(canvas: Canvas, layout: android.text.Layout, selStart: Int, selEnd: Int) {
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

    fun drawStaticText(canvas: Canvas, layout: android.text.Layout) {
        layout.draw(canvas)
    }

    fun drawStaticTextWithHoles(canvas: Canvas, layout: android.text.Layout, holes: List<android.graphics.RectF>) {
        if (holes.isEmpty()) {
            layout.draw(canvas)
            return
        }
        canvas.save()
        for (region in holes) {
            canvas.clipOutRect(region.left, region.top, region.right, region.bottom)
        }
        layout.draw(canvas)
        canvas.restore()
    }

    fun drawPreeditUnderline(canvas: Canvas, layout: android.text.Layout, compStart: Int, compEnd: Int) {
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

    fun drawCursor(canvas: Canvas, cursorUtf16: Int, cursorX: Float, cursorY: Float, cursorHeight: Float) {
        if (cursorUtf16 < 0) return
        canvas.drawRect(cursorX, cursorY, cursorX + 2f, cursorY + cursorHeight, cursorPaint)
    }

    fun setThemeColors(@Suppress("UNUSED_PARAMETER") textColor: Int, cursorColor: Int, selectionColor: Int, preeditColor: Int, bgColor: Int = Color.WHITE) {
        cursorPaint.color = cursorColor
        selectionPaint.color = selectionColor
        preeditUnderlinePaint.color = preeditColor
        backgroundColor = bgColor
    }
}
