package com.xiwei.sujian.platform.api

import android.app.ActivityManager
import android.content.Context
import android.content.res.Configuration
import android.hardware.SensorManager
import android.hardware.input.InputManager
import android.os.Build
import android.view.InputDevice
import android.view.WindowManager
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

open class CapabilityProvider(private val context: Context) {

    private val _capabilities = MutableStateFlow(detectCapabilities())
    val capabilities: StateFlow<AndroidCapabilities> = _capabilities.asStateFlow()

    private var inputDeviceListener: InputManager.InputDeviceListener? = null

    fun updateFromConfiguration(config: Configuration) {
        val current = _capabilities.value
        val windowSizeClass = config.toWindowSizeClass()
        val hasHardwareKeyboard = config.keyboard != Configuration.KEYBOARD_NOKEYS
        _capabilities.value = current.copy(
            windowSizeClass = windowSizeClass,
            hasHardwareKeyboard = hasHardwareKeyboard,
        )
    }

    fun updateFromFoldFeatures(features: List<androidx.window.layout.DisplayFeature>) {
        val current = _capabilities.value
        val foldPosture = features.toFoldPosture()
        _capabilities.value = current.copy(foldPosture = foldPosture)
    }

    fun registerInputDeviceListener() {
        val inputManager = context.getSystemService(Context.INPUT_SERVICE) as? InputManager ?: return
        val listener = object : InputManager.InputDeviceListener {
            override fun onInputDeviceAdded(deviceId: Int) {
                rescanInputDevices()
            }
            override fun onInputDeviceRemoved(deviceId: Int) {
                rescanInputDevices()
            }
            override fun onInputDeviceChanged(deviceId: Int) {
                rescanInputDevices()
            }
        }
        inputDeviceListener = listener
        inputManager.registerInputDeviceListener(listener, null)
    }

    fun unregisterInputDeviceListener() {
        val inputManager = context.getSystemService(Context.INPUT_SERVICE) as? InputManager ?: return
        val listener = inputDeviceListener ?: return
        inputManager.unregisterInputDeviceListener(listener)
        inputDeviceListener = null
    }

    private fun rescanInputDevices() {
        val availableKinds = com.xiwei.sujian.platform.window.detectPointerKindsFromInputDevices(context)
        val current = _capabilities.value
        val activeKind = chooseActivePointerKind(availableKinds, current.activePointerKind)
        _capabilities.value = current.copy(
            availablePointerKinds = availableKinds,
            activePointerKind = activeKind,
        )
    }

    fun updateActivePointerKind(kind: PointerKind) {
        val current = _capabilities.value
        if (current.availablePointerKinds.contains(kind)) {
            _capabilities.value = current.copy(activePointerKind = kind)
        }
    }

    fun updateFromInputDevices(inputDeviceSources: Set<PointerKind>) {
        val current = _capabilities.value
        val activeKind = chooseActivePointerKind(inputDeviceSources, current.activePointerKind)
        _capabilities.value = current.copy(
            availablePointerKinds = inputDeviceSources,
            activePointerKind = activeKind,
        )
    }

    private fun chooseActivePointerKind(
        available: Set<PointerKind>,
        currentActive: PointerKind
    ): PointerKind {
        if (available.contains(currentActive)) return currentActive
        val priority = listOf(PointerKind.Stylus, PointerKind.Mouse, PointerKind.Trackpad)
        for (kind in priority) {
            if (available.contains(kind)) return kind
        }
        return PointerKind.Touch
    }

    private fun detectCapabilities(): AndroidCapabilities {
        val sensorManager = context.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
        val activityManager = context.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager
        val windowManager = context.getSystemService(Context.WINDOW_SERVICE) as? WindowManager

        val hasGyroscope = sensorManager?.getDefaultSensor(android.hardware.Sensor.TYPE_GYROSCOPE) != null
        val hasAccelerometer = sensorManager?.getDefaultSensor(android.hardware.Sensor.TYPE_ACCELEROMETER) != null
        val isLowRamDevice = activityManager?.isLowRamDevice == true

        val (currentRefreshRate, maxRefreshRate) = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            windowManager?.defaultDisplay?.let { display ->
                val mode = display.mode
                val current = mode.refreshRate
                val max = if (display.supportedModes.isNotEmpty()) {
                    display.supportedModes.maxOfOrNull { it.refreshRate } ?: current
                } else current
                current to max
            } ?: (60f to 60f)
        } else (60f to 60f)

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
        val availablePointerKinds = com.xiwei.sujian.platform.window.detectPointerKindsFromInputDevices(context)
        val activePointerKind = chooseActivePointerKind(availablePointerKinds, PointerKind.Touch)

        return AndroidCapabilities(
            sdkInt = Build.VERSION.SDK_INT,
            windowSizeClass = config.toWindowSizeClass(),
            foldPosture = FoldPosture.None,
            hasHardwareKeyboard = config.keyboard != Configuration.KEYBOARD_NOKEYS,
            availablePointerKinds = availablePointerKinds,
            activePointerKind = activePointerKind,
            currentRefreshRateHz = currentRefreshRate,
            maxRefreshRateHz = maxRefreshRate,
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
