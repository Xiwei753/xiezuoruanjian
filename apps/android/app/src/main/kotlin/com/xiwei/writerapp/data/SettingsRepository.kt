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
    private val bridge = TemporarySettingsBridge(context)

    fun getLocalSettings(): LocalSettings {
        return bridge.getLocalSettings()
    }

    fun saveLocalSettings(settings: LocalSettings): Boolean {
        return bridge.saveLocalSettings(settings)
    }
}
