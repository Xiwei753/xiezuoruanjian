package com.xiwei.sujian.ui.span

import android.graphics.Canvas
import android.graphics.Paint
import android.text.style.ReplacementSpan
import android.util.Log
import com.xiwei.sujian.ui.WriterEditText

class InputRevealSpan(
    private val editText: WriterEditText,
    private val start: Int,
    private val end: Int
) : ReplacementSpan() {

    private val TAG = "WriterInputReveal"

    var isRevealed = false
        private set

    private var frozenGlyphX: Float = -1f
    private var frozenGlyphY: Float = -1f
    private var frozenWidth: Float = -1f

    fun freezeGlyphRect(glyphX: Float, glyphY: Float) {
        frozenGlyphX = glyphX
        frozenGlyphY = glyphY
        val paint = editText.paint
        val editable = editText.text ?: return
        if (start in 0..end && end <= editable.length) {
            frozenWidth = paint.measureText(editable, start, end)
        }
    }

    fun reveal() {
        if (isRevealed) return
        isRevealed = true
        val editable = editText.text ?: return
        val s = editText.isUpdatingSpanWrapper
        editText.isUpdatingSpanWrapper = true
        try {
            editable.removeSpan(this)
        } finally {
            editText.isUpdatingSpanWrapper = s
        }
        Log.d(TAG, "reveal: span removed at [$start, $end)")
    }

    override fun getSize(
        paint: Paint,
        text: CharSequence?,
        start: Int,
        end: Int,
        fm: Paint.FontMetricsInt?
    ): Int {
        val width = if (frozenWidth >= 0f) frozenWidth.toInt()
        else paint.measureText(text ?: "", start, end).toInt()
        if (fm != null) {
            val pfm = paint.fontMetricsInt
            fm.top = pfm.top
            fm.ascent = pfm.ascent
            fm.descent = pfm.descent
            fm.bottom = pfm.bottom
        }
        return width
    }

    override fun draw(
        canvas: Canvas,
        text: CharSequence?,
        start: Int,
        end: Int,
        x: Float,
        top: Int,
        baseline: Int,
        bottom: Int,
        paint: Paint
    ) {
        if (isRevealed) {
            canvas.drawText(text ?: "", start, end, x, baseline.toFloat(), paint)
        }
    }
}
