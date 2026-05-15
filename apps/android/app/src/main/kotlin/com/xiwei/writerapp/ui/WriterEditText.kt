package com.xiwei.writerapp.ui

import android.content.Context
import android.text.Editable
import android.text.Spanned
import android.text.TextWatcher
import android.text.style.LeadingMarginSpan
import android.util.AttributeSet
import androidx.appcompat.widget.AppCompatEditText

class WriterEditText @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = android.R.attr.editTextStyle
) : AppCompatEditText(context, attrs, defStyleAttr) {

    private var autoIndentEnabled: Boolean = false
    private var autoIndentPx: Int = 0
    private var currentIndentSpan: LeadingMarginSpan.Standard? = null
    private var isUpdatingSpan = false

    init {
        addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(s: Editable?) {
                if (isUpdatingSpan) return

                val editable = s ?: return

                if (currentIndentSpan == null && autoIndentEnabled && autoIndentPx > 0 && editable.isNotEmpty()) {
                    // Create span if it was missing (e.g. text was empty initially)
                    applyIndentation()
                    return
                }

                val span = currentIndentSpan ?: return

                // Fast path: just ensure the existing span covers the whole text.
                // Re-applying an existing span is extremely cheap if it's already there.
                val start = editable.getSpanStart(span)
                val end = editable.getSpanEnd(span)
                if (start != 0 || end != editable.length) {
                    isUpdatingSpan = true
                    editable.setSpan(span, 0, editable.length, Spanned.SPAN_INCLUSIVE_INCLUSIVE)
                    isUpdatingSpan = false
                }
            }
        })
    }

    fun setAutoIndent(enabled: Boolean, widthChars: Float) {
        val oldEnabled = this.autoIndentEnabled
        val oldPx = this.autoIndentPx

        this.autoIndentEnabled = enabled
        if (enabled && widthChars > 0) {
            val emWidth = paint.measureText("中")
            this.autoIndentPx = (emWidth * widthChars).toInt()
        } else {
            this.autoIndentPx = 0
        }

        if (oldEnabled != this.autoIndentEnabled || oldPx != this.autoIndentPx) {
            applyIndentation()
        }
    }

    private fun applyIndentation() {
        val editable = text ?: return

        isUpdatingSpan = true

        // Remove old span entirely
        val existingSpans = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java)
        for (span in existingSpans) {
            editable.removeSpan(span)
        }
        currentIndentSpan = null

        if (autoIndentEnabled && autoIndentPx > 0 && editable.isNotEmpty()) {
            val newSpan = LeadingMarginSpan.Standard(autoIndentPx, 0)
            currentIndentSpan = newSpan
            editable.setSpan(
                newSpan,
                0,
                editable.length,
                Spanned.SPAN_INCLUSIVE_INCLUSIVE
            )
        }

        isUpdatingSpan = false
    }
}
