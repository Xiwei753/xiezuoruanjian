package com.xiwei.writerapp.ui

import android.animation.PropertyValuesHolder
import android.animation.ValueAnimator
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import kotlin.math.abs

class SmoothCursorRenderer(private val editText: WriterEditText) {

    var cursorRuntimeReady = false
    var smoothCursorEnabled = false
        private set
    var smoothCursorDurationMs: Long = 80L
        private set

    private var cursorAnimator: ValueAnimator? = null
    private var currentCursorX = -1f
    private var currentCursorTop = -1f
    private var currentCursorBottom = -1f

    private val cursorPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        strokeWidth = 4f
    }

    private var isCursorBlinkVisible = true
    private val lastInvalidateRect = Rect()

    private val cursorBlinkRunnable = object : Runnable {
        override fun run() {
            if (editText.isFocused && smoothCursorEnabled && editText.selectionStart == editText.selectionEnd) {
                isCursorBlinkVisible = !isCursorBlinkVisible
                invalidateCursorRect()
                editText.postDelayed(this, 500)
            }
        }
    }

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long = 80L) {
        val wasEnabled = smoothCursorEnabled
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs
        editText.isCursorVisible = !enabled
        if (enabled && editText.isFocused) {
            startCursorBlink()
            if (editText.layout == null) {
                editText.post { updateCursorTarget(false) }
            } else {
                updateCursorTarget(false)
            }
        } else {
            stopCursorBlink()
            if (wasEnabled) {
                if (cursorRuntimeReady && !lastInvalidateRect.isEmpty) {
                    editText.invalidate(lastInvalidateRect)
                }
            }
        }
    }

    fun startCursorBlink() {
        editText.removeCallbacks(cursorBlinkRunnable)
        isCursorBlinkVisible = true
        editText.postDelayed(cursorBlinkRunnable, 500)
    }

    fun stopCursorBlink() {
        editText.removeCallbacks(cursorBlinkRunnable)
        isCursorBlinkVisible = false
    }

    fun invalidateCursorRect() {
        if (!cursorRuntimeReady || !smoothCursorEnabled || currentCursorX < 0 || editText.width <= 0 || editText.height <= 0) return

        // Invalidate coordinates need scrollX/Y because View.invalidate(Rect) is relative to the View's un-scrolled bounds,
        // so we must subtract scroll.
        val left = (currentCursorX + editText.compoundPaddingLeft - editText.scrollX - 8f).toInt()
        val top = (currentCursorTop + editText.compoundPaddingTop - editText.scrollY - 8f).toInt()
        val right = (currentCursorX + editText.compoundPaddingLeft - editText.scrollX + 16f).toInt()
        val bottom = (currentCursorBottom + editText.compoundPaddingTop - editText.scrollY + 8f).toInt()

        if (!lastInvalidateRect.isEmpty) {
            editText.invalidate(lastInvalidateRect)
        }
        val unionRect = Rect(lastInvalidateRect)
        unionRect.union(left, top, right, bottom)

        lastInvalidateRect.set(left, top, right, bottom)
        editText.invalidate(unionRect)
    }

    fun updateCursorTarget(animate: Boolean) {
        if (!cursorRuntimeReady) return
        val layout = editText.layout ?: return
        val pos = editText.selectionStart
        if (pos < 0) return

        val line = layout.getLineForOffset(pos)
        val targetX = layout.getPrimaryHorizontal(pos)
        val targetTop = layout.getLineTop(line).toFloat()
        val targetBottom = layout.getLineBottom(line).toFloat()



        if (currentCursorX < 0 || !animate || smoothCursorDurationMs <= 0) {
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

    fun hideNativeCursorIfNeeded() {
        if (smoothCursorEnabled) {
            editText.isCursorVisible = false
        }
    }

    fun onDetachedFromWindow() {
        editText.removeCallbacks(cursorBlinkRunnable)
        cursorAnimator?.cancel()
    }

    fun onFocusChanged(focused: Boolean) {
        if (smoothCursorEnabled) editText.isCursorVisible = false
        if (!cursorRuntimeReady) return
        if (smoothCursorEnabled) {
            if (focused) startCursorBlink() else stopCursorBlink()
            invalidateCursorRect()
        }
    }

    fun onSelectionChanged(selStart: Int, selEnd: Int) {
        if (smoothCursorEnabled) editText.isCursorVisible = false
        if (!cursorRuntimeReady) return
        if (smoothCursorEnabled && selStart == selEnd) {
            updateCursorTarget(true)
            startCursorBlink()
        } else if (smoothCursorEnabled) {
            invalidateCursorRect()
        }
    }

    fun draw(canvas: Canvas) {
        if (smoothCursorEnabled) editText.isCursorVisible = false
        if (!cursorRuntimeReady) return
        if (smoothCursorEnabled && editText.isFocused && editText.selectionStart == editText.selectionEnd && isCursorBlinkVisible && currentCursorX >= 0) {
            cursorPaint.color = editText.currentTextColor
            // The Canvas provided to onDraw is already translated by scrollX and scrollY.
            // We should only add padding.
            val drawX = currentCursorX + editText.compoundPaddingLeft
            val drawTop = currentCursorTop + editText.compoundPaddingTop
            val drawBottom = currentCursorBottom + editText.compoundPaddingTop
            canvas.drawLine(drawX, drawTop, drawX, drawBottom, cursorPaint)
        }
    }
}
