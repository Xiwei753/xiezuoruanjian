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

    /**
     * Draw the static text layout, clipping out [holes] regions that are covered by
     * animated slices. Each hole must be the exact bounding box of the cluster(s) the
     * animation renderer will draw — not a merged bounding rect of source + destination,
     * which would erase non-animated text between them (especially for cross-line Moves).
     *
     * [blockShifts] apply a uniform Y translation to paragraphs that shifted vertically
     * due to reflow in a preceding paragraph. These paragraphs do NOT have per-line Bitmaps —
     * the renderer translates the static new-layout text by the interpolated deltaY.
     * [progress] interpolates from 0 (old position) to 1 (new position).
     */
    fun drawStaticTextWithHoles(
        canvas: Canvas,
        layout: android.text.Layout,
        holes: List<android.graphics.RectF>,
        blockShifts: List<com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction.BlockShift> = emptyList(),
        progress: Float = 1f
    ) {
        if (holes.isEmpty() && blockShifts.isEmpty()) {
            layout.draw(canvas)
            return
        }
        canvas.save()
        for (region in holes) {
            canvas.clipOutRect(region.left, region.top, region.right, region.bottom)
        }
        if (blockShifts.isNotEmpty()) {
            val text = layout.text?.toString() ?: ""
            for (shift in blockShifts) {
                val startUtf16 = utf8ToUtf16(text, shift.paragraphStartUtf8)
                val endUtf16 = utf8ToUtf16(text, shift.paragraphEndUtf8.coerceAtMost(
                    text.toByteArray(Charsets.UTF_8).size
                ))
                val startLine = layout.getLineForOffset(startUtf16.coerceIn(0, text.length))
                val endLine = layout.getLineForOffset(endUtf16.coerceIn(0, text.length))
                val currentDeltaY = shift.deltaY * progress
                canvas.save()
                canvas.translate(0f, currentDeltaY)
                canvas.clipRect(
                    layout.getLineLeft(startLine), layout.getLineTop(startLine).toFloat(),
                    layout.getLineRight(endLine), layout.getLineBottom(endLine).toFloat()
                )
                layout.draw(canvas)
                canvas.restore()
            }
        } else {
            layout.draw(canvas)
        }
        canvas.restore()
    }

    private fun utf8ToUtf16(text: String, utf8Offset: Int): Int {
        val bytes = text.toByteArray(Charsets.UTF_8)
        val safeOffset = utf8Offset.coerceIn(0, bytes.size)
        return text.substring(0, String(bytes.copyOfRange(0, safeOffset), Charsets.UTF_8).length).length
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

    fun getCursorPaint(): Paint = cursorPaint

    fun setThemeColors(@Suppress("UNUSED_PARAMETER") textColor: Int, cursorColor: Int, selectionColor: Int, preeditColor: Int, bgColor: Int = Color.WHITE) {
        cursorPaint.color = cursorColor
        selectionPaint.color = selectionColor
        preeditUnderlinePaint.color = preeditColor
        backgroundColor = bgColor
    }
}
