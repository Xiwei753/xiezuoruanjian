package com.xiwei.writerapp.ui

import android.content.Context
import android.graphics.Canvas
import android.text.Editable
import android.text.TextWatcher
import android.util.AttributeSet
import android.view.MotionEvent
import androidx.appcompat.widget.AppCompatEditText

class WriterEditText @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = android.R.attr.editTextStyle
) : AppCompatEditText(context, attrs, defStyleAttr) {

    private var isUpdatingSpanWrapper = false
    private var controllersReady = false

    private var typingAnimationController: TypingAnimationController? = null
    private var typingOverlayRenderer: TypingOverlayRenderer? = null
    private var smoothCursorRenderer: SmoothCursorRenderer? = null
    private var autoIndentController: AutoIndentController? = null

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        if (!controllersReady) return
        typingAnimationController?.setTypingAnimationEnabled(enabled, durationMs)
    }

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long = 80L) {
        if (!controllersReady) return
        smoothCursorRenderer?.setSmoothCursorEnabled(enabled, durationMs)
    }

    fun setAutoIndent(enabled: Boolean, widthChars: Float) {
        if (!controllersReady) return
        autoIndentController?.setAutoIndent(enabled, widthChars)
    }

    fun runWithoutTextAnimations(block: () -> Unit) {
        if (!controllersReady) {
            block()
            return
        }
        val oldAnimEnabled = typingAnimationController?.isSuppressAnimations ?: false
        val oldIndentEnabled = autoIndentController?.isSuppressing ?: false
        typingAnimationController?.isSuppressAnimations = true
        autoIndentController?.isSuppressing = true
        try {
            block()
        } finally {
            typingAnimationController?.isSuppressAnimations = oldAnimEnabled
            autoIndentController?.isSuppressing = oldIndentEnabled
            if (text != null) {
                autoIndentController?.updateParagraphIndentSpans(text!!, isFullRebuild = true)
            }
        }
    }

    init {
        typingOverlayRenderer = TypingOverlayRenderer(this)
        typingAnimationController = TypingAnimationController(this, typingOverlayRenderer!!)
        smoothCursorRenderer = SmoothCursorRenderer(this)
        autoIndentController = AutoIndentController(this)
        controllersReady = true

        addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {
                if (!controllersReady) return
                if (isUpdatingSpanWrapper || autoIndentController?.isUpdatingSpan == true) return
                typingAnimationController?.beforeTextChanged(s, start, count, after)
            }

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                if (!controllersReady) return
                if (isUpdatingSpanWrapper || autoIndentController?.isUpdatingSpan == true) return
                typingAnimationController?.onTextChanged(s, start, before, count)
            }

            override fun afterTextChanged(s: Editable?) {
                if (!controllersReady) return
                if (isUpdatingSpanWrapper || autoIndentController?.isUpdatingSpan == true) return
                val editable = s ?: return

                typingAnimationController?.afterTextChanged(editable) { updating ->
                    isUpdatingSpanWrapper = updating
                }

                autoIndentController?.updateParagraphIndentSpans(editable, updateStartPos = selectionStart)
            }
        })

        smoothCursorRenderer?.cursorRuntimeReady = true

        viewTreeObserver.addOnGlobalLayoutListener {
            if (smoothCursorRenderer?.smoothCursorEnabled == true && isFocused) {
                smoothCursorRenderer?.updateCursorTarget(false)
            }
        }

        typeface = android.graphics.Typeface.create("sans-serif", typeface?.style ?: android.graphics.Typeface.NORMAL)
    }

    override fun onSelectionChanged(selStart: Int, selEnd: Int) {
        super.onSelectionChanged(selStart, selEnd)
        if (!controllersReady) return
        smoothCursorRenderer?.onSelectionChanged(selStart, selEnd)
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        if (!controllersReady) return
        smoothCursorRenderer?.onDetachedFromWindow()
        typingAnimationController?.onDetachedFromWindow()
    }

    override fun onFocusChanged(focused: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(focused, direction, previouslyFocusedRect)
        if (!controllersReady) return
        smoothCursorRenderer?.onFocusChanged(focused)
    }

    override fun onDraw(canvas: Canvas) {
        if (smoothCursorRenderer?.smoothCursorEnabled == true && selectionStart == selectionEnd) {
            isCursorVisible = false
        }
        super.onDraw(canvas)
        if (!controllersReady) return
        smoothCursorRenderer?.draw(canvas)
        typingOverlayRenderer?.onDraw(canvas)
    }
}
