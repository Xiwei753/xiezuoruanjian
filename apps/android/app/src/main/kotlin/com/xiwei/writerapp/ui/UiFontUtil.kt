package com.xiwei.writerapp.ui

import android.graphics.Typeface
import android.view.View
import android.view.ViewGroup
import android.widget.TextView

object UiFontUtil {
    fun applySansSerifFallback(view: View) {
        if (view is TextView) {
            val currentStyle = view.typeface?.style ?: Typeface.NORMAL
            view.typeface = Typeface.create("sans-serif", currentStyle)
        } else if (view is ViewGroup) {
            for (i in 0 until view.childCount) {
                applySansSerifFallback(view.getChildAt(i))
            }
        }
    }
}
