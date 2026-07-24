package com.xiwei.sujian.platform.window

import android.content.Context
import android.hardware.input.InputManager
import android.view.InputDevice
import com.xiwei.sujian.platform.api.PointerKind

data class PointerAndKeyboardState(
    val hasHardwareKeyboard: Boolean = false,
    val hasStylus: Boolean = false,
    val hasMouse: Boolean = false,
    val hasTrackpad: Boolean = false,
)

fun detectPointerKindsFromInputDevices(context: Context): Set<PointerKind> {
    val kinds = mutableSetOf<PointerKind>()
    val inputManager = context.getSystemService(Context.INPUT_SERVICE) as? InputManager
    val deviceIds = inputManager?.inputDeviceIds ?: return setOf(PointerKind.Touch)
    for (id in deviceIds) {
        val device = inputManager.getInputDevice(id) ?: continue
        val sources = device.sources
        if (sources and InputDevice.SOURCE_MOUSE == InputDevice.SOURCE_MOUSE ||
            sources and InputDevice.SOURCE_MOUSE_RELATIVE == InputDevice.SOURCE_MOUSE_RELATIVE
        ) {
            kinds.add(PointerKind.Mouse)
        }
        if (sources and InputDevice.SOURCE_TOUCHPAD == InputDevice.SOURCE_TOUCHPAD) {
            kinds.add(PointerKind.Trackpad)
        }
        if (sources and InputDevice.SOURCE_STYLUS == InputDevice.SOURCE_STYLUS ||
            sources and InputDevice.SOURCE_BLUETOOTH_STYLUS == InputDevice.SOURCE_BLUETOOTH_STYLUS
        ) {
            kinds.add(PointerKind.Stylus)
        }
        if (sources and InputDevice.SOURCE_TOUCHSCREEN == InputDevice.SOURCE_TOUCHSCREEN) {
            kinds.add(PointerKind.Touch)
        }
    }
    return kinds.ifEmpty { setOf(PointerKind.Touch) }
}
