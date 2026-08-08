package com.xiwei.sujian.core.platform.hardware

import android.app.ActivityManager
import android.content.Context
import android.os.Build

data class LowRamCapability(
    val isLowRamDevice: Boolean = false,
    val memoryClassMb: Int = 0,
    val totalMemoryMb: Long = 0L,
)

fun detectLowRamCapability(context: Context): LowRamCapability {
    val activityManager = context.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager
    return LowRamCapability(
        isLowRamDevice = activityManager?.isLowRamDevice == true,
        memoryClassMb = activityManager?.memoryClass ?: 0,
        totalMemoryMb = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            activityManager?.let {
                val info = ActivityManager.MemoryInfo()
                it.getMemoryInfo(info)
                info.totalMem / (1024 * 1024)
            } ?: 0L
        } else 0L,
    )
}
