package com.xiwei.writerapp.ui

import android.graphics.Typeface
import android.view.View
import android.view.ViewGroup
import android.widget.TextView

/**
 * UiFontUtil — UI 字体工具类
 *
 * 递归遍历 View 树，将所有 TextView 的字体设置为 sans-serif 回退字体。
 *
 * ## 架构定位
 * - 全局工具类，用于解决部分设备字体渲染问题
 *
 * ## 使用场景
 * - 应用启动时统一设置字体
 * - 解决部分 Android 设备的字体兼容性问题
 */
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
