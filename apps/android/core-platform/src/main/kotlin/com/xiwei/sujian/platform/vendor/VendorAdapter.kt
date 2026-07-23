package com.xiwei.sujian.platform.vendor

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
}
