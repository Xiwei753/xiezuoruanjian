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

    var isSuppressing = false

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
        if (isSuppressing) return

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

        var paragraphStart = 0
        var scanEnd = editable.length

        if (!isFullRebuild && updateStartPos >= 0) {
            var prevNewline = editable.lastIndexOf('\n', updateStartPos - 1)
            paragraphStart = if (prevNewline == -1) 0 else prevNewline + 1
        }

        val textLength = editable.length
        val spansToRemove = mutableListOf<LeadingMarginSpan.Standard>()

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
                val span = existingSpans.firstOrNull { editable.getSpanStart(it) == paragraphStart && editable.getSpanEnd(it) == paragraphEnd && it.getLeadingMargin(true) == autoIndentPx }
                if (span != null) {
                    spansToRemove.remove(span)
                }
            } else if (paragraphEnd > paragraphStart && !(paragraphEnd - paragraphStart == 1 && editable[paragraphStart] == '\n')) {
                val currentSpanEnd = spanRanges[paragraphStart]

                // Only replace spans if their boundaries or properties have fundamentally changed.
                // Avoid replacing during single character inputs when span range end updates naturally.
                if (currentSpanEnd != paragraphEnd) {
                    val span = existingSpans.firstOrNull { editable.getSpanStart(it) == paragraphStart && it.getLeadingMargin(true) == autoIndentPx }
                    if (span != null && editable.getSpanEnd(span) == paragraphEnd - 1 && editable.length >= paragraphEnd) {
                         // The text grew by 1 naturally at the end of the span, let the span grow.
                         spansToRemove.remove(span)
                         editable.setSpan(span, paragraphStart, paragraphEnd, Spanned.SPAN_PARAGRAPH)
                    } else {
                         editable.setSpan(LeadingMarginSpan.Standard(autoIndentPx, 0), paragraphStart, paragraphEnd, Spanned.SPAN_PARAGRAPH)
                    }
                } else {
                    val span = existingSpans.firstOrNull { editable.getSpanStart(it) == paragraphStart && editable.getSpanEnd(it) == paragraphEnd && it.getLeadingMargin(true) == autoIndentPx }
                    if (span != null) {
                        spansToRemove.remove(span)
                    } else {
                        editable.setSpan(LeadingMarginSpan.Standard(autoIndentPx, 0), paragraphStart, paragraphEnd, Spanned.SPAN_PARAGRAPH)
                    }
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
