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
import android.text.style.ReplacementSpan
import android.view.inputmethod.BaseInputConnection
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

    // Typing Animation
    private var typingAnimationEnabled = false
    private var lastAddedStart = -1
    private var lastAddedCount = 0

    fun setTypingAnimationEnabled(enabled: Boolean) {
        typingAnimationEnabled = enabled
    }

    private inner class TypingAnimationSpan : ReplacementSpan() {
        var progress: Float = 0f

        override fun getSize(paint: Paint, text: CharSequence, start: Int, end: Int, fm: Paint.FontMetricsInt?): Int {
            if (fm != null) {
                paint.getFontMetricsInt(fm)
            }
            return paint.measureText(text, start, end).toInt()
        }

        override fun draw(canvas: Canvas, text: CharSequence, start: Int, end: Int, x: Float, top: Int, y: Int, bottom: Int, paint: Paint) {
            val originalAlpha = paint.alpha
            val offsetX = (1f - progress) * -10f
            paint.alpha = (originalAlpha * progress).toInt().coerceIn(0, 255)
            canvas.drawText(text, start, end, x + offsetX, y.toFloat(), paint)
            paint.alpha = originalAlpha
        }
    }

    // Custom Cursor
    private var cursorRuntimeReady = false
    private var smoothCursorEnabled = false
    private var cursorAnimator: ValueAnimator? = null
    private var currentCursorX = -1f
    private var currentCursorTop = -1f
    private var currentCursorBottom = -1f
    private val cursorPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        strokeWidth = 4f
    }
    private var isCursorBlinkVisible = true
    private val cursorBlinkRunnable = object : Runnable {
        override fun run() {
            if (isFocused && smoothCursorEnabled && selectionStart == selectionEnd) {
                isCursorBlinkVisible = !isCursorBlinkVisible
                invalidateCursorRect()
                postDelayed(this, 500)
            }
        }
    }
    private val cursorRect = RectF()

    init {
        addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                if (!isUpdatingSpan && count > 0 && count < 50) {
                    lastAddedStart = start
                    lastAddedCount = count
                } else {
                    lastAddedStart = -1
                    lastAddedCount = 0
                }
            }
            override fun afterTextChanged(s: Editable?) {
                if (isUpdatingSpan) return

                val editable = s ?: return

                if (typingAnimationEnabled && lastAddedCount > 0 && lastAddedStart >= 0) {
                    val start = lastAddedStart
                    val end = start + lastAddedCount

                    val composingStart = BaseInputConnection.getComposingSpanStart(editable)
                    val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
                    val isComposing = composingStart != -1 && composingEnd != -1 && start < composingEnd && end > composingStart

                    if (end <= editable.length && !isComposing) {
                        val span = TypingAnimationSpan()
                        isUpdatingSpan = true
                        editable.setSpan(span, start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                        isUpdatingSpan = false

                        val lineTop = layout?.getLineTop(layout.getLineForOffset(start)) ?: 0
                        val lineBottom = layout?.getLineBottom(layout.getLineForOffset(end)) ?: height
                        val typeRect = android.graphics.Rect(0, lineTop + paddingTop - 16, width, lineBottom + paddingTop + 16)

                        ValueAnimator.ofFloat(0f, 1f).apply {
                            duration = 100
                            addUpdateListener { anim ->
                                span.progress = anim.animatedValue as Float
                                invalidate(typeRect)
                            }
                            addListener(object : android.animation.AnimatorListenerAdapter() {
                                override fun onAnimationEnd(animation: android.animation.Animator) {
                                    isUpdatingSpan = true
                                    editable.removeSpan(span)
                                    isUpdatingSpan = false
                                    invalidate(typeRect)
                                }
                            })
                            start()
                        }
                    }
                    lastAddedStart = -1
                    lastAddedCount = 0
                }

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
        cursorRuntimeReady = true
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

    fun setSmoothCursorEnabled(enabled: Boolean) {
        val wasEnabled = smoothCursorEnabled
        smoothCursorEnabled = enabled
        isCursorVisible = !enabled
        if (enabled && isFocused) {
            startCursorBlink()
            if (layout == null) {
                post { updateCursorTarget(false) }
            } else {
                updateCursorTarget(false)
            }
        } else {
            stopCursorBlink()
            if (wasEnabled) {
                if (cursorRuntimeReady && !lastInvalidateRect.isEmpty) {
                    invalidate(lastInvalidateRect)
                }
            }
        }
    }

    private fun startCursorBlink() {
        removeCallbacks(cursorBlinkRunnable)
        isCursorBlinkVisible = true
        postDelayed(cursorBlinkRunnable, 500)
    }

    private fun stopCursorBlink() {
        removeCallbacks(cursorBlinkRunnable)
        isCursorBlinkVisible = false
    }

    private val lastInvalidateRect = android.graphics.Rect()

    private fun invalidateCursorRect() {
        if (!cursorRuntimeReady || !smoothCursorEnabled || currentCursorX < 0 || width <= 0 || height <= 0) return

        val left = (currentCursorX + paddingLeft - 8f).toInt()
        val top = (currentCursorTop + paddingTop - 8f).toInt()
        val right = (currentCursorX + paddingLeft + 16f).toInt()
        val bottom = (currentCursorBottom + paddingTop + 8f).toInt()

        if (!lastInvalidateRect.isEmpty) {
            invalidate(lastInvalidateRect)
        }
        lastInvalidateRect.set(left, top, right, bottom)
        invalidate(lastInvalidateRect)
    }

    override fun onSelectionChanged(selStart: Int, selEnd: Int) {
        super.onSelectionChanged(selStart, selEnd)
        if (!cursorRuntimeReady) return
        if (smoothCursorEnabled && selStart == selEnd) {
            updateCursorTarget(true)
            startCursorBlink()
        } else if (smoothCursorEnabled) {
            invalidateCursorRect()
        }
    }

    private fun updateCursorTarget(animate: Boolean) {
        if (!cursorRuntimeReady) return
        val layout = layout ?: return
        val pos = selectionStart
        if (pos < 0) return

        val line = layout.getLineForOffset(pos)
        val targetX = layout.getPrimaryHorizontal(pos)
        val targetTop = layout.getLineTop(line).toFloat()
        val targetBottom = layout.getLineBottom(line).toFloat()

        if (currentCursorX < 0 || !animate || targetTop != currentCursorTop) {
            invalidateCursorRect()
            currentCursorX = targetX
            currentCursorTop = targetTop
            currentCursorBottom = targetBottom
            invalidateCursorRect()
            return
        }

        cursorAnimator?.cancel()

        val pX = PropertyValuesHolder.ofFloat("x", currentCursorX, targetX)
        val pTop = PropertyValuesHolder.ofFloat("top", currentCursorTop, targetTop)
        val pBottom = PropertyValuesHolder.ofFloat("bottom", currentCursorBottom, targetBottom)

        cursorAnimator = ValueAnimator.ofPropertyValuesHolder(pX, pTop, pBottom).apply {
            duration = 80
            addUpdateListener { anim ->
                invalidateCursorRect()
                currentCursorX = anim.getAnimatedValue("x") as Float
                currentCursorTop = anim.getAnimatedValue("top") as Float
                currentCursorBottom = anim.getAnimatedValue("bottom") as Float
                invalidateCursorRect()
            }
            start()
        }
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        removeCallbacks(cursorBlinkRunnable)
        cursorAnimator?.cancel()
    }

    override fun onFocusChanged(focused: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(focused, direction, previouslyFocusedRect)
        if (!cursorRuntimeReady) return
        if (smoothCursorEnabled) {
            if (focused) startCursorBlink() else stopCursorBlink()
            invalidateCursorRect()
        }
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        if (!cursorRuntimeReady) return
        if (smoothCursorEnabled && isFocused && selectionStart == selectionEnd && isCursorBlinkVisible && currentCursorX >= 0) {
            cursorPaint.color = currentTextColor
            val drawX = currentCursorX + paddingLeft
            val drawTop = currentCursorTop + paddingTop
            val drawBottom = currentCursorBottom + paddingTop
            canvas.drawLine(drawX, drawTop, drawX, drawBottom, cursorPaint)
        }
    }
}
