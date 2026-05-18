package com.xiwei.writerapp.ui

import android.animation.Animator
import android.animation.AnimatorListenerAdapter
import android.animation.ValueAnimator
import android.text.Editable
import android.util.Log
import android.view.inputmethod.BaseInputConnection
import android.text.style.ForegroundColorSpan
import android.graphics.Color
import android.text.Spanned
import android.widget.EditText

class TypingAnimationController(
    private val editText: EditText,
    private val renderer: TypingOverlayRenderer
) {
    private val DEBUG_ANIM = false
    private val TAG = "WriterEditorAnim"

    var typingAnimationEnabled = false
        private set
    var typingAnimationDurationMs: Long = 100L
        private set

    var isSuppressAnimations = false

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
        if (isSuppressAnimations) {
            renderer.clear()
            if (DEBUG_ANIM) {
                Log.d(TAG, "beforeTextChanged - suppressed animation")
            }
            return
        }

        if (count > 0 && after == 0) {
            // Deletion
            isPasteOrDelete = true
        } else if (after > 100) {
            // Treat very large replacements as paste to avoid massive animations
            isPasteOrDelete = true
        } else {
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
        if (isSuppressAnimations) {
            lastAddedStart = -1
            lastAddedCount = 0
            return
        }

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
        if (isSuppressAnimations) return

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
            val animLimit = Math.min(MAX_ANIMATIONS, lastAddedCount)
            val end = Math.min(start + animLimit, editable.length)

            if (end > start && typingAnimationDurationMs > 0) {
                val insertedText = editable.subSequence(start, end).toString()

                val animator = ValueAnimator.ofFloat(0f, 1f).apply {
                    duration = typingAnimationDurationMs
                    interpolator = android.view.animation.DecelerateInterpolator()
                }

                val transparentSpan = ForegroundColorSpan(Color.TRANSPARENT)
                editable.setSpan(transparentSpan, start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)

                val anim = OverlayAnim(
                    insertedStart = start,
                    insertedText = insertedText,
                    startX = cursorBeforeX,
                    startY = cursorBeforeY,
                    progress = 0f,
                    animator = animator,
                    span = transparentSpan
                )

                renderer.addAnim(anim)

                if (DEBUG_ANIM) {
                    Log.d(TAG, "afterTextChanged - created anim: insertedStart=${anim.insertedStart}, length=${insertedText.length}")
                }

                animator.addUpdateListener { a ->
                    anim.progress = a.animatedValue as Float
                    editText.invalidate()
                }
                animator.addListener(object : AnimatorListenerAdapter() {
                    override fun onAnimationEnd(animation: Animator) {
                        renderer.removeAnim(anim)
                        editable.removeSpan(transparentSpan)
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
        renderer.clear()
    }
}
