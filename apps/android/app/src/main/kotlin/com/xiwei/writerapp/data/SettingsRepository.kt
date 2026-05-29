package com.xiwei.writerapp.data

import android.content.Context
import com.xiwei.writerapp.model.*
import com.xiwei.writerapp.model.LocalSettings
import com.xiwei.writerapp.model.SyncableSettings

/**
 * SettingsRepository — 设置仓库层
 *
 * 对设置、同步、native 状态领域 Bridge 的封装，提供统一的设置读写接口。
 *
 * ## 架构定位
 * - ViewModel/Activity → SettingsRepository → SettingsBridge/SyncBridge → legacy internal adapter → JNI → Rust Core
 *
 * ## 职责边界
 * - **做**：加载/保存本地设置、可同步设置、同步配置和密钥
 * - **不做**：业务逻辑（只做类型转换和错误处理）
 *
 * ## 使用场景
 * - EditorViewModel 加载编辑器设置
 * - SettingsActivity 保存用户设置
 * - SyncPage 加载/保存同步配置
 */
class SettingsRepository(context: Context) {
    private val settingsBridge = BridgeProvider.getSettingsBridge(context)
    private val syncBridge = BridgeProvider.getSyncBridge(context)
    private val nativeStatusBridge = BridgeProvider.getNativeStatusBridge(context)

    fun getLocalSettings(): LocalSettings {
        return when (val result = settingsBridge.getLocalSettings()) {
            is BridgeResult.Success -> result.data ?: LocalSettings()
            is BridgeResult.Error -> {
                System.err.println("加载本地设置失败: ${result.message}")
                LocalSettings()
            }
            BridgeResult.NotLoaded -> LocalSettings()
        }
    }

    fun saveLocalSettings(settings: LocalSettings): Boolean {
        return when (val result = settingsBridge.saveLocalSettings(settings)) {
            is BridgeResult.Success -> {
                SettingsChangeBus.notifyChanged()
                result.data
            }
            is BridgeResult.Error -> {
                System.err.println("保存本地设置失败: ${result.message}")
                false
            }
            BridgeResult.NotLoaded -> false
        }
    }

    fun getSyncableSettings(): SyncableSettings {
        return when (val result = settingsBridge.getSyncableSettings()) {
            is BridgeResult.Success -> result.data ?: SyncableSettings()
            is BridgeResult.Error -> {
                System.err.println("加载同步设置失败: ${result.message}")
                val defaultSettings = SyncableSettings()
                defaultSettings
            }
            BridgeResult.NotLoaded -> SyncableSettings()
        }
    }

    fun saveSyncableSettings(settings: SyncableSettings): Boolean {
        return when (val result = settingsBridge.saveSyncableSettings(settings)) {
            is BridgeResult.Success -> {
                SettingsChangeBus.notifyChanged()
                result.data
            }
            is BridgeResult.Error -> {
                System.err.println("保存同步设置失败: ${result.message}")
                false
            }
            BridgeResult.NotLoaded -> false
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
        return when (val result = syncBridge.loadSyncState()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                System.err.println("加载同步状态失败: ${result.message}")
                SyncState()
            }
            BridgeResult.NotLoaded -> SyncState()
        }
    }

    fun loadSyncConfig(): SyncConfig {
        return when (val result = syncBridge.loadSyncConfig()) {
            is BridgeResult.Success -> result.data.normalize()
            is BridgeResult.Error -> {
                System.err.println("加载同步配置失败: ${result.message}")
                SyncConfig().normalize()
            }
            BridgeResult.NotLoaded -> SyncConfig().normalize()
        }
    }

    fun saveSyncConfig(config: SyncConfig): Boolean {
        return when (val result = syncBridge.saveSyncConfig(config)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                System.err.println("保存同步配置失败: ${result.message}")
                false
            }
            BridgeResult.NotLoaded -> false
        }
    }

    fun loadSyncSecrets(): SyncSecrets {
        return when (val result = syncBridge.loadSyncSecrets()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                System.err.println("加载同步密钥失败: ${result.message}")
                SyncSecrets()
            }
            BridgeResult.NotLoaded -> SyncSecrets()
        }
    }

    fun saveSyncSecrets(secrets: SyncSecrets): Boolean {
        return when (val result = syncBridge.saveSyncSecrets(secrets)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                System.err.println("保存同步密钥失败: ${result.message}")
                false
            }
            BridgeResult.NotLoaded -> false
        }
    }

    fun aiAvailable(): Boolean {
        return nativeStatusBridge.aiAvailable()
    }

    fun workspaceDir(): String {
        return nativeStatusBridge.workspaceDir
    }

    fun performSyncDiagnostics(config: SyncConfig): BridgeResult<SyncDiagnosticsResult> {
        return syncBridge.performSyncDiagnostics(config)
    }

    fun performSyncDryRun(config: SyncConfig): BridgeResult<SyncPlan> {
        return syncBridge.performSyncDryRun(config)
    }

    fun performSync(config: SyncConfig): BridgeResult<SyncResult> {
        return syncBridge.performSync(config)
    }

}
