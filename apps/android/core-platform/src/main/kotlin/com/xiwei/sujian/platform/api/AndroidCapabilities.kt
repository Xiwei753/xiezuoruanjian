package com.xiwei.sujian.platform.api

import android.os.Build

enum class FoldPosture {
    None,
    Flat,
    HalfOpened,
}

enum class PointerKind {
    Touch,
    Stylus,
    Mouse,
    Trackpad,
}

enum class WindowSizeClass {
    Compact,
    Medium,
    Expanded,
}

data class AndroidCapabilities(
    val sdkInt: Int = Build.VERSION.SDK_INT,
    val windowSizeClass: WindowSizeClass = WindowSizeClass.Compact,
    val foldPosture: FoldPosture = FoldPosture.None,
    val hasHardwareKeyboard: Boolean = false,
    val pointerKinds: Set<PointerKind> = setOf(PointerKind.Touch),
    val refreshRateHz: Float = 60f,
    val isLowRamDevice: Boolean = false,
    val hasGyroscope: Boolean = false,
    val hasAccelerometer: Boolean = false,
    val hasHaptics: Boolean = false,
    val supportsDynamicColor: Boolean = Build.VERSION.SDK_INT >= 31,
    val supportsPredictiveBack: Boolean = Build.VERSION.SDK_INT >= 34,
)
