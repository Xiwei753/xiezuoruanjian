package com.xiwei.sujian.editor.selfrender

import android.os.Handler
import android.os.Looper
import android.view.View

/**
 * SujianCursorController — 自研写作区光标控制器
 *
 * 管理光标闪烁、光标可见性、光标动画。
 *
 * ## 规则
 * - 光标闪烁：有焦点且无选区时闪烁
 * - 滚动中光标 snap（不闪烁）
 * - 光标动画由 Core Cursor 事件驱动
 */
class SujianCursorController(
    private val view: View,
    private val buffer: SujianEditorBuffer,
    private val renderer: SujianEditorRenderer
) {
    private val cursorBlinkHandler = Handler(Looper.getMainLooper())
    private val blinkPeriodMs = 530L
    
    private var isCursorBlinkOn = true
    private var _isCursorVisible = false
    val isCursorVisible: Boolean get() = _isCursorVisible
    private var isBlinking = false
    
    // 光标动画
    var smoothCursorEnabled = false
    var smoothCursorDurationMs = 80L
    
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
        }
        view.invalidate()
    }
    
    /**
     * 选区变化
     */
    fun onSelectionChanged() {
        if (buffer.selection.isCollapsed) {
            // 折叠选区 → 显示光标
            if (isCursorVisible && !isBlinking) {
                startBlinking()
            }
        } else {
            // 有选区 → 隐藏光标
            isCursorBlinkOn = false
            renderer.cursorBlinkOn = false
            stopBlinking()
        }
    }
    
    /**
     * 滚动状态变化
     */
    fun onScrollStateChanged(scrolling: Boolean) {
        if (scrolling) {
            // 滚动中光标 snap（显示但不闪烁）
            stopBlinking()
            isCursorBlinkOn = true
            renderer.cursorBlinkOn = true
        } else {
            if (isCursorVisible && buffer.selection.isCollapsed) {
                startBlinking()
            }
        }
    }
    
    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long = 80L) {
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs.coerceIn(20L, 500L)
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
    }
}
