package com.xiwei.writerapp.ui

import android.animation.Animator
import android.animation.AnimatorListenerAdapter
import android.animation.ValueAnimator
import android.graphics.Canvas
import android.graphics.Paint
import android.text.Editable
import android.text.Spanned
import android.text.style.ReplacementSpan
import android.util.Log
import android.view.inputmethod.BaseInputConnection
import android.widget.EditText
import kotlin.math.abs
import kotlin.math.sqrt

class TypingAnimationController(private val editText: EditText) {

    private val DEBUG_ANIM = false
    private val TAG = "WriterEditorAnim"

    var typingAnimationEnabled = false
        private set
    var typingAnimationDurationMs: Long = 100L
        private set

    private var lastAddedStart = -1
    private var lastAddedCount = 0
    private var cursorBeforeX = -1f
    private var cursorBeforeY = -1f

    private var isPasteOrDelete = false

    private val MAX_ANIMATIONS = 24

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        typingAnimationEnabled = enabled
        typingAnimationDurationMs = durationMs
    }

    fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {
        if (count > 0 && after == 0) {
            // Deletion
            isPasteOrDelete = true
        } else {
            // Include normal insertions and large pastes
            isPasteOrDelete = false
        }

        if (after > 0 && editText.layout != null) {
            val line = editText.layout.getLineForOffset(start)
            cursorBeforeX = editText.layout.getPrimaryHorizontal(start)
            cursorBeforeY = editText.layout.getLineBaseline(line).toFloat()
        }

        if (DEBUG_ANIM) {
            Log.d(TAG, "beforeTextChanged - replaced: $count, after: $after, cursor: ($cursorBeforeX, $cursorBeforeY)")
        }
    }

    fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
        if (!isPasteOrDelete && count > 0) {
            lastAddedStart = start
            lastAddedCount = count
        } else {
            lastAddedStart = -1
            lastAddedCount = 0
        }
    }

    fun afterTextChanged(editable: Editable?, onSpanUpdate: (Boolean) -> Unit) {
        if (editable == null) return

        val composingStart = BaseInputConnection.getComposingSpanStart(editable)
        val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
        val isComposing = composingStart != -1 && composingEnd != -1

        // Skip animation for composing regions (wait for commitText)
        if (isComposing && lastAddedStart >= composingStart && lastAddedStart < composingEnd) {
             if (DEBUG_ANIM) Log.d(TAG, "afterTextChanged - skipping animation for composing text.")
             lastAddedStart = -1
             lastAddedCount = 0
             return
        }

        if (typingAnimationEnabled && lastAddedCount > 0 && lastAddedStart >= 0) {
            val start = lastAddedStart
            val totalEnd = start + lastAddedCount

            val activeSpans = editable.getSpans(0, editable.length, TypingAnimSpan::class.java)

            // Remove oldest spans if we exceed limits to prevent infinite buildup
            if (activeSpans.size >= MAX_ANIMATIONS) {
                activeSpans.sortBy { editable.getSpanStart(it) }
                for (i in 0 until (activeSpans.size - MAX_ANIMATIONS + 1)) {
                    activeSpans[i].animator.cancel()
                }
            }

            // Cap the amount of text we animate to prevent freezing on large fast pastes
            val animLimit = Math.min(MAX_ANIMATIONS, lastAddedCount)
            val end = start + animLimit

            if (end <= editable.length && typingAnimationDurationMs > 0) {
                val animator = ValueAnimator.ofFloat(0f, 1f).apply {
                    duration = typingAnimationDurationMs
                    interpolator = android.view.animation.DecelerateInterpolator()
                }

                val span = TypingAnimSpan(cursorBeforeX, cursorBeforeY, animator)

                onSpanUpdate(true)
                editable.setSpan(span, start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                onSpanUpdate(false)

                animator.addUpdateListener { anim ->
                    span.progress = anim.animatedValue as Float
                    editText.invalidate()
                }
                animator.addListener(object : AnimatorListenerAdapter() {
                    override fun onAnimationEnd(animation: Animator) {
                        onSpanUpdate(true)
                        if (editable.getSpanStart(span) >= 0) {
                            editable.removeSpan(span)
                        }
                        onSpanUpdate(false)
                        editText.invalidate()
                    }
                })
                animator.start()
            }
            lastAddedStart = -1
            lastAddedCount = 0
        }
    }

    fun onDetachedFromWindow() {
        val editable = editText.text ?: return
        val activeSpans = editable.getSpans(0, editable.length, TypingAnimSpan::class.java)
        for (span in activeSpans) {
            span.animator.cancel()
        }
    }

    inner class TypingAnimSpan(
        val startX: Float,
        val startY: Float,
        val animator: ValueAnimator
    ) : ReplacementSpan() {
        var progress: Float = 0f

        override fun getSize(
            paint: Paint,
            text: CharSequence?,
            start: Int,
            end: Int,
            fm: Paint.FontMetricsInt?
        ): Int {
            if (text == null) return 0
            val measureText = text.subSequence(start, end).toString()
            return paint.measureText(measureText).toInt()
        }

        override fun draw(
            canvas: Canvas,
            text: CharSequence?,
            start: Int,
            end: Int,
            x: Float,
            top: Int,
            y: Int,
            bottom: Int,
            paint: Paint
        ) {
            if (text == null) return

            val layout = editText.layout ?: return
            val originalAlpha = paint.alpha
            paint.alpha = (originalAlpha * progress).toInt().coerceIn(0, 255)

            var i = start
            while (i < end) {
                val cp = Character.codePointAt(text, i)
                val charCount = Character.charCount(cp)
                val textToDraw = text.subSequence(i, i + charCount).toString()

                val destX = layout.getPrimaryHorizontal(i)
                val line = layout.getLineForOffset(i)
                val destY = layout.getLineBaseline(line).toFloat()

                var sX = startX
                var sY = startY

                if (sX < 0 || sY < 0) {
                    sX = destX
                    sY = destY
                } else {
                    val dx = destX - sX
                    val dy = destY - sY
                    val distSq = dx * dx + dy * dy
                    val maxDist = 80f
                    if (distSq > maxDist * maxDist) {
                        val dist = sqrt(distSq.toDouble()).toFloat()
                        sX = destX - (dx / dist) * maxDist
                        sY = destY - (dy / dist) * maxDist
                    }
                }

                val currentX = sX + (destX - sX) * progress
                val currentY = sY + (destY - sY) * progress

                // In ReplacementSpan, Canvas is already translated by compoundPaddingLeft/Top.
                // We should just draw at the target positions computed relative to layout.
                canvas.drawText(
                    textToDraw,
                    currentX,
                    currentY,
                    paint
                )
                i += charCount
            }

            paint.alpha = originalAlpha
        }
    }
}
