package com.xiwei.writerapp.ui

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.animation.ValueAnimator
import android.animation.PropertyValuesHolder
import android.text.Editable
import android.text.Spanned
import android.text.TextWatcher
import android.view.inputmethod.BaseInputConnection
import android.text.style.LeadingMarginSpan
import android.util.AttributeSet
import android.util.Log
import androidx.appcompat.widget.AppCompatEditText
import android.text.style.CharacterStyle
import android.text.TextPaint
import kotlin.math.abs

class WriterEditText @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = android.R.attr.editTextStyle
) : AppCompatEditText(context, attrs, defStyleAttr) {

    private var isUpdatingSpanWrapper = false

    private val typingAnimationController = TypingAnimationController(this)
    private val smoothCursorRenderer = SmoothCursorRenderer(this)
    private val autoIndentController = AutoIndentController(this)

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        typingAnimationController.setTypingAnimationEnabled(enabled, durationMs)
    }

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long = 80L) {
        smoothCursorRenderer.setSmoothCursorEnabled(enabled, durationMs)
    }

    fun setAutoIndent(enabled: Boolean, widthChars: Float) {
        autoIndentController.setAutoIndent(enabled, widthChars)
    }

    init {
        addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {
                if (isUpdatingSpanWrapper || autoIndentController.isUpdatingSpan) return
                typingAnimationController.beforeTextChanged(s, start, count, after)
            }

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                if (isUpdatingSpanWrapper || autoIndentController.isUpdatingSpan) return
                typingAnimationController.onTextChanged(s, start, before, count)
            }

            override fun afterTextChanged(s: Editable?) {
                if (isUpdatingSpanWrapper || autoIndentController.isUpdatingSpan) return
                val editable = s ?: return

                typingAnimationController.afterTextChanged(editable) { updating ->
                    isUpdatingSpanWrapper = updating
                }

                autoIndentController.updateParagraphIndentSpans(editable)
            }
        })

        smoothCursorRenderer.cursorRuntimeReady = true

        viewTreeObserver.addOnGlobalLayoutListener {
            if (smoothCursorRenderer.smoothCursorEnabled && isFocused) {
                smoothCursorRenderer.updateCursorTarget(false)
            }
        }

        typeface = android.graphics.Typeface.create("sans-serif", typeface?.style ?: android.graphics.Typeface.NORMAL)
    }

    override fun onSelectionChanged(selStart: Int, selEnd: Int) {
        super.onSelectionChanged(selStart, selEnd)
        smoothCursorRenderer.onSelectionChanged(selStart, selEnd)
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        smoothCursorRenderer.onDetachedFromWindow()
        typingAnimationController.onDetachedFromWindow()
    }

    override fun onFocusChanged(focused: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(focused, direction, previouslyFocusedRect)
        smoothCursorRenderer.onFocusChanged(focused)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        smoothCursorRenderer.draw(canvas)
    }
}
