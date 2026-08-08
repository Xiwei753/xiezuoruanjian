package com.xiwei.sujian.feature.editor.render

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.os.Build
import androidx.core.graphics.withClip
import androidx.core.graphics.withSave
import androidx.core.graphics.withTranslation

private fun Canvas.clipOutRectCompat(
    left: Float,
    top: Float,
    right: Float,
    bottom: Float,
) {
    if (Build.VERSION.SDK_INT >= 26) {
        clipOutRect(left, top, right, bottom)
    } else {
        @Suppress("DEPRECATION")
        clipRect(left, top, right, bottom, android.graphics.Region.Op.DIFFERENCE)
    }
}

class AndroidTextRenderer(
    private val textPaint: Paint =
        Paint().apply {
            color = Color.BLACK
            isAntiAlias = true
        },
) {
    private var backgroundColor: Int = Color.WHITE
    private val backgroundPaint =
        Paint().apply {
            color = Color.WHITE
            style = Paint.Style.FILL
        }
    private val cursorPaint =
        Paint().apply {
            color = Color.BLACK
            strokeWidth = 2f
            isAntiAlias = true
        }
    private val selectionPaint =
        Paint().apply {
            color = Color.argb(51, 0, 0, 255)
            style = Paint.Style.FILL
            isAntiAlias = true
        }
    private val preeditUnderlinePaint =
        Paint().apply {
            color = Color.BLACK
            strokeWidth = 2f
            isAntiAlias = true
        }
    private val searchHighlightPaint =
        Paint().apply {
            color = Color.argb(40, 255, 200, 0)
            style = Paint.Style.FILL
            isAntiAlias = true
        }

    fun drawBackground(canvas: Canvas) {
        canvas.drawColor(backgroundColor)
    }

    fun drawSearchHighlights(
        canvas: Canvas,
        layout: android.text.Layout,
        highlights: List<Pair<Int, Int>>,
        blockShifts: List<com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction.BlockShift> = emptyList(),
        progress: Float = 1f,
    ) {
        if (highlights.isEmpty()) return
        if (blockShifts.isEmpty() || progress >= 1f) {
            drawSearchHighlightsUnshifted(canvas, layout, highlights)
            return
        }
        val shiftedLineRanges =
            blockShifts.flatMap {
                    shift ->
                shift.startLineIndex until shift.endLineIndexExclusive
            }.toSet()
        val unshiftedHighlights = mutableListOf<Pair<Int, Int>>()
        val shiftedHighlights = mutableListOf<Pair<Int, Int>>()
        for ((startUtf16, endUtf16) in highlights) {
            val startLine = layout.getLineForOffset(startUtf16)
            val endLine = layout.getLineForOffset((endUtf16 - 1).coerceAtLeast(startUtf16))
            val anyShifted = (startLine..endLine).any { it in shiftedLineRanges }
            if (anyShifted) {
                shiftedHighlights.add(Pair(startUtf16, endUtf16))
            } else {
                unshiftedHighlights.add(Pair(startUtf16, endUtf16))
            }
        }
        drawSearchHighlightsUnshifted(canvas, layout, unshiftedHighlights)
        canvas.withSave {
            for (shift in blockShifts) {
                canvas.clipOutRectCompat(shift.left, shift.top, shift.right, shift.bottom)
            }
            drawSearchHighlightsUnshifted(canvas, layout, shiftedHighlights)
        }
        val groupedByDeltaY = blockShifts.groupBy { it.deltaY }
        for ((deltaY, group) in groupedByDeltaY) {
            val currentDeltaY = deltaY * (progress - 1f)
            val groupLineRange =
                group.flatMap {
                        shift ->
                    shift.startLineIndex until shift.endLineIndexExclusive
                }.toSet()
            val groupShiftedHighlights =
                shiftedHighlights.filter { (startUtf16, endUtf16) ->
                    val startLine = layout.getLineForOffset(startUtf16)
                    val endLine = layout.getLineForOffset((endUtf16 - 1).coerceAtLeast(startUtf16))
                    (startLine..endLine).any { it in groupLineRange }
                }
            canvas.withTranslation(0f, currentDeltaY) {
                val clipPath = Path()
                for (shift in group) {
                    clipPath.addRect(shift.left, shift.top, shift.right, shift.bottom, Path.Direction.CW)
                }
                canvas.clipPath(clipPath)
                drawSearchHighlightsUnshifted(canvas, layout, groupShiftedHighlights)
            }
        }
    }

    private fun drawSearchHighlightsUnshifted(
        canvas: Canvas,
        layout: android.text.Layout,
        highlights: List<Pair<Int, Int>>,
    ) {
        val path = Path()
        for ((startUtf16, endUtf16) in highlights) {
            path.reset()
            layout.getSelectionPath(startUtf16, endUtf16, path)
            canvas.drawPath(path, searchHighlightPaint)
        }
    }

    fun drawSelectionHighlight(
        canvas: Canvas,
        layout: android.text.Layout,
        selStart: Int,
        selEnd: Int,
        blockShifts: List<com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction.BlockShift> = emptyList(),
        progress: Float = 1f,
    ) {
        if (selStart == selEnd) return
        if (blockShifts.isEmpty() || progress >= 1f) {
            drawSelectionHighlightUnshifted(canvas, layout, selStart, selEnd)
            return
        }
        val shiftedLineRanges =
            blockShifts.flatMap {
                    shift ->
                shift.startLineIndex until shift.endLineIndexExclusive
            }.toSet()
        val startLine = layout.getLineForOffset(selStart)
        val endLine = layout.getLineForOffset((selEnd - 1).coerceAtLeast(selStart))
        val anyShifted = (startLine..endLine).any { it in shiftedLineRanges }
        if (!anyShifted) {
            drawSelectionHighlightUnshifted(canvas, layout, selStart, selEnd)
            return
        }
        canvas.withSave {
            for (shift in blockShifts) {
                canvas.clipOutRectCompat(shift.left, shift.top, shift.right, shift.bottom)
            }
            drawSelectionHighlightUnshifted(canvas, layout, selStart, selEnd)
        }
        val groupedByDeltaY = blockShifts.groupBy { it.deltaY }
        for ((deltaY, group) in groupedByDeltaY) {
            val currentDeltaY = deltaY * (progress - 1f)
            val groupLineRange =
                group.flatMap {
                        shift ->
                    shift.startLineIndex until shift.endLineIndexExclusive
                }.toSet()
            val selStartLine = layout.getLineForOffset(selStart)
            val selEndLine = layout.getLineForOffset((selEnd - 1).coerceAtLeast(selStart))
            val overlapsGroup = (selStartLine..selEndLine).any { it in groupLineRange }
            if (!overlapsGroup) continue
            canvas.withTranslation(0f, currentDeltaY) {
                val clipPath = Path()
                for (shift in group) {
                    clipPath.addRect(shift.left, shift.top, shift.right, shift.bottom, Path.Direction.CW)
                }
                canvas.clipPath(clipPath)
                drawSelectionHighlightUnshifted(canvas, layout, selStart, selEnd)
            }
        }
    }

    private fun drawSelectionHighlightUnshifted(
        canvas: Canvas,
        layout: android.text.Layout,
        selStart: Int,
        selEnd: Int,
    ) {
        val path = Path()
        layout.getSelectionPath(selStart, selEnd, path)
        canvas.drawPath(path, selectionPaint)
    }

    fun drawStaticText(
        canvas: Canvas,
        layout: android.text.Layout,
    ) {
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
        blockShifts: List<com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction.BlockShift> = emptyList(),
        progress: Float = 1f,
    ) {
        if (holes.isEmpty() && blockShifts.isEmpty()) {
            layout.draw(canvas)
            return
        }
        canvas.withSave {
            for (region in holes) {
                canvas.clipOutRectCompat(region.left, region.top, region.right, region.bottom)
            }
            for (shift in blockShifts) {
                canvas.clipOutRectCompat(
                    shift.left,
                    shift.top,
                    shift.right,
                    shift.bottom,
                )
            }
            layout.draw(canvas)
        }
        // Group by deltaY so that all paragraphs shifting by the same amount share one
        // canvas.save/translate/clip/draw/restore cycle. After mergeAdjacentBlockShifts,
        // most edits produce a single group (all suffix paragraphs shift by the same deltaY),
        // so this typically results in one additional layout.draw() call per frame.
        // Multiple groups are rare (different deltaY values arise only when paragraphs
        // with different line heights shift by different amounts after an edit that
        // changes the number of visual lines in the edit paragraph).
        val groupedByDeltaY = blockShifts.groupBy { it.deltaY }
        for ((deltaY, group) in groupedByDeltaY) {
            val currentDeltaY = deltaY * (progress - 1f)
            canvas.withTranslation(0f, currentDeltaY) {
                val clipPath = Path()
                for (shift in group) {
                    clipPath.addRect(shift.left, shift.top, shift.right, shift.bottom, Path.Direction.CW)
                }
                canvas.clipPath(clipPath)
                layout.draw(canvas)
            }
        }
    }

    fun drawPreeditUnderline(
        canvas: Canvas,
        layout: android.text.Layout,
        compStart: Int,
        compEnd: Int,
        blockShifts: List<com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction.BlockShift> = emptyList(),
        progress: Float = 1f,
    ) {
        if (compStart < 0 || compEnd < 0 || compStart >= compEnd) return
        if (blockShifts.isEmpty() || progress >= 1f) {
            drawPreeditUnderlineUnshifted(canvas, layout, compStart, compEnd)
            return
        }
        val shiftedLineRanges =
            blockShifts.flatMap {
                    shift ->
                shift.startLineIndex until shift.endLineIndexExclusive
            }.toSet()
        val startLine = layout.getLineForOffset(compStart)
        val endLine = layout.getLineForOffset((compEnd - 1).coerceAtLeast(compStart))
        val anyShifted = (startLine..endLine).any { it in shiftedLineRanges }
        if (!anyShifted) {
            drawPreeditUnderlineUnshifted(canvas, layout, compStart, compEnd)
            return
        }
        canvas.withSave {
            for (shift in blockShifts) {
                canvas.clipOutRectCompat(shift.left, shift.top, shift.right, shift.bottom)
            }
            drawPreeditUnderlineUnshifted(canvas, layout, compStart, compEnd)
        }
        val groupedByDeltaY = blockShifts.groupBy { it.deltaY }
        for ((deltaY, group) in groupedByDeltaY) {
            val currentDeltaY = deltaY * (progress - 1f)
            val groupLineRange =
                group.flatMap {
                        shift ->
                    shift.startLineIndex until shift.endLineIndexExclusive
                }.toSet()
            val compStartLine = layout.getLineForOffset(compStart)
            val compEndLine = layout.getLineForOffset((compEnd - 1).coerceAtLeast(compStart))
            val overlapsGroup = (compStartLine..compEndLine).any { it in groupLineRange }
            if (!overlapsGroup) continue
            canvas.withTranslation(0f, currentDeltaY) {
                val clipPath = Path()
                for (shift in group) {
                    clipPath.addRect(shift.left, shift.top, shift.right, shift.bottom, Path.Direction.CW)
                }
                canvas.clipPath(clipPath)
                drawPreeditUnderlineUnshifted(canvas, layout, compStart, compEnd)
            }
        }
    }

    @Suppress("DEPRECATION")
    private fun drawPreeditUnderlineUnshifted(
        canvas: Canvas,
        layout: android.text.Layout,
        compStart: Int,
        compEnd: Int,
    ) {
        val startLine = layout.getLineForOffset(compStart)
        val endLine = layout.getLineForOffset((compEnd - 1).coerceAtLeast(compStart))
        for (line in startLine..endLine) {
            val lineStart = if (line == startLine) compStart else layout.getLineStart(line)
            val lineEnd = if (line == endLine) compEnd else layout.getLineEnd(line)
            val path = Path()
            layout.getSelectionPath(lineStart, lineEnd, path)
            if (path.isEmpty) continue
            val bottom = layout.getLineBottom(line).toFloat()
            canvas.withClip(path) {
                canvas.drawLine(0f, bottom, layout.width.toFloat(), bottom, preeditUnderlinePaint)
            }
        }
    }

    fun drawCursor(
        canvas: Canvas,
        cursorUtf16: Int,
        cursorX: Float,
        cursorY: Float,
        cursorHeight: Float,
    ) {
        if (cursorUtf16 < 0) return
        canvas.drawRect(cursorX, cursorY, cursorX + 2f, cursorY + cursorHeight, cursorPaint)
    }

    fun getCursorPaint(): Paint = cursorPaint

    fun getPaint(): Paint = textPaint

    fun getPreeditPaint(): Paint = preeditUnderlinePaint

    fun getSelectionPaint(): Paint = selectionPaint

    fun getBackgroundPaint(): Paint = backgroundPaint

    fun setThemeColors(
        textColor: Int,
        cursorColor: Int,
        selectionColor: Int,
        preeditColor: Int,
        bgColor: Int = Color.WHITE,
        searchHighlightColor: Int = 0,
    ) {
        textPaint.color = textColor
        cursorPaint.color = cursorColor
        selectionPaint.color = selectionColor
        preeditUnderlinePaint.color = preeditColor
        backgroundColor = bgColor
        backgroundPaint.color = bgColor
        searchHighlightPaint.color = searchHighlightColor
    }
}
