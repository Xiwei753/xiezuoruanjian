package com.xiwei.sujian.editor.selfrender

import android.os.Handler
import android.os.Looper
import android.view.Choreographer
import android.view.View
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

/**
 * SujianCursorController — 自研写作区光标控制器
 *
 * 管理光标闪烁、光标可见性、光标平滑动画。
 *
 * ## 规则
 * - 光标闪烁：有焦点且无选区时闪烁
 * - 滚动中光标 snap（不闪烁）
 * - 光标动画由 Choreographer 驱动，FastOutSlowIn 插值
 * - 字号变化、加载章节、滚动时 snap，不做动画
 * - 光标宽度固定为 1.5dp，不随字号线性放大
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
    
    // 光标动画设置
    var smoothCursorEnabled = false
    var smoothCursorDurationMs = 80L
    
    // ── 光标视觉状态 ──
    data class CursorVisualState(
        val currentX: Float,
        val currentTop: Float,
        val currentBottom: Float,
        val targetX: Float,
        val targetTop: Float,
        val targetBottom: Float,
        val startX: Float,
        val startTop: Float,
        val startBottom: Float,
        val startTimeNanos: Long,
        val durationMs: Long
    )
    
    private var cursorVisualState: CursorVisualState? = null
    private var isCursorAnimating = false
    private var forceSnapNext = false
    
    // Choreographer 帧回调
    private val choreographer = Choreographer.getInstance()
    private var frameCallback: Choreographer.FrameCallback? = null
    
    // 插值器：FastOutSlowIn (Material Design)
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
    
    /**
     * 焦点变化
     */
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
    
    /**
     * 选区变化
     */
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
    
    /**
     * 滚动状态变化
     */
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
    
    /**
     * 字号变化时 snap 光标
     */
    fun onFontSizeChanged() {
        forceSnapNext = true
        stopCursorAnimation()
    }
    
    /**
     * 强制下次光标更新 snap 到目标位置
     */
    fun requestForceSnap() {
        forceSnapNext = true
    }
    
    /**
     * 加载章节时 snap 光标
     */
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
            // 恢复静态光标位置
            val state = cursorVisualState
            if (state != null) {
                renderer.cursorVisualX = state.targetX
                renderer.cursorVisualTop = state.targetTop
                renderer.cursorVisualBottom = state.targetBottom
                cursorVisualState = null
                view.invalidate()
            }
        }
    }
    
    /**
     * 更新光标目标位置（由 layout 计算后调用）
     * @param targetX 新的 X 坐标
     * @param targetTop 新的 top 坐标
     * @param targetBottom 新的 bottom 坐标
     * @param animate 是否动画
     */
    fun updateCursorTarget(targetX: Float, targetTop: Float, targetBottom: Float, animate: Boolean) {
        // 更新 renderer 的目标位置
        renderer.cursorTargetX = targetX
        renderer.cursorTargetTop = targetTop
        renderer.cursorTargetBottom = targetBottom
        
        val currentState = cursorVisualState
        
        if (!smoothCursorEnabled || !animate || forceSnapNext) {
            // Snap：直接跳到目标位置
            forceSnapNext = false
            stopCursorAnimation()
            renderer.cursorVisualX = targetX
            renderer.cursorVisualTop = targetTop
            renderer.cursorVisualBottom = targetBottom
            cursorVisualState = CursorVisualState(
                currentX = targetX, currentTop = targetTop, currentBottom = targetBottom,
                targetX = targetX, targetTop = targetTop, targetBottom = targetBottom,
                startX = targetX, startTop = targetTop, startBottom = targetBottom,
                startTimeNanos = 0, durationMs = 0
            )
            view.invalidate()
            return
        }
        
        // 动画：从当前位置插值到目标位置
        val startX = currentState?.currentX ?: targetX
        val startTop = currentState?.currentTop ?: targetTop
        val startBottom = currentState?.currentBottom ?: targetBottom
        
        // 跨行或大距离移动时 snap
        if (Math.abs(targetTop - startTop) > 5f) {
            renderer.cursorVisualX = targetX
            renderer.cursorVisualTop = targetTop
            renderer.cursorVisualBottom = targetBottom
            cursorVisualState = CursorVisualState(
                currentX = targetX, currentTop = targetTop, currentBottom = targetBottom,
                targetX = targetX, targetTop = targetTop, targetBottom = targetBottom,
                startX = targetX, startTop = targetTop, startBottom = targetBottom,
                startTimeNanos = 0, durationMs = 0
            )
            view.invalidate()
            return
        }
        
        cursorVisualState = CursorVisualState(
            currentX = startX, currentTop = startTop, currentBottom = startBottom,
            targetX = targetX, targetTop = targetTop, targetBottom = targetBottom,
            startX = startX, startTop = startTop, startBottom = startBottom,
            startTimeNanos = -1L, // 将在第一帧设置
            durationMs = smoothCursorDurationMs
        )
        
        startCursorAnimation()
    }
    
    private fun startCursorAnimation() {
        if (isCursorAnimating) return
        isCursorAnimating = true
        renderer.isCursorAnimating = true
        renderer.smoothCursorEnabled = smoothCursorEnabled
        
        val callback = object : Choreographer.FrameCallback {
            override fun doFrame(frameTimeNanos: Long) {
                if (!isCursorAnimating) return
                tickCursorAnimation(frameTimeNanos)
                if (isCursorAnimating) {
                    choreographer.postFrameCallback(this)
                }
            }
        }
        frameCallback = callback
        choreographer.postFrameCallback(callback)
    }
    
    private fun stopCursorAnimation() {
        isCursorAnimating = false
        renderer.isCursorAnimating = false
        frameCallback?.let { choreographer.removeFrameCallback(it) }
        frameCallback = null
    }
    
    private fun tickCursorAnimation(frameTimeNanos: Long) {
        val state = cursorVisualState ?: run {
            stopCursorAnimation()
            return
        }
        
        var startTimeNanos = state.startTimeNanos
        if (startTimeNanos < 0) {
            startTimeNanos = frameTimeNanos
            cursorVisualState = state.copy(startTimeNanos = startTimeNanos)
        }
        
        val elapsedNanos = frameTimeNanos - startTimeNanos
        val durationNanos = state.durationMs * 1_000_000f
        val progress = if (durationNanos > 0) {
            (elapsedNanos / durationNanos).coerceIn(0f, 1f)
        } else {
            1f
        }
        
        val interpolated = interpolator.getInterpolation(progress)
        
        val currentX = state.startX + (state.targetX - state.startX) * interpolated
        val currentTop = state.startTop + (state.targetTop - state.startTop) * interpolated
        val currentBottom = state.startBottom + (state.targetBottom - state.startBottom) * interpolated
        
        renderer.cursorVisualX = currentX
        renderer.cursorVisualTop = currentTop
        renderer.cursorVisualBottom = currentBottom
        
        cursorVisualState = state.copy(
            currentX = currentX,
            currentTop = currentTop,
            currentBottom = currentBottom
        )
        
        view.invalidate()
        
        if (progress >= 1f) {
            // 动画完成
            renderer.cursorVisualX = state.targetX
            renderer.cursorVisualTop = state.targetTop
            renderer.cursorVisualBottom = state.targetBottom
            cursorVisualState = state.copy(
                currentX = state.targetX,
                currentTop = state.targetTop,
                currentBottom = state.targetBottom
            )
            stopCursorAnimation()
        }
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
