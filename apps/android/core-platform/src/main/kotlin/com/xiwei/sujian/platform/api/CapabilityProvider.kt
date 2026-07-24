package com.xiwei.sujian.platform.api

import android.app.ActivityManager
import android.content.Context
import android.content.res.Configuration
import android.hardware.SensorManager
import android.os.Build
import android.view.WindowManager
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

open class CapabilityProvider(private val context: Context) {

    private val _capabilities = MutableStateFlow(detectCapabilities())
    val capabilities: StateFlow<AndroidCapabilities> = _capabilities.asStateFlow()

    fun updateFromConfiguration(config: Configuration) {
        val current = _capabilities.value
        val windowSizeClass = config.toWindowSizeClass()
        val hasHardwareKeyboard = config.keyboard != Configuration.KEYBOARD_NOKEYS
        val pointerKinds = config.toPointerKinds()
        _capabilities.value = current.copy(
            windowSizeClass = windowSizeClass,
            hasHardwareKeyboard = hasHardwareKeyboard,
            pointerKinds = pointerKinds,
        )
    }

    fun updateFromFoldFeatures(features: List<androidx.window.layout.DisplayFeature>) {
        val current = _capabilities.value
        val foldPosture = features.toFoldPosture()
        _capabilities.value = current.copy(foldPosture = foldPosture)
    }

    private fun detectCapabilities(): AndroidCapabilities {
        val sensorManager = context.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
        val activityManager = context.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager
        val windowManager = context.getSystemService(Context.WINDOW_SERVICE) as? WindowManager

        val hasGyroscope = sensorManager?.getDefaultSensor(android.hardware.Sensor.TYPE_GYROSCOPE) != null
        val hasAccelerometer = sensorManager?.getDefaultSensor(android.hardware.Sensor.TYPE_ACCELEROMETER) != null
        val isLowRamDevice = activityManager?.isLowRamDevice == true

        val refreshRate = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            windowManager?.defaultDisplay?.let { display ->
                if (display.supportedModes.isNotEmpty()) {
                    display.supportedModes.maxOfOrNull { it.refreshRate } ?: 60f
                } else 60f
            } ?: 60f
        } else 60f

        val hasHaptics = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            context.getSystemService(Context.VIBRATOR_SERVICE)?.let {
                (it as? android.os.Vibrator)?.hasVibrator() == true
            } ?: false
        } else {
            @Suppress("DEPRECATION")
            (context.getSystemService(Context.VIBRATOR_SERVICE) as? android.os.Vibrator)?.hasVibrator() == true
        }

        val config = context.resources.configuration
        val supportsDynamicColor = Build.VERSION.SDK_INT >= 31

        return AndroidCapabilities(
            sdkInt = Build.VERSION.SDK_INT,
            windowSizeClass = config.toWindowSizeClass(),
            foldPosture = FoldPosture.None,
            hasHardwareKeyboard = config.keyboard != Configuration.KEYBOARD_NOKEYS,
            pointerKinds = config.toPointerKinds(),
            refreshRateHz = refreshRate,
            isLowRamDevice = isLowRamDevice,
            hasGyroscope = hasGyroscope,
            hasAccelerometer = hasAccelerometer,
            hasHaptics = hasHaptics,
            supportsDynamicColor = supportsDynamicColor,
            supportsPredictiveBack = Build.VERSION.SDK_INT >= 34,
        )
    }

    private fun Configuration.toWindowSizeClass(): WindowSizeClass {
        val screenWidthDp = screenWidthDp
        return when {
            screenWidthDp >= 840 -> WindowSizeClass.Expanded
            screenWidthDp >= 600 -> WindowSizeClass.Medium
            else -> WindowSizeClass.Compact
        }
    }

    private fun Configuration.toPointerKinds(): Set<PointerKind> {
        val kinds = mutableSetOf(PointerKind.Touch)
        if (keyboard != Configuration.KEYBOARD_NOKEYS) {
            kinds.add(PointerKind.Mouse)
        }
        if (touchscreen == Configuration.TOUCHSCREEN_STYLUS) {
            kinds.add(PointerKind.Stylus)
        }
        return kinds
    }

    fun updateFromInputDevices(inputDeviceSources: Set<PointerKind>) {
        val current = _capabilities.value
        _capabilities.value = current.copy(pointerKinds = inputDeviceSources)
    }

    private fun List<androidx.window.layout.DisplayFeature>.toFoldPosture(): FoldPosture {
        val foldingFeature = filterIsInstance<androidx.window.layout.FoldingFeature>().firstOrNull()
            ?: return FoldPosture.None
        return when (foldingFeature.state) {
            androidx.window.layout.FoldingFeature.State.FLAT -> FoldPosture.Flat
            androidx.window.layout.FoldingFeature.State.HALF_OPENED -> FoldPosture.HalfOpened
            else -> FoldPosture.None
        }
    }
}
