package com.xiwei.writerapp.data

import android.content.Context
import com.xiwei.writerapp.model.*
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
