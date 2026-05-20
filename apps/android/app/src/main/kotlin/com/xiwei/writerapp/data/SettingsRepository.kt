package com.xiwei.writerapp.data

import android.content.Context
import com.xiwei.writerapp.model.*
import com.xiwei.writerapp.model.LocalSettings
import com.xiwei.writerapp.model.SyncableSettings

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

    fun getSyncableSettings(): SyncableSettings {
        return when (val result = bridge.getSyncableSettings()) {
            is NativeResult.Success -> result.data ?: SyncableSettings()
            is NativeResult.Error -> {
                System.err.println("加载同步设置失败: ${result.message}")
                SyncableSettings()
            }
            NativeResult.NotLoaded -> SyncableSettings()
        }
    }

    fun saveSyncableSettings(settings: SyncableSettings): Boolean {
        return when (val result = bridge.saveSyncableSettings(settings)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> {
                System.err.println("保存同步设置失败: ${result.message}")
                false
            }
            NativeResult.NotLoaded -> false
        }
    }

    fun getEffectiveFontSize(): Float {
        val syncable = getSyncableSettings()
        if (syncable.fontSize > 0.0) {
            return syncable.fontSize.toFloat()
        }
        val local = getLocalSettings()
        if (local.editorFontSize > 0.0f) {
            return local.editorFontSize
        }
        return 16f
    }

    fun setFontSize(fontSize: Float): Boolean {
        val syncable = getSyncableSettings()
        return saveSyncableSettings(syncable.copy(fontSize = fontSize.toDouble()))
    }

    fun loadSyncState(): SyncState {
        return when (val result = bridge.loadSyncState()) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> {
                System.err.println("加载同步状态失败: ${result.message}")
                SyncState()
            }
            NativeResult.NotLoaded -> SyncState()
        }
    }

    fun loadSyncConfig(): SyncConfig {
        return when (val result = bridge.loadSyncConfig()) {
            is NativeResult.Success -> result.data.normalize()
            is NativeResult.Error -> {
                System.err.println("加载同步配置失败: ${result.message}")
                SyncConfig().normalize()
            }
            NativeResult.NotLoaded -> SyncConfig().normalize()
        }
    }

    fun saveSyncConfig(config: SyncConfig): Boolean {
        return when (val result = bridge.saveSyncConfig(config)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> {
                System.err.println("保存同步配置失败: ${result.message}")
                false
            }
            NativeResult.NotLoaded -> false
        }
    }

    fun loadSyncSecrets(): SyncSecrets {
        return when (val result = bridge.loadSyncSecrets()) {
            is NativeResult.Success -> result.data ?: SyncSecrets()
            is NativeResult.Error -> {
                System.err.println("加载同步密钥失败: ${result.message}")
                SyncSecrets()
            }
            NativeResult.NotLoaded -> SyncSecrets()
        }
    }

    fun saveSyncSecrets(secrets: SyncSecrets): Boolean {
        return when (val result = bridge.saveSyncSecrets(secrets)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> {
                System.err.println("保存同步密钥失败: ${result.message}")
                false
            }
            NativeResult.NotLoaded -> false
        }
    }

    fun performSyncDiagnostics(config: SyncConfig): NativeResult<SyncDiagnosticsResult> {
        return bridge.performSyncDiagnostics(config)
    }

    fun performSyncDryRun(config: SyncConfig): NativeResult<SyncPlan> {
        return bridge.performSyncDryRun(config)
    }

    fun performSync(config: SyncConfig): NativeResult<SyncResult> {
        return bridge.performSync(config)
    }

}
