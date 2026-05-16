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
import androidx.appcompat.widget.AppCompatEditText
import android.text.style.CharacterStyle
import android.text.TextPaint
import kotlin.math.abs

class WriterEditText @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = android.R.attr.editTextStyle
) : AppCompatEditText(context, attrs, defStyleAttr) {

    private var autoIndentEnabled: Boolean = false
    private var autoIndentPx: Int = 0
    private var isUpdatingSpan = false

    // Typing Animation
    private var typingAnimationEnabled = false
    private var typingAnimationDurationMs: Long = 100L
    private var lastAddedStart = -1
    private var lastAddedCount = 0

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        typingAnimationEnabled = enabled
        typingAnimationDurationMs = durationMs
    }

    private inner class TypingAnimationSpan : CharacterStyle() {
        var progress: Float = 0f
        override fun updateDrawState(tp: TextPaint) {
            val originalAlpha = tp.alpha
            tp.alpha = (originalAlpha * progress).toInt().coerceIn(0, 255)
        }
    }

    // Custom Cursor
    private var cursorRuntimeReady = false
    private var smoothCursorEnabled = false
    private var smoothCursorDurationMs: Long = 80L
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

    init {
        addTextChangedListener(object : TextWatcher {
            private var isPasteOrDelete = false

            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {
                if (isUpdatingSpan) return
                if (count > 0 && after == 0) {
                    // Deletion
                    isPasteOrDelete = true
                    if (smoothCursorEnabled) {
                        cursorAnimator?.cancel()
                    }
                } else if (after > 50) {
                    // Large paste
                    isPasteOrDelete = true
                } else {
                    isPasteOrDelete = false
                }
            }

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                if (isUpdatingSpan) return
                if (!isPasteOrDelete && count > 0 && count < 50) {
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

                // Handle Typing Animation
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

                        ValueAnimator.ofFloat(0f, 1f).apply {
                            duration = typingAnimationDurationMs
                            addUpdateListener { anim ->
                                span.progress = anim.animatedValue as Float
                                // Since we modified TextPaint via CharacterStyle, we just need to invalidate the text region.
                                // Instead of recalculating layout, a simple invalidate over the view does the trick
                                // or we can just invalidate the whole view for simplicity without layout measure
                                invalidate()
                            }
                            addListener(object : android.animation.AnimatorListenerAdapter() {
                                override fun onAnimationEnd(animation: android.animation.Animator) {
                                    isUpdatingSpan = true
                                    editable.removeSpan(span)
                                    isUpdatingSpan = false
                                    invalidate()
                                }
                            })
                            start()
                        }
                    }
                    lastAddedStart = -1
                    lastAddedCount = 0
                }

                // Handle Auto Indent
                updateParagraphIndentSpans(editable)
            }
        })
        cursorRuntimeReady = true

        viewTreeObserver.addOnGlobalLayoutListener {
            if (smoothCursorEnabled && isFocused) {
                updateCursorTarget(false)
            }
        }

        typeface = android.graphics.Typeface.create("sans-serif", typeface?.style ?: android.graphics.Typeface.NORMAL)
    }

    private fun updateParagraphIndentSpans(editable: Editable) {
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

        isUpdatingSpan = true
        val existingSpans = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java)
        val spanRanges = mutableMapOf<Int, Int>() // start -> end
        for (span in existingSpans) {
            val start = editable.getSpanStart(span)
            val end = editable.getSpanEnd(span)
            spanRanges[start] = end
        }

        var paragraphStart = 0
        val textLength = editable.length

        val spansToRemove = existingSpans.toMutableList()

        while (paragraphStart < textLength) {
            var paragraphEnd = editable.indexOf('\n', paragraphStart)
            if (paragraphEnd == -1) {
                paragraphEnd = textLength
            } else {
                paragraphEnd += 1 // Include newline character in the span
            }

            // Don't indent completely empty lines (where start == end, meaning just a newline or EOF)
            if (paragraphEnd > paragraphStart && !(paragraphEnd - paragraphStart == 1 && editable[paragraphStart] == '\n')) {
                // We need a span from paragraphStart to paragraphEnd
                val currentSpanEnd = spanRanges[paragraphStart]
                if (currentSpanEnd == paragraphEnd) {
                    // Match found, remove from the to-delete list
                    val span = existingSpans.firstOrNull { editable.getSpanStart(it) == paragraphStart && editable.getSpanEnd(it) == paragraphEnd && it.getLeadingMargin(true) == autoIndentPx }
                    if (span != null) {
                        spansToRemove.remove(span)
                    } else {
                        // Shouldn't happen but just in case
                        editable.setSpan(LeadingMarginSpan.Standard(autoIndentPx, 0), paragraphStart, paragraphEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                    }
                } else {
                    editable.setSpan(LeadingMarginSpan.Standard(autoIndentPx, 0), paragraphStart, paragraphEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                }
            }

            paragraphStart = paragraphEnd
        }

        // Remove spans that are no longer valid
        for (span in spansToRemove) {
            editable.removeSpan(span)
        }

        isUpdatingSpan = false
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
            val editable = text
            if (editable != null) {
                // If disabled, remove all spans. If enabled, apply new spans.
                updateParagraphIndentSpans(editable)
            }
        }
    }

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long = 80L) {
        val wasEnabled = smoothCursorEnabled
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs
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

        val left = (currentCursorX + compoundPaddingLeft - 8f).toInt()
        val top = (currentCursorTop + compoundPaddingTop - 8f).toInt()
        val right = (currentCursorX + compoundPaddingLeft + 16f).toInt()
        val bottom = (currentCursorBottom + compoundPaddingTop + 8f).toInt()

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

        val isNewLine = currentCursorTop >= 0 && abs(targetTop - currentCursorTop) > 1f

        if (currentCursorX < 0 || !animate || isNewLine) {
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
            duration = smoothCursorDurationMs
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
            val drawX = currentCursorX + compoundPaddingLeft
            val drawTop = currentCursorTop + compoundPaddingTop
            val drawBottom = currentCursorBottom + compoundPaddingTop
            canvas.drawLine(drawX, drawTop, drawX, drawBottom, cursorPaint)
        }
    }
}
