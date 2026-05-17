package com.xiwei.writerapp.ui

import android.view.MotionEvent
import android.view.VelocityTracker
import android.view.ViewConfiguration
import android.widget.EditText
import android.widget.OverScroller
import kotlin.math.abs

class EditorFlingScroller(private val editText: EditText) {
    private val scroller = OverScroller(editText.context)
    private var velocityTracker: VelocityTracker? = null

    private val configuration = ViewConfiguration.get(editText.context)
    private val maximumVelocity = configuration.scaledMaximumFlingVelocity
    private val minimumVelocity = configuration.scaledMinimumFlingVelocity

    fun onTouchEvent(event: MotionEvent) {
        if (velocityTracker == null) {
            velocityTracker = VelocityTracker.obtain()
        }
        velocityTracker?.addMovement(event)

        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                if (!scroller.isFinished) {
                    scroller.abortAnimation()
                }
            }
            MotionEvent.ACTION_UP -> {
                velocityTracker?.computeCurrentVelocity(1000, maximumVelocity.toFloat())
                val velocityY = velocityTracker?.yVelocity ?: 0f

                if (abs(velocityY) > minimumVelocity) {
                    fling(-velocityY.toInt())
                }

                velocityTracker?.recycle()
                velocityTracker = null
            }
            MotionEvent.ACTION_CANCEL -> {
                velocityTracker?.recycle()
                velocityTracker = null
            }
        }
    }

    private fun fling(velocityY: Int) {
        val maxScrollY = getMaxScrollY()
        if (maxScrollY > 0) {
            scroller.fling(
                editText.scrollX, editText.scrollY,
                0, velocityY,
                0, 0,
                0, maxScrollY
            )
            editText.postInvalidateOnAnimation()
        }
    }

    fun computeScroll() {
        if (scroller.computeScrollOffset()) {
            val maxScrollY = getMaxScrollY()
            val y = scroller.currY.coerceIn(0, maxScrollY)
            editText.scrollTo(editText.scrollX, y)
            editText.postInvalidateOnAnimation()
        }
    }

    private fun getMaxScrollY(): Int {
        val layout = editText.layout ?: return 0
        val contentHeight = layout.height + editText.paddingTop + editText.paddingBottom
        return Math.max(0, contentHeight - editText.height)
    }

    fun onDetachedFromWindow() {
        if (!scroller.isFinished) {
            scroller.abortAnimation()
        }
        velocityTracker?.recycle()
        velocityTracker = null
    }
}
