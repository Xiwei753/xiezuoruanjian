package com.xiwei.sujian.ui

import android.graphics.Canvas
import android.graphics.Paint

/**
 * SmoothCursorRenderer — 平滑光标渲染器
 *
 * 使用插值动画实现光标的平滑移动效果，替代系统默认的跳跃式光标。
 *
 * ## 架构定位
 * - EditorRenderLayer → SmoothCursorRenderer → Canvas 绘制
 * - 实现 EditorAnimationRuntime.Animatable 接口
 *
 * ## 职责边界
 * - **做**：光标位置插值动画、光标绘制
 * - **不做**：光标位置计算（由系统 EditText 负责）
 *
 * ## 使用场景
 * - 编辑器中光标移动时的平滑过渡动画
 */
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

    private val cursorBlinkRunnable = object : Runnable {
        override fun run() {
            if (editText.isFocused && smoothCursorEnabled && editText.selectionStart == editText.selectionEnd) {
                isCursorBlinkVisible = !isCursorBlinkVisible
                invalidateCursorRect()
                editText.postDelayed(this, 500)
            }
        }
    }

    private data class CursorTargetCoords(val x: Float, val top: Float, val bottom: Float)

    private fun computeCursorTarget(pos: Int): CursorTargetCoords {
        val layout = editText.layout
        if (layout == null) {
            return CursorTargetCoords(-1f, -1f, -1f)
        }
        val line = layout.getLineForOffset(pos)
        val x = layout.getPrimaryHorizontal(pos)

        val baseline = layout.getLineBaseline(line).toFloat()
        val fontMetrics = editText.paint.fontMetrics
        // Add minimal vertical padding for better aesthetics
        val density = editText.resources.displayMetrics.density
        val cursorVerticalPadding = 1f * density
        val top = baseline + fontMetrics.ascent + cursorVerticalPadding
        val bottom = baseline + fontMetrics.descent - cursorVerticalPadding

        return CursorTargetCoords(x, top, bottom)
    }

    fun invalidateCursorRectAt(x: Float, t: Float, b: Float) {
        if (!cursorRuntimeReady || !smoothCursorEnabled || x < 0 || editText.width <= 0 || editText.height <= 0) return
        val left = (x + editText.compoundPaddingLeft - editText.scrollX - 8f).toInt()
        val top = (t + editText.compoundPaddingTop - editText.scrollY - 8f).toInt()
        val right = (x + editText.compoundPaddingLeft - editText.scrollX + 16f).toInt()
        val bottom = (b + editText.compoundPaddingTop - editText.scrollY + 8f).toInt()
        editText.postInvalidateOnAnimation(left, top, right, bottom)
    }

    fun invalidateCursorRect() {
        invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)
    }

    override fun onAnimationStep(frameTimeNanos: Long): Boolean {
        if (!smoothCursorEnabled || smoothCursorDurationMs <= 0) {
            invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)
            currentCursorX = targetX
            currentCursorTop = targetTop
            currentCursorBottom = targetBottom
            invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)
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

        // 先 invalidate 旧 cursor rect
        invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)

        // 更新 currentCursorX/top/bottom
        currentCursorX = startX + (targetX - startX) * interpolated
        currentCursorTop = startY_top + (targetTop - startY_top) * interpolated
        currentCursorBottom = startY_bottom + (targetBottom - startY_bottom) * interpolated

        // 再 invalidate 新 cursor rect
        invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)

        if (progress >= 1f) {
            currentCursorX = targetX
            currentCursorTop = targetTop
            currentCursorBottom = targetBottom
            invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)
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
                invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)
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

    fun updateCursorTarget(animate: Boolean) {
        if (!cursorRuntimeReady) return
        if (editText.layout == null) return
        val pos = editText.selectionStart
        val end = editText.selectionEnd
        if (pos < 0) return

        if (pos != end) {
            isAnimating = false
            editText.animationRuntime?.unregister(this)
            stopCursorBlink()
            invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)
            return
        }

        val coords = computeCursorTarget(pos)
        val newTargetX = coords.x
        val newTargetTop = coords.top
        val newTargetBottom = coords.bottom

        if (currentCursorX < 0 || !animate || smoothCursorDurationMs <= 0 || !smoothCursorEnabled) {
            invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)
            isAnimating = false
            editText.animationRuntime?.unregister(this)
            currentCursorX = newTargetX
            currentCursorTop = newTargetTop
            currentCursorBottom = newTargetBottom
            invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)
            return
        }

        // 快速连续点击时：新 target 从当前 cursor 位置接管；startTimeNanos 重置；不堆积多个 cursor anim。
        invalidateCursorRectAt(currentCursorX, currentCursorTop, currentCursorBottom)

        startX = currentCursorX
        startY_top = currentCursorTop
        startY_bottom = currentCursorBottom

        targetX = newTargetX
        targetTop = newTargetTop
        targetBottom = newTargetBottom

        startTimeNanos = -1L
        isAnimating = true
        editText.animationRuntime?.register(this)
        editText.postInvalidateOnAnimation()
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

    fun onEditorResume() {
        if (smoothCursorEnabled) editText.isCursorVisible = false
        if (!cursorRuntimeReady || !smoothCursorEnabled) return
        if (!editText.isFocused || editText.selectionStart != editText.selectionEnd) {
            stopCursorBlink()
            return
        }
        startTimeNanos = -1L
        if (editText.layout == null) {
            editText.post { updateCursorTarget(false) }
        } else {
            updateCursorTarget(false)
        }
        startCursorBlink()
        invalidateCursorRect()
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
            updateCursorTarget(true) // will stop animating and hide
        }
    }

    fun draw(canvas: Canvas) {
        if (smoothCursorEnabled) editText.isCursorVisible = false
        if (!cursorRuntimeReady) return
        if (smoothCursorEnabled && editText.isFocused && editText.selectionStart == editText.selectionEnd && isCursorBlinkVisible && currentCursorX >= 0) {
            cursorPaint.color = editText.currentTextColor
            val drawX = currentCursorX + editText.compoundPaddingLeft
            val drawTop = currentCursorTop + editText.compoundPaddingTop
            val drawBottom = currentCursorBottom + editText.compoundPaddingTop
            canvas.drawLine(drawX, drawTop, drawX, drawBottom, cursorPaint)
        }
    }
}
