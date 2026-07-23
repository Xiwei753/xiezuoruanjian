package com.xiwei.sujian.platform.vendor

import android.os.Build

interface VendorAdapter {
    val vendorId: String
    val displayName: String
    fun supportsFeature(feature: String): Boolean
}

class VendorAdapterRegistry {
    private val adapters = mutableMapOf<String, VendorAdapter>()

    fun register(adapter: VendorAdapter) {
        adapters[adapter.vendorId] = adapter
    }

    fun getAdapter(vendorId: String): VendorAdapter? = adapters[vendorId]

    fun allAdapters(): Collection<VendorAdapter> = adapters.values

    fun currentVendorAdapter(): VendorAdapter? {
        val manufacturer = Build.MANUFACTURER.lowercase()
        val vendorId = when {
            manufacturer.contains("huawei") || manufacturer.contains("honor") -> "huawei"
            manufacturer.contains("xiaomi") || manufacturer.contains("redmi") || manufacturer.contains("poco") -> "xiaomi"
            manufacturer.contains("oppo") || manufacturer.contains("realme") || manufacturer.contains("oneplus") -> "oppo"
            manufacturer.contains("vivo") -> "vivo"
            manufacturer.contains("samsung") -> "samsung"
            else -> "aosp"
        }
        return getAdapter(vendorId) ?: getAdapter("aosp")
    }

    fun supportsFeature(feature: String): Boolean {
        return currentVendorAdapter()?.supportsFeature(feature) == true
    }
}
