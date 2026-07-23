package com.xiwei.sujian.platform.aosp

import android.content.Context
import android.content.res.Configuration
import com.xiwei.sujian.platform.api.CapabilityProvider

class AospCapabilityProvider(context: Context) : CapabilityProvider(context)

class AospSystemBarsAdapter(private val context: Context) {
    fun isStatusBarLight(): Boolean {
        val config = context.resources.configuration
        return (config.uiMode and Configuration.UI_MODE_NIGHT_MASK) != Configuration.UI_MODE_NIGHT_YES
    }
}
