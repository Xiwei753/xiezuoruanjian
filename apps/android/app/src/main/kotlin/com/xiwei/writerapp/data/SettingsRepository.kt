package com.xiwei.writerapp.data

import android.content.Context
import com.xiwei.writerapp.model.LocalSettings

class SettingsRepository(context: Context) {
    private val bridge = NativeCoreBridge(context)

    fun getLocalSettings(): LocalSettings {
        return when (val result = bridge.getLocalSettings()) {
            is NativeResult.Success -> result.data ?: LocalSettings()
            is NativeResult.Error -> {
                System.err.println("加载本地设置失败: ${result.message}")
                LocalSettings()
            }
            NativeResult.NotLoaded -> LocalSettings()
        }
    }

    fun saveLocalSettings(settings: LocalSettings): Boolean {
        return when (val result = bridge.saveLocalSettings(settings)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> {
                System.err.println("保存本地设置失败: ${result.message}")
                false
            }
            NativeResult.NotLoaded -> false
        }
    }
}
