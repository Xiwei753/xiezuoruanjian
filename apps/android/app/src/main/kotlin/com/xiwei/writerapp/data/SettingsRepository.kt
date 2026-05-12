package com.xiwei.writerapp.data

import android.content.Context
import com.xiwei.writerapp.model.LocalSettings

/**
 * A thin repository layer for the UI to interact with settings.
 *
 * It delegates all settings logic to the underlying bridge/facade.
 * Under no circumstances should this class construct file paths or understand
 * the settings format.
 */
class SettingsRepository(context: Context) {
    private val bridge = NativeCoreBridge(context)
    private val fallbackBridge = TemporarySettingsBridge(context)

    fun getLocalSettings(): LocalSettings {
        // Here we just use native first. It falls back to default LocalSettings on failure anyway.
        val nativeSettings = bridge.getLocalSettings()
        if (nativeSettings != null) {
            return nativeSettings
        }
        return fallbackBridge.getLocalSettings()
    }

    fun saveLocalSettings(settings: LocalSettings): Boolean {
        if (bridge.isLoaded) {
            val success = bridge.saveLocalSettings(settings)
            if (success) return true
        }
        return fallbackBridge.saveLocalSettings(settings)
    }
}
