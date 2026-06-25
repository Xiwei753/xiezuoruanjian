package com.xiwei.sujian.ui.span

import android.graphics.Canvas
import android.graphics.Paint
import android.text.style.ReplacementSpan
import android.util.Log
import com.xiwei.sujian.ui.WriterEditText

class DeletingHoldSpan(
    private val editText: WriterEditText,
    private val start: Int,
    private val end: Int
) : ReplacementSpan() {

    private val TAG = "WriterDeletingHold"

    var isDeleted = false
        private set

    private var frozenWidth: Float = -1f

    fun freezeWidth() {
        val paint = editText.paint
        val editable = editText.text ?: return
        if (start in 0..end && end <= editable.length) {
            frozenWidth = paint.measureText(editable, start, end)
        }
    }

    fun performDelete() {
        if (isDeleted) return
        isDeleted = true
        val editable = editText.text ?: return
        val s = editText.isUpdatingSpanWrapper
        editText.isUpdatingSpanWrapper = true
        try {
            editable.removeSpan(this)
            if (start in 0..end && end <= editable.length) {
                editable.delete(start, end)
            }
        } finally {
            editText.isUpdatingSpanWrapper = s
        }
        Log.d(TAG, "performDelete: deleted at [$start, $end)")
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
        if (!isDeleted) {
            val alpha = paint.alpha
            paint.alpha = (alpha * 0.4f).toInt().coerceIn(0, 255)
            canvas.drawText(text ?: "", start, end, x, baseline.toFloat(), paint)
            paint.alpha = alpha
        }
    }
}
