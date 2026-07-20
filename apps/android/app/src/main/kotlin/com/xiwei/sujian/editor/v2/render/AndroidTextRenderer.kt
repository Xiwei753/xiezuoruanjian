package com.xiwei.sujian.editor.v2.render

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF

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
     *
     * BlockShift rendering: the shifted region is first clipped out of the base static draw
     * (so it is not double-rendered), then re-drawn with canvas.translate(0, deltaY * (progress - 1)).
     * At progress=0 the text appears at its old Y position; at progress=1 it aligns with
     * the new layout (no translation).
     *
     * Coordinate semantics of the translate+clip sequence: canvas.translate shifts the
     * coordinate system, so the subsequent clipRect(shift.left, shift.top, shift.right, shift.bottom)
     * is in the *translated* coordinate frame. This means the clip region moves with the
     * translation, keeping the visible area aligned with the block's new-layout geometry
     * rather than the old-layout geometry. Without this, the clip would be at the wrong
     * position after translation, cutting off the shifted text.
     *
     * Merged BlockShifts: [AndroidVisualPlanner.mergeAdjacentBlockShifts] merges consecutive
     * paragraphs with identical deltaY into a single BlockShift entry. Each merged entry
     * triggers one [layout.draw] call per frame, so the total draw calls equal the number
     * of distinct deltaY groups, not the number of shifted paragraphs.
     *
     * Line-range clipping: each BlockShift stores [startLineIndex]/[endLineIndexExclusive]
     * and pre-computed [top]/[bottom]/[left]/[right] geometry. The renderer uses these
     * directly rather than converting from UTF-8 exclusive-end offsets, avoiding
     * [getLineForOffset] on an exclusive boundary that could land on the next paragraph's
     * first line.
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
        for (shift in blockShifts) {
            canvas.clipOutRect(
                shift.left, shift.top,
                shift.right, shift.bottom
            )
        }
        layout.draw(canvas)
        canvas.restore()
        val groupedByDeltaY = blockShifts.groupBy { it.deltaY }
        for ((deltaY, group) in groupedByDeltaY) {
            val currentDeltaY = deltaY * (progress - 1f)
            canvas.save()
            canvas.translate(0f, currentDeltaY)
            val clipPath = Path()
            for (shift in group) {
                clipPath.addRect(shift.left, shift.top, shift.right, shift.bottom, Path.Direction.CW)
            }
            canvas.clipPath(clipPath)
            layout.draw(canvas)
            canvas.restore()
        }
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
