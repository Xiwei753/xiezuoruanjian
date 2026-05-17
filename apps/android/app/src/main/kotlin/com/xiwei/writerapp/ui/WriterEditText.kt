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

    private var autoIndentEnabled: Boolean = false
    private var autoIndentPx: Int = 0
    private var isUpdatingSpan = false

    // Typing Animation
    private var typingAnimationEnabled = false
    private var typingAnimationDurationMs: Long = 100L
    private var lastAddedStart = -1
    private var lastAddedCount = 0
    private var cursorBeforeX = -1f
    private var cursorBeforeY = -1f

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        typingAnimationEnabled = enabled
        typingAnimationDurationMs = durationMs
    }

    inner class TypingAnimSpan(
        val startX: Float,
        val startY: Float,
        val animator: ValueAnimator
    ) : CharacterStyle() {
        var progress: Float = 0f
        override fun updateDrawState(tp: TextPaint) {
            // Make the original text transparent so we can draw it manually
            tp.color = android.graphics.Color.TRANSPARENT
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
                        currentCursorX = -1f // force instant snap
                    }
                } else if (after > 50) {
                    // Large paste
                    isPasteOrDelete = true
                } else {
                    isPasteOrDelete = false
                }

                // Capture cursor coordinates before text is added
                if (after > 0 && layout != null) {
                    val line = layout.getLineForOffset(start)
                    cursorBeforeX = layout.getPrimaryHorizontal(start)
                    cursorBeforeY = layout.getLineBaseline(line).toFloat()
                }

                if (true) {
                    Log.d("WriterEditorAnim", "beforeTextChanged - oldLength: ${s?.length ?: 0}, newLength(expected): ${(s?.length ?: 0) - count + after}, replaced count: $count with after: $after. cursorBefore: ($cursorBeforeX, $cursorBeforeY)")
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

                    // Get currently active typing animation spans
                    val activeSpans = editable.getSpans(0, editable.length, TypingAnimSpan::class.java)

                    // If we have too many concurrent animations (e.g. fast typing or huge paste chunking),
                    // cancel oldest to prevent lag
                    var skippedReason: String? = null
                    if (activeSpans.size >= 20) {
                        val oldestSpan = activeSpans.first()
                        oldestSpan.animator.cancel()
                        skippedReason = "max_limit_exceeded (canceled oldest)"
                    }

                    if (end <= editable.length && typingAnimationDurationMs > 0) {
                        if (true) {
                            Log.d("WriterEditorAnim", "afterTextChanged - insertedRange: [$start, $end], animationCount: ${activeSpans.size + 1}, skippedReason: ${skippedReason ?: "none"}")
                        }

                        val animator = ValueAnimator.ofFloat(0f, 1f).apply {
                            duration = typingAnimationDurationMs
                            interpolator = android.view.animation.DecelerateInterpolator()
                        }

                        val span = TypingAnimSpan(cursorBeforeX, cursorBeforeY, animator)
                        isUpdatingSpan = true
                        editable.setSpan(span, start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                        isUpdatingSpan = false

                        animator.addUpdateListener { anim ->
                            span.progress = anim.animatedValue as Float
                            invalidate()
                        }
                        animator.addListener(object : android.animation.AnimatorListenerAdapter() {
                            override fun onAnimationEnd(animation: android.animation.Animator) {
                                isUpdatingSpan = true
                                if (editable.getSpanStart(span) >= 0) {
                                    editable.removeSpan(span)
                                }
                                isUpdatingSpan = false
                                invalidate()
                            }
                        })
                        animator.start()
                    }
                    lastAddedStart = -1
                    lastAddedCount = 0
                }

                // Handle Auto Indent
                if (isPasteOrDelete) {
                    updateParagraphIndentSpans(editable, isFullRebuild = true)
                } else {
                    updateParagraphIndentSpans(editable, updateStartPos = if (lastAddedStart >= 0) lastAddedStart else 0)
                }
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

    private fun updateParagraphIndentSpans(editable: Editable, updateStartPos: Int = -1, isFullRebuild: Boolean = false) {
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
        val spanRanges = mutableMapOf<Int, Int>() // start -> end
        for (span in existingSpans) {
            val start = editable.getSpanStart(span)
            val end = editable.getSpanEnd(span)
            spanRanges[start] = end
        }

        var paragraphStart = 0
        val textLength = editable.length

        val spansToRemove = existingSpans.toMutableList()

        val composingStart = BaseInputConnection.getComposingSpanStart(editable)
        val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
        val isComposing = composingStart != -1 && composingEnd != -1

        while (paragraphStart < textLength) {
            var paragraphEnd = editable.indexOf('\n', paragraphStart)
            if (paragraphEnd == -1) {
                paragraphEnd = textLength
            } else {
                paragraphEnd += 1 // Include newline character in the span
            }

            // Skip paragraph if it overlaps with composing region (to prevent jitter during IME input)
            val overlapsComposing = isComposing && paragraphEnd > composingStart && paragraphStart < composingEnd

            if (overlapsComposing) {
                // If the paragraph overlaps composing, we must preserve its existing span to avoid jitter.
                // Find the existing span for this paragraph and remove it from spansToRemove so it isn't deleted.
                val span = existingSpans.firstOrNull { editable.getSpanStart(it) == paragraphStart && editable.getSpanEnd(it) == paragraphEnd && it.getLeadingMargin(true) == autoIndentPx }
                if (span != null) {
                    spansToRemove.remove(span)
                }
            } else if (paragraphEnd > paragraphStart && !(paragraphEnd - paragraphStart == 1 && editable[paragraphStart] == '\n')) {
                // Don't indent completely empty lines (where start == end, meaning just a newline or EOF)
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
            if (editable.getSpanStart(span) >= 0) {
                editable.removeSpan(span)
            }
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
                updateParagraphIndentSpans(editable, isFullRebuild = true)
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

        val isNewLine = currentCursorTop >= 0 && Math.abs(targetTop - currentCursorTop) > 1f

        if (currentCursorX < 0 || !animate || isNewLine || smoothCursorDurationMs <= 0) {
            invalidateCursorRect()
            cursorAnimator?.cancel()
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

        // Draw typing animation spans manually
        if (typingAnimationEnabled && text != null && layout != null) {
            val editable = text!!
            val spans = editable.getSpans(0, editable.length, TypingAnimSpan::class.java)
            if (spans.isNotEmpty()) {
                val tp = paint
                val originalAlpha = tp.alpha

                for (span in spans) {
                    val start = editable.getSpanStart(span)
                    val end = editable.getSpanEnd(span)
                    if (start < 0 || end < 0 || start >= end) continue

                    val progress = span.progress
                    tp.alpha = (originalAlpha * progress).toInt().coerceIn(0, 255)

                    // Draw each character in the span
                    var i = start
                    while (i < end) {
                        val cp = Character.codePointAt(editable, i)
                        val charCount = Character.charCount(cp)
                        val textToDraw = editable.subSequence(i, i + charCount).toString()

                        // Final destination
                        val destX = layout.getPrimaryHorizontal(i)
                        val line = layout.getLineForOffset(i)
                        val destY = layout.getLineBaseline(line).toFloat()

                        // Clamped start coordinates
                        var sX = span.startX
                        var sY = span.startY

                        // If starting position was invalid or not captured, default to destination
                        if (sX < 0 || sY < 0) {
                            sX = destX
                            sY = destY
                        } else {
                            // Clamp delta to prevent flying across the screen on huge pastes
                            val dx = destX - sX
                            val dy = destY - sY
                            val distSq = dx * dx + dy * dy
                            val maxDist = 80f // Limit animation origin distance
                            if (distSq > maxDist * maxDist) {
                                val dist = Math.sqrt(distSq.toDouble()).toFloat()
                                sX = destX - (dx / dist) * maxDist
                                sY = destY - (dy / dist) * maxDist
                            }
                        }

                        // Interpolate position
                        val currentX = sX + (destX - sX) * progress
                        val currentY = sY + (destY - sY) * progress

                        canvas.drawText(
                            textToDraw,
                            currentX + compoundPaddingLeft,
                            currentY + compoundPaddingTop,
                            tp
                        )
                        i += charCount
                    }
                }
                tp.alpha = originalAlpha
            }
        }

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
