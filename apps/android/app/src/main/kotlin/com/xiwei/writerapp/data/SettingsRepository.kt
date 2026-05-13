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
        return when (val result = bridge.getLocalSettings()) {
            is NativeResult.Success -> result.data ?: LocalSettings()
            is NativeResult.Error -> {
                // Log error but provide safe fallback for startup
                System.err.println("加载本地设置失败: ${result.message}")
                LocalSettings()
            }
            NativeResult.NotLoaded -> fallbackBridge.getLocalSettings()
        }
    }

    fun saveLocalSettings(settings: LocalSettings): Boolean {
        return when (val result = bridge.saveLocalSettings(settings)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> {
                System.err.println("保存本地设置失败: ${result.message}")
                false
            }
            NativeResult.NotLoaded -> fallbackBridge.saveLocalSettings(settings)
        }
    }
}
