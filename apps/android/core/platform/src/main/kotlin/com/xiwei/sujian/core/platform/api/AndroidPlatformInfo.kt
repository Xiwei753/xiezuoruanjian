package com.xiwei.sujian.core.platform.api

import android.os.Build

data class AndroidPlatformInfo(
    val manufacturer: String = Build.MANUFACTURER,
    val brand: String = Build.BRAND,
    val model: String = Build.MODEL,
    val device: String = Build.DEVICE,
    val sdkInt: Int = Build.VERSION.SDK_INT,
    val release: String = Build.VERSION.RELEASE,
    val abi: String = Build.SUPPORTED_ABIS.firstOrNull() ?: "unknown",
)
