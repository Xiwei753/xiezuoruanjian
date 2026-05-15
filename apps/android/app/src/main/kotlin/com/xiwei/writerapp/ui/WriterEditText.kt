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
    private var isUpdatingSpan = false

    init {
        addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(s: Editable?) {
                if (isUpdatingSpan) return
                applyIndentation()
            }
        })
    }

    fun setAutoIndent(enabled: Boolean, widthChars: Float) {
        this.autoIndentEnabled = enabled
        if (enabled && widthChars > 0) {
            val emWidth = paint.measureText("中")
            this.autoIndentPx = (emWidth * widthChars).toInt()
        } else {
            this.autoIndentPx = 0
        }
        applyIndentation()
    }

    private fun applyIndentation() {
        val editable = text ?: return
        if (editable.isEmpty()) return

        if (!autoIndentEnabled || autoIndentPx <= 0) {
            val existingSpans = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java)
            if (existingSpans.isNotEmpty()) {
                isUpdatingSpan = true
                for (span in existingSpans) {
                    editable.removeSpan(span)
                }
                isUpdatingSpan = false
            }
            return
        }

        val existingSpans = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java)

        // Check if there is exactly one span covering the whole text
        var needsUpdate = true
        if (existingSpans.size == 1) {
            val span = existingSpans[0]
            val start = editable.getSpanStart(span)
            val end = editable.getSpanEnd(span)

            // Note: Since we use INCLUSIVE_INCLUSIVE, the span might automatically grow.
            // However, Android sometimes breaks INCLUSIVE_INCLUSIVE spans on extreme edits.
            // If it covers 0 to length, we don't need to reapply.
            if (start == 0 && end == editable.length) {
                needsUpdate = false
            }
        }

        if (needsUpdate) {
            isUpdatingSpan = true
            for (span in existingSpans) {
                editable.removeSpan(span)
            }
            editable.setSpan(
                LeadingMarginSpan.Standard(autoIndentPx, 0),
                0,
                editable.length,
                Spanned.SPAN_INCLUSIVE_INCLUSIVE
            )
            isUpdatingSpan = false
        }
    }
}
