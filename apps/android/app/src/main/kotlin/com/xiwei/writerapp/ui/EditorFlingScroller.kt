package com.xiwei.writerapp.ui

import android.content.Context
import android.view.MotionEvent
import android.view.VelocityTracker
import android.view.ViewConfiguration
import android.widget.OverScroller

class EditorFlingScroller(
    private val view: android.widget.TextView
) {
    private val scroller = OverScroller(view.context)
    private var velocityTracker: VelocityTracker? = null
    private var isTracking = false
    private val touchSlop: Float
    private var lastY = 0f
    private var isDragging = false

    init {
        touchSlop = ViewConfiguration.get(view.context).scaledTouchSlop.toFloat()
    }

    fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                if (!scroller.isFinished) {
                    scroller.forceFinished(true)
                }
                velocityTracker?.recycle()
                velocityTracker = VelocityTracker.obtain()
                velocityTracker?.addMovement(event)
                lastY = event.y
                isTracking = true
                isDragging = false
            }
            MotionEvent.ACTION_MOVE -> {
                if (!isTracking) return false
                velocityTracker?.addMovement(event)
                val dy = lastY - event.y
                if (kotlin.math.abs(dy) > touchSlop) {
                    isDragging = true
                }
                if (isDragging) {
                    val scrollY = view.scrollY
                    val maxScroll = computeMaxScrollY()
                    val newScrollY = (scrollY + dy).toInt().coerceIn(0, maxScroll)
                    view.scrollTo(0, newScrollY)
                }
                lastY = event.y
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                if (isTracking && isDragging) {
                    velocityTracker?.computeCurrentVelocity(1000)
                    val velocityY = velocityTracker?.yVelocity ?: 0f
                    if (kotlin.math.abs(velocityY) > ViewConfiguration.get(view.context).scaledMinimumFlingVelocity) {
                        fling(-velocityY.toInt())
                    }
                }
                velocityTracker?.recycle()
                velocityTracker = null
                isTracking = false
                isDragging = false
            }
        }
        return isDragging
    }

    fun fling(velocityY: Int) {
        val maxY = computeMaxScrollY()
        scroller.fling(
            0, view.scrollY,
            0, velocityY,
            0, 0,
            0, maxY,
            0, 40
        )
        view.postInvalidateOnAnimation()
    }

    fun computeScroll(): Boolean {
        if (scroller.computeScrollOffset()) {
            val maxScroll = computeMaxScrollY()
            val newScrollY = scroller.currY.coerceIn(0, maxScroll)
            view.scrollTo(0, newScrollY)
            view.postInvalidateOnAnimation()
            return true
        }
        return false
    }

    fun abortAnimation() {
        if (!scroller.isFinished) {
            scroller.forceFinished(true)
        }
    }

    private fun computeMaxScrollY(): Int {
        val contentHeight = view.layout?.height ?: 0
        val viewHeight = view.height - view.paddingTop - view.paddingBottom
        return kotlin.math.max(0, contentHeight - viewHeight)
    }
}
