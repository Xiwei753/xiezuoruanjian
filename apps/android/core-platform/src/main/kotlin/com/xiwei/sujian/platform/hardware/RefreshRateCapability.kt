package com.xiwei.sujian.platform.hardware

import android.content.Context
import android.os.Build
import android.view.Display
import android.view.WindowManager

data class RefreshRateCapability(
    val maxRefreshRateHz: Float = 60f,
    val minRefreshRateHz: Float = 60f,
)

fun detectRefreshRateCapability(context: Context): RefreshRateCapability {
    val windowManager = context.getSystemService(Context.WINDOW_SERVICE) as? WindowManager
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
        val display = windowManager?.defaultDisplay
        val mode = display?.let { d ->
            if (d.supportedModes.isNotEmpty()) {
                d.supportedModes.maxByOrNull { it.refreshRate }
            } else null
        }
        RefreshRateCapability(
            maxRefreshRateHz = mode?.refreshRate ?: 60f,
            minRefreshRateHz = 60f,
        )
    } else {
        RefreshRateCapability()
    }
}
