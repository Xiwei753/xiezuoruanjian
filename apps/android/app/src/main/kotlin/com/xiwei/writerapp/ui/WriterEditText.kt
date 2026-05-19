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

    internal var isUpdatingSpanWrapper = false
    private var controllersReady = false

    private var typingAnimationController: TypingAnimationController? = null
    internal var renderLayer: EditorRenderLayer? = null
    private var autoIndentController: AutoIndentController? = null

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        if (!controllersReady) return
        typingAnimationController?.setTypingAnimationEnabled(enabled, durationMs)
    }

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long = 80L) {
        if (!controllersReady) return
        renderLayer?.smoothCursorRenderer?.setSmoothCursorEnabled(enabled, durationMs)
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
        renderLayer?.clear()
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
        val layer = EditorRenderLayer(this)
        renderLayer = layer
        typingAnimationController = TypingAnimationController(this, layer.typingOverlayRenderer)
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

                typingAnimationController?.afterTextChanged(editable)

                autoIndentController?.updateParagraphIndentSpans(editable, updateStartPos = selectionStart)
            }
        })

        layer.smoothCursorRenderer.cursorRuntimeReady = true

        viewTreeObserver.addOnGlobalLayoutListener {
            if (layer.smoothCursorRenderer.smoothCursorEnabled && isFocused) {
                layer.smoothCursorRenderer.updateCursorTarget(false)
            }
        }

        typeface = android.graphics.Typeface.create("sans-serif", typeface?.style ?: android.graphics.Typeface.NORMAL)
    }

    override fun onSelectionChanged(selStart: Int, selEnd: Int) {
        super.onSelectionChanged(selStart, selEnd)
        if (!controllersReady) return
        renderLayer?.onSelectionChanged(selStart, selEnd)
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        if (!controllersReady) return
        renderLayer?.onDetachedFromWindow()
        typingAnimationController?.onDetachedFromWindow()
    }

    override fun onFocusChanged(focused: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(focused, direction, previouslyFocusedRect)
        if (!controllersReady) return
        renderLayer?.onFocusChanged(focused)
    }

    override fun onDraw(canvas: Canvas) {
        if (!controllersReady) {
            super.onDraw(canvas)
            return
        }
        renderLayer?.beforeTextDraw()
        super.onDraw(canvas)
        renderLayer?.drawAfterText(canvas)
    }
}
