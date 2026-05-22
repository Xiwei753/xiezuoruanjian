package com.xiwei.writerapp.ui

import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect

class SmoothCursorRenderer(private val editText: WriterEditText) : EditorAnimationRuntime.Animatable {

    var cursorRuntimeReady = false
    var smoothCursorEnabled = false
        private set
    var smoothCursorDurationMs: Long = 80L
        private set

    private var currentCursorX = -1f
    private var currentCursorTop = -1f
    private var currentCursorBottom = -1f

    private var isAnimating = false
    private var startTimeNanos = -1L
    private var startX = -1f
    private var startY_top = -1f
    private var startY_bottom = -1f
    private var targetX = -1f
    private var targetTop = -1f
    private var targetBottom = -1f

    private val interpolator = androidx.core.view.animation.PathInterpolatorCompat.create(0.4f, 0.0f, 0.2f, 1.0f) // FastOutSlowIn

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

    override fun onAnimationStep(frameTimeNanos: Long): Boolean {
        if (!smoothCursorEnabled || smoothCursorDurationMs <= 0) {
            currentCursorX = targetX
            currentCursorTop = targetTop
            currentCursorBottom = targetBottom
            isAnimating = false
            return false
        }
        if (startTimeNanos == -1L) {
            startTimeNanos = frameTimeNanos
        }
        val elapsedNanos = frameTimeNanos - startTimeNanos
        val durationNanos = smoothCursorDurationMs * 1_000_000f
        val progress = if (durationNanos > 0) {
            (elapsedNanos / durationNanos).coerceIn(0f, 1f)
        } else {
            1f
        }

        val interpolated = interpolator.getInterpolation(progress)

        currentCursorX = startX + (targetX - startX) * interpolated
        currentCursorTop = startY_top + (targetTop - startY_top) * interpolated
        currentCursorBottom = startY_bottom + (targetBottom - startY_bottom) * interpolated

        if (progress >= 1f) {
            isAnimating = false
            return false
        }
        return true
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
            isAnimating = false
            editText.animationRuntime?.unregister(this)
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
        val newTargetX = layout.getPrimaryHorizontal(pos)

        val baseline = layout.getLineBaseline(line).toFloat()
        val fontMetrics = editText.paint.fontMetrics
        // Add minimal vertical padding for better aesthetics
        val density = editText.resources.displayMetrics.density
        val cursorVerticalPadding = 1f * density
        val newTargetTop = baseline + fontMetrics.ascent + cursorVerticalPadding
        val newTargetBottom = baseline + fontMetrics.descent - cursorVerticalPadding

        if (currentCursorX < 0 || !animate || smoothCursorDurationMs <= 0) {
            isAnimating = false
            editText.animationRuntime?.unregister(this)
            currentCursorX = newTargetX
            currentCursorTop = newTargetTop
            currentCursorBottom = newTargetBottom
            invalidateCursorRect()
            return
        }

        // Initialize animation start positions
        // If currently animating, smoothly take over from current position
        startX = currentCursorX
        startY_top = currentCursorTop
        startY_bottom = currentCursorBottom

        targetX = newTargetX
        targetTop = newTargetTop
        targetBottom = newTargetBottom

        startTimeNanos = -1L
        isAnimating = true
        editText.animationRuntime?.register(this)
    }

    fun hideNativeCursorIfNeeded() {
        if (smoothCursorEnabled) {
            editText.isCursorVisible = false
        }
    }

    fun onDetachedFromWindow() {
        editText.removeCallbacks(cursorBlinkRunnable)
        isAnimating = false
        editText.animationRuntime?.unregister(this)
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
            val drawX = currentCursorX + editText.compoundPaddingLeft
            val drawTop = currentCursorTop + editText.compoundPaddingTop
            val drawBottom = currentCursorBottom + editText.compoundPaddingTop
            canvas.drawLine(drawX, drawTop, drawX, drawBottom, cursorPaint)
        }
    }
}
