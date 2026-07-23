package com.xiwei.sujian.platform.hardware

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator

data class HapticsCapability(
    val hasVibrator: Boolean = false,
    val hasAmplitudeControl: Boolean = false,
)

fun detectHapticsCapability(context: Context): HapticsCapability {
    val vibrator = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        context.getSystemService(Vibrator::class.java)
    } else {
        @Suppress("DEPRECATION")
        context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
    }
    return HapticsCapability(
        hasVibrator = vibrator?.hasVibrator() == true,
        hasAmplitudeControl = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            vibrator?.hasAmplitudeControl() == true
        } else false,
    )
}
