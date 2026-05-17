package com.xiwei.writerapp.ui

import android.text.Editable
import android.text.Spanned
import android.text.style.LeadingMarginSpan
import android.view.inputmethod.BaseInputConnection
import android.widget.EditText

class AutoIndentController(private val editText: EditText) {

    var autoIndentEnabled: Boolean = false
        private set
    var autoIndentPx: Int = 0
        private set

    var isUpdatingSpan = false
        private set

    fun setAutoIndent(enabled: Boolean, widthChars: Float) {
        val oldEnabled = this.autoIndentEnabled
        val oldPx = this.autoIndentPx

        this.autoIndentEnabled = enabled
        if (enabled && widthChars > 0) {
            val emWidth = editText.paint.measureText("中")
            this.autoIndentPx = (emWidth * widthChars).toInt()
        } else {
            this.autoIndentPx = 0
        }

        if (oldEnabled != this.autoIndentEnabled || oldPx != this.autoIndentPx) {
            val editable = editText.text
            if (editable != null) {
                updateParagraphIndentSpans(editable, isFullRebuild = true)
            }
        }
    }

    fun updateParagraphIndentSpans(editable: Editable, updateStartPos: Int = -1, isFullRebuild: Boolean = false) {
        if (!autoIndentEnabled || autoIndentPx <= 0) {
            val existingSpans = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java)
            if (existingSpans.isNotEmpty()) {
                isUpdatingSpan = true
                for (span in existingSpans) {
                    if (editable.getSpanStart(span) >= 0) {
                        editable.removeSpan(span)
                    }
                }
                isUpdatingSpan = false
            }
            return
        }

        isUpdatingSpan = true
        val existingSpans = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java)
        val spanRanges = mutableMapOf<Int, Int>()
        for (span in existingSpans) {
            val start = editable.getSpanStart(span)
            val end = editable.getSpanEnd(span)
            spanRanges[start] = end
        }

        // Optimization: only process affected area if possible
        var paragraphStart = 0
        var scanEnd = editable.length

        if (!isFullRebuild && updateStartPos >= 0) {
            // Find paragraph start
            var prevNewline = editable.lastIndexOf('\n', updateStartPos - 1)
            paragraphStart = if (prevNewline == -1) 0 else prevNewline + 1

            // Limit scan to some reasonable bounds if it's not a full rebuild, but let's
            // be safe and process to the end, or at least past the updated region.
            // A simple implementation processes from the modified paragraph to the end,
            // avoiding touching spans before the edit point.
        }

        val textLength = editable.length
        val spansToRemove = mutableListOf<LeadingMarginSpan.Standard>()

        // Identify spans in the scan region to potentially remove
        for (span in existingSpans) {
            val start = editable.getSpanStart(span)
            if (start >= paragraphStart) {
                spansToRemove.add(span)
            }
        }

        val composingStart = BaseInputConnection.getComposingSpanStart(editable)
        val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
        val isComposing = composingStart != -1 && composingEnd != -1

        while (paragraphStart < textLength) {
            var paragraphEnd = editable.indexOf('\n', paragraphStart)
            if (paragraphEnd == -1) {
                paragraphEnd = textLength
            } else {
                paragraphEnd += 1
            }

            val overlapsComposing = isComposing && paragraphEnd > composingStart && paragraphStart < composingEnd

            if (overlapsComposing) {
                // Keep the span overlapping with composing region to prevent jitter
                val span = existingSpans.firstOrNull { editable.getSpanStart(it) == paragraphStart && editable.getSpanEnd(it) == paragraphEnd && it.getLeadingMargin(true) == autoIndentPx }
                if (span != null) {
                    spansToRemove.remove(span)
                }
            } else if (paragraphEnd > paragraphStart && !(paragraphEnd - paragraphStart == 1 && editable[paragraphStart] == '\n')) {
                val currentSpanEnd = spanRanges[paragraphStart]
                if (currentSpanEnd == paragraphEnd) {
                    val span = existingSpans.firstOrNull { editable.getSpanStart(it) == paragraphStart && editable.getSpanEnd(it) == paragraphEnd && it.getLeadingMargin(true) == autoIndentPx }
                    if (span != null) {
                        spansToRemove.remove(span)
                    } else {
                        editable.setSpan(LeadingMarginSpan.Standard(autoIndentPx, 0), paragraphStart, paragraphEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                    }
                } else {
                    editable.setSpan(LeadingMarginSpan.Standard(autoIndentPx, 0), paragraphStart, paragraphEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                }
            }

            paragraphStart = paragraphEnd
        }

        for (span in spansToRemove) {
            if (editable.getSpanStart(span) >= 0) {
                editable.removeSpan(span)
            }
        }

        isUpdatingSpan = false
    }
}
