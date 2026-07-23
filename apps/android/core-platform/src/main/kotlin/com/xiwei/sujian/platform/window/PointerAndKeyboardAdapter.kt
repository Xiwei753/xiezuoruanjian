package com.xiwei.sujian.platform.window

import android.content.res.Configuration

data class PointerAndKeyboardState(
    val hasHardwareKeyboard: Boolean = false,
    val hasStylus: Boolean = false,
    val hasMouse: Boolean = false,
    val hasTrackpad: Boolean = false,
)

fun Configuration.toPointerAndKeyboardState(): PointerAndKeyboardState {
    return PointerAndKeyboardState(
        hasHardwareKeyboard = keyboard != Configuration.KEYBOARD_NOKEYS,
        hasStylus = touchscreen == Configuration.TOUCHSCREEN_STYLUS,
        hasMouse = navigation == Configuration.NAVIGATION_TRACKBALL ||
            navigation == Configuration.NAVIGATION_DPAD,
        hasTrackpad = keyboard != Configuration.KEYBOARD_NOKEYS,
    )
}
