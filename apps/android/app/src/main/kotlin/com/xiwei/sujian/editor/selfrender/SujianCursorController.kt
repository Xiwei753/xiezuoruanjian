package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import android.os.Handler
import android.os.Looper
import android.view.View
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

/**
 * SujianCursorController — 光标控制器（统一 Timeline 版本，issue #515）
 *
 * 核心变更：
 * - 删除独立 Choreographer 路线。光标位置由事务 Timeline progress 驱动。
 * - 每帧由 SujianEditorView.onDraw() → tickCursorFromTimeline() 调用，
 *   不再自己 post Choreographer.FrameCallback。
 * - transactionDrivenCursor 的 progress 来自 AndroidPlatformVisualTransaction.timeline。
 * - 普通（无正文变更的）光标移动创建 CursorOnly PlatformVisualTransaction，
 *   仍使用相同 Timeline 类型。
 */
class SujianCursorController(
    private val view: View,
    private val buffer: SujianEditorBuffer,
    private val renderer: SujianEditorRenderer
) {
    private val TAG = "SujianCursorCtrl"
    private val cursorBlinkHandler = Handler(Looper.getMainLooper())
    private val blinkPeriodMs = 530L

    private var isCursorBlinkOn = true
    private var _isCursorVisible = false
    val isCursorVisible: Boolean get() = _isCursorVisible
    private var isBlinking = false

    var smoothCursorEnabled = false
    var smoothCursorDurationMs = 80L

    private var cursorFromX: Float = 0f
    private var cursorFromTop: Float = 0f
    private var cursorFromBottom: Float = 0f
    private var cursorTargetX: Float = 0f
    private var cursorTargetTop: Float = 0f
    private var cursorTargetBottom: Float = 0f

    internal var isCursorAnimating = false
    private var forceSnapNext = false

    private var activeTransaction: AndroidPlatformVisualTransaction? = null

    private val interpolator = android.view.animation.PathInterpolator(0.4f, 0.0f, 0.2f, 1.0f)

    private val blinkRunnable = object : Runnable {
        override fun run() {
            if (!isBlinking) return
            isCursorBlinkOn = !isCursorBlinkOn
            renderer.cursorBlinkOn = isCursorBlinkOn
            view.invalidate()
            cursorBlinkHandler.postDelayed(this, blinkPeriodMs)
        }
    }

    fun onFocusChanged(focused: Boolean) {
        _isCursorVisible = focused
        renderer.cursorVisible = focused

        if (focused) {
            startBlinking()
        } else {
            stopBlinking()
            stopCursorAnimation()
        }
        view.invalidate()
    }

    fun onSelectionChanged() {
        if (buffer.selection.isCollapsed) {
            if (isCursorVisible && !isBlinking) {
                startBlinking()
            }
        } else {
            isCursorBlinkOn = false
            renderer.cursorBlinkOn = false
            stopBlinking()
            stopCursorAnimation()
        }
    }

    fun onScrollStateChanged(scrolling: Boolean) {
        if (scrolling) {
            stopBlinking()
            stopCursorAnimation()
            isCursorBlinkOn = true
            renderer.cursorBlinkOn = true
            forceSnapNext = true
        } else {
            if (isCursorVisible && buffer.selection.isCollapsed) {
                startBlinking()
            }
        }
    }

    fun onFontSizeChanged() {
        forceSnapNext = true
        stopCursorAnimation()
    }

    fun requestForceSnap() {
        forceSnapNext = true
    }

    fun onChapterLoaded() {
        forceSnapNext = true
        stopCursorAnimation()
    }

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long = 80L) {
        val wasEnabled = smoothCursorEnabled
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs.coerceIn(20L, 500L)
        renderer.smoothCursorEnabled = enabled
        if (!enabled) {
            stopCursorAnimation()
            renderer.cursorVisualX = cursorTargetX
            renderer.cursorVisualTop = cursorTargetTop
            renderer.cursorVisualBottom = cursorTargetBottom
            view.invalidate()
        }
    }

    fun setTransactionDrivenCursor(transition: AndroidCursorTransition) {
        if (transition.isSnap) {
            val rect = transition.newRect ?: return
            snapCursorTo(rect.left, rect.top, rect.bottom)
            return
        }

        val oldRect = transition.oldRect ?: return
        val newRect = transition.newRect ?: return

        cursorFromX = oldRect.left
        cursorFromTop = oldRect.top
        cursorFromBottom = oldRect.bottom
        cursorTargetX = newRect.left
        cursorTargetTop = newRect.top
        cursorTargetBottom = newRect.bottom
        isCursorAnimating = true
        renderer.isCursorAnimating = true
        renderer.smoothCursorEnabled = smoothCursorEnabled
    }

    fun updateCursorTarget(targetX: Float, targetTop: Float, targetBottom: Float, animate: Boolean) {
        renderer.cursorTargetX = targetX
        renderer.cursorTargetTop = targetTop
        renderer.cursorTargetBottom = targetBottom

        if (!smoothCursorEnabled || !animate || forceSnapNext) {
            forceSnapNext = false
            stopCursorAnimation()
            snapCursorTo(targetX, targetTop, targetBottom)
            return
        }

        val tx = activeTransaction
        if (tx != null && (tx.state == AndroidVisualTransactionState.Rendering || tx.state == AndroidVisualTransactionState.Paused)) {
            cursorFromX = renderer.cursorVisualX
            cursorFromTop = renderer.cursorVisualTop
            cursorFromBottom = renderer.cursorVisualBottom
            cursorTargetX = targetX
            cursorTargetTop = targetTop
            cursorTargetBottom = targetBottom
            isCursorAnimating = true
            renderer.isCursorAnimating = true
            renderer.smoothCursorEnabled = smoothCursorEnabled
            return
        }

        cursorFromX = renderer.cursorVisualX
        cursorFromTop = renderer.cursorVisualTop
        cursorFromBottom = renderer.cursorVisualBottom
        cursorTargetX = targetX
        cursorTargetTop = targetTop
        cursorTargetBottom = targetBottom
        isCursorAnimating = true
        renderer.isCursorAnimating = true
        renderer.smoothCursorEnabled = smoothCursorEnabled
    }

    /**
     * 每帧由 onDraw() 调用，从事务 Timeline progress 计算光标位置。
     * 不再使用独立 Choreographer。
     */
    fun tickCursorFromTimeline() {
        if (!isCursorAnimating) return

        val tx = activeTransaction
        val progress: Float
        if (tx != null && (tx.state == AndroidVisualTransactionState.Rendering || tx.state == AndroidVisualTransactionState.Paused)) {
            progress = tx.progress
        } else {
            progress = 1f
        }

        val interpolated = interpolator.getInterpolation(progress)

        val currentX = cursorFromX + (cursorTargetX - cursorFromX) * interpolated
        val currentTop = cursorFromTop + (cursorTargetTop - cursorFromTop) * interpolated
        val currentBottom = cursorFromBottom + (cursorTargetBottom - cursorFromBottom) * interpolated

        renderer.cursorVisualX = currentX
        renderer.cursorVisualTop = currentTop
        renderer.cursorVisualBottom = currentBottom

        if (progress >= 1f) {
            renderer.cursorVisualX = cursorTargetX
            renderer.cursorVisualTop = cursorTargetTop
            renderer.cursorVisualBottom = cursorTargetBottom
            isCursorAnimating = false
            renderer.isCursorAnimating = false
            activeTransaction = null
        }
    }

    fun setActiveTransaction(tx: AndroidPlatformVisualTransaction?) {
        activeTransaction = tx
    }

    private fun snapCursorTo(x: Float, top: Float, bottom: Float) {
        renderer.cursorVisualX = x
        renderer.cursorVisualTop = top
        renderer.cursorVisualBottom = bottom
        cursorFromX = x
        cursorFromTop = top
        cursorFromBottom = bottom
        cursorTargetX = x
        cursorTargetTop = top
        cursorTargetBottom = bottom
        isCursorAnimating = false
        renderer.isCursorAnimating = false
        view.invalidate()
    }

    private fun stopCursorAnimation() {
        isCursorAnimating = false
        renderer.isCursorAnimating = false
        activeTransaction = null
    }

    private fun startBlinking() {
        if (isBlinking) return
        isBlinking = true
        isCursorBlinkOn = true
        renderer.cursorBlinkOn = true
        cursorBlinkHandler.removeCallbacks(blinkRunnable)
        cursorBlinkHandler.postDelayed(blinkRunnable, blinkPeriodMs)
    }

    private fun stopBlinking() {
        isBlinking = false
        cursorBlinkHandler.removeCallbacks(blinkRunnable)
    }

    fun onDetachedFromWindow() {
        stopBlinking()
        stopCursorAnimation()
    }
}
