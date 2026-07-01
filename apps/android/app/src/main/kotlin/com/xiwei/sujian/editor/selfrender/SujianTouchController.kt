package com.xiwei.sujian.editor.selfrender

import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.widget.OverScroller
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

/**
 * SujianTouchController — 自研写作区触摸控制器
 *
 * 管理触摸事件：点击定位光标、拖动选择、滚动。
 *
 * ## 第一阶段
 * - 点击定位光标
 * - 拖动选择
 * - 长按选词可后置
 * - 滚动（fling + drag）
 */
class SujianTouchController(
    private val view: View,
    private val buffer: SujianEditorBuffer,
    private val layout: SujianEditorLayout,
    private val selectionController: SujianSelectionController,
    private val cursorController: SujianCursorController,
    private val animationController: SujianAnimationController
) {
    private val TAG = "SujianTouchCtrl"
    
    /**
     * 滚动位置变化回调。
     * 用于通知 IME 更新 CursorAnchorInfo，确保候选框跟随光标。
     */
    var onScrollChanged: (() -> Unit)? = null
    
    private val touchConfig = ViewConfiguration.get(view.context)
    private val touchSlop = touchConfig.scaledTouchSlop
    private val minimumFlingVelocity = touchConfig.scaledMinimumFlingVelocity
    private val maximumFlingVelocity = touchConfig.scaledMaximumFlingVelocity
    
    private val scroller = OverScroller(view.context)
    
    // 触摸状态
    private var isDragging = false
    private var isScrolling = false
    private var touchDownX = 0f
    private var touchDownY = 0f
    private var lastTouchY = 0f
    private var hasMovedBeyondSlop = false
    
    // 滚动状态
    var scrollX: Int = 0
        private set
    var scrollY: Int = 0
        private set
    
    // 滚动空闲检测
    private val scrollIdleDelayMs = 140L
    private val scrollIdleRunnable = Runnable { setScrolling(false) }
    
    /**
     * 处理触摸事件
     */
    fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                touchDownX = event.x
                touchDownY = event.y
                lastTouchY = event.y
                hasMovedBeyondSlop = false
                isDragging = false
                scroller.forceFinished(true)
                return true
            }
            
            MotionEvent.ACTION_MOVE -> {
                val dx = event.x - touchDownX
                val dy = event.y - touchDownY
                
                if (!hasMovedBeyondSlop) {
                    if (kotlin.math.abs(dy) > touchSlop || kotlin.math.abs(dx) > touchSlop) {
                        hasMovedBeyondSlop = true
                        isDragging = true
                        setScrolling(true)
                    }
                }
                
                if (hasMovedBeyondSlop) {
                    // 滚动
                    val deltaY = lastTouchY - event.y
                    scrollBy(0, deltaY.toInt())
                    lastTouchY = event.y
                }
                
                return true
            }
            
            MotionEvent.ACTION_UP -> {
                if (!hasMovedBeyondSlop) {
                    // 点击 → 定位光标
                    val adjustedX = event.x + scrollX
                    val adjustedY = event.y + scrollY
                    selectionController.handleTap(adjustedX, adjustedY)
                    cursorController.onSelectionChanged()
                    view.invalidate()
                } else {
                    // 可能 fling
                    // 第一阶段简化：不实现 fling
                    setScrolling(false)
                }
                
                isDragging = false
                return true
            }
            
            MotionEvent.ACTION_CANCEL -> {
                isDragging = false
                setScrolling(false)
                return true
            }
        }
        
        return false
    }
    
    /**
     * 获取最大滚动 Y
     */
    fun getMaxScrollY(): Int {
        val contentHeight = layout.getHeight(buffer.text)
        val viewportHeight = view.height - view.paddingTop - view.paddingBottom
        return (contentHeight - viewportHeight).coerceAtLeast(0)
    }
    
    /**
     * 滚动指定偏移
     */
    fun scrollBy(dx: Int, dy: Int) {
        val newX = (scrollX + dx).coerceIn(0, 0) // 暂不支持水平滚动
        val newY = (scrollY + dy).coerceIn(0, getMaxScrollY())
        
        if (newX != scrollX || newY != scrollY) {
            scrollX = newX
            scrollY = newY
            view.invalidate()
            onScrollChanged?.invoke()
        }
    }
    
    /**
     * 滚动到指定位置
     */
    fun scrollTo(x: Int, y: Int) {
        val newX = x.coerceIn(0, 0)
        val newY = y.coerceIn(0, getMaxScrollY())
        
        if (newX != scrollX || newY != scrollY) {
            scrollX = newX
            scrollY = newY
            view.invalidate()
            onScrollChanged?.invoke()
        }
    }
    
    /**
     * 确保光标可见（自动滚动）
     */
    fun ensureCursorVisible() {
        val cursorRect = layout.getCursorRect(buffer.text, buffer.selection.head)
        val cursorTop = cursorRect.top.toInt() - view.paddingTop
        val cursorBottom = cursorRect.bottom.toInt() - view.paddingTop
        val viewportHeight = view.height - view.paddingTop - view.paddingBottom
        
        if (cursorTop < scrollY) {
            scrollTo(scrollX, cursorTop)
        } else if (cursorBottom > scrollY + viewportHeight) {
            scrollTo(scrollX, cursorBottom - viewportHeight)
        }
    }
    
    fun onDetachedFromWindow() {
        scroller.forceFinished(true)
        view.removeCallbacks(scrollIdleRunnable)
    }
    
    private fun setScrolling(scrolling: Boolean) {
        if (isScrolling != scrolling) {
            isScrolling = scrolling
            animationController.setScrolling(scrolling)
            cursorController.onScrollStateChanged(scrolling)
        }
        view.removeCallbacks(scrollIdleRunnable)
        if (scrolling) {
            view.postDelayed(scrollIdleRunnable, scrollIdleDelayMs)
        }
    }
}
