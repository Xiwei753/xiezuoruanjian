package com.xiwei.sujian.platform.aosp

import android.os.Build
import com.xiwei.sujian.platform.vendor.VendorAdapter
import com.xiwei.sujian.platform.vendor.VendorAdapterRegistry

class AospVendorAdapter : VendorAdapter {
    override val vendorId: String = "aosp"
    override val displayName: String = "AOSP"

    override fun supportsFeature(feature: String): Boolean = when (feature) {
        VendorFeatures.DYNAMIC_COLOR -> Build.VERSION.SDK_INT >= 31
        VendorFeatures.PREDICTIVE_BACK -> Build.VERSION.SDK_INT >= 34
        VendorFeatures.EDGE_TO_EDGE -> Build.VERSION.SDK_INT >= 29
        else -> false
    }
}

object VendorFeatures {
    const val DYNAMIC_COLOR = "dynamic_color"
    const val PREDICTIVE_BACK = "predictive_back"
    const val EDGE_TO_EDGE = "edge_to_edge"
    const val NEARBY_COMMUNICATION = "nearby_communication"
    const val VENDOR_SYSTEM_SERVICE = "vendor_system_service"
    const val FOLD_POSTURE = "fold_posture"
    const val HIGH_REFRESH_RATE = "high_refresh_rate"
    const val ADVANCED_HAPTICS = "advanced_haptics"
}

object VendorAdapterSetup {
    @Volatile
    private var initialized = false

    fun ensureInitialized(registry: VendorAdapterRegistry) {
        if (initialized) return
        synchronized(this) {
            if (initialized) return
            registry.register(AospVendorAdapter())
            initialized = true
        }
    }
}
