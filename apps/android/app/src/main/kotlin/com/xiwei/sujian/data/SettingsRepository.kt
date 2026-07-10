package com.xiwei.sujian.data

import android.content.Context
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.model.*
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.model.SyncableSettings

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
    private val appContext = context.applicationContext
    private val settingsBridge = BridgeProvider.getSettingsBridge(context)
    private val syncBridge = BridgeProvider.getSyncBridge(context)
    private val statsBridge = BridgeProvider.getStatsBridge(context)
    private val diagPrefs = appContext.getSharedPreferences("sujian_diagnostics", android.content.Context.MODE_PRIVATE)
    private val devicePrefs = appContext.getSharedPreferences("sujian_device", android.content.Context.MODE_PRIVATE)

    @Volatile
    var lastWarning: String? = null
        private set

    fun consumeWarning(): String? {
        val w = lastWarning
        lastWarning = null
        return w
    }

    private fun warn(msg: String) {
        lastWarning = msg
        DiagnosticsLogger.w("SettingsRepository", msg)
    }

    fun getLocalSettings(): LocalSettings {
        val fromCore = when (val result = settingsBridge.getLocalSettings()) {
            is BridgeResult.Success -> result.data ?: LocalSettings()
            is BridgeResult.Error -> {
                warn("Failed to load local settings: ${result.message}")
                LocalSettings()
            }
            BridgeResult.NotLoaded -> LocalSettings()
        }
        return fromCore.copy(
            diagnosticsEnabled = diagPrefs.getBoolean("diagnostics_enabled", true),
            diagnosticsVerbose = diagPrefs.getBoolean("diagnostics_verbose", true),
            useSelfRenderEditorOnAndroid = diagPrefs.getBoolean("use_self_render_editor_on_android", true),
            experimentalFullscreenMode = diagPrefs.getBoolean("experimental_fullscreen_mode", false)
        )
    }

    fun saveLocalSettings(settings: LocalSettings): Boolean {
        val effectiveVerbose = if (settings.diagnosticsEnabled) settings.diagnosticsVerbose else false
        diagPrefs.edit()
            .putBoolean("diagnostics_enabled", settings.diagnosticsEnabled)
            .putBoolean("diagnostics_verbose", effectiveVerbose)
            .putBoolean("use_self_render_editor_on_android", settings.useSelfRenderEditorOnAndroid)
            .putBoolean("experimental_fullscreen_mode", settings.experimentalFullscreenMode)
            .apply()
        val coreSettings = settings.copy(diagnosticsEnabled = false, diagnosticsVerbose = false, useSelfRenderEditorOnAndroid = false, experimentalFullscreenMode = false)
        return when (val result = settingsBridge.saveLocalSettings(coreSettings)) {
            is BridgeResult.Success -> {
                CoreSettingsEvents.record(result.envelope)
                result.data
            }
            is BridgeResult.Error -> {
                warn("Failed to save local settings: ${result.message}")
                false
            }
            BridgeResult.NotLoaded -> false
        }
    }

    fun getSyncableSettings(): SyncableSettings {
        return when (val result = settingsBridge.getSyncableSettings()) {
            is BridgeResult.Success -> result.data ?: SyncableSettings()
            is BridgeResult.Error -> {
                warn("Failed to load syncable settings: ${result.message}")
                val defaultSettings = SyncableSettings()
                defaultSettings
            }
            BridgeResult.NotLoaded -> SyncableSettings()
        }
    }

    fun saveSyncableSettings(settings: SyncableSettings): Boolean {
        return when (val result = settingsBridge.saveSyncableSettings(settings)) {
            is BridgeResult.Success -> {
                CoreSettingsEvents.record(result.envelope)
                result.data
            }
            is BridgeResult.Error -> {
                warn("Failed to save syncable settings: ${result.message}")
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
                warn("Failed to load sync state: ${result.message}")
                SyncState()
            }
            BridgeResult.NotLoaded -> SyncState()
        }
    }

    fun loadSyncConfig(): SyncConfig {
        return when (val result = syncBridge.loadSyncConfig()) {
            is BridgeResult.Success -> result.data.normalize()
            is BridgeResult.Error -> {
                warn("Failed to load sync config: ${result.message}")
                SyncConfig().normalize()
            }
            BridgeResult.NotLoaded -> SyncConfig().normalize()
        }
    }

    fun saveSyncConfig(config: SyncConfig): Boolean {
        return when (val result = syncBridge.saveSyncConfig(config)) {
            is BridgeResult.Success -> {
                AutoSyncScheduler.scheduleFromSettings(appContext)
                result.data
            }
            is BridgeResult.Error -> {
                warn("Failed to save sync config: ${result.message}")
                false
            }
            BridgeResult.NotLoaded -> false
        }
    }

    fun loadSyncSecrets(): SyncSecrets {
        return when (val result = syncBridge.loadSyncSecrets()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load sync secrets: ${result.message}")
                SyncSecrets()
            }
            BridgeResult.NotLoaded -> SyncSecrets()
        }
    }

    fun saveSyncSecrets(secrets: SyncSecrets): Boolean {
        return when (val result = syncBridge.saveSyncSecrets(secrets)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to save sync secrets: ${result.message}")
                false
            }
            BridgeResult.NotLoaded -> false
        }
    }

    fun aiAvailable(): Boolean {
        return BridgeProvider.getAiStatus(appContext)
    }

    fun workspaceDir(): String {
        return WorkspaceManager.getWorkspaceDir(appContext).absolutePath
    }

    fun performSyncDiagnostics(config: SyncConfig): BridgeResult<SyncDiagnosticsResult> {
        return syncBridge.performSyncDiagnostics(config)
    }

    fun performSyncDryRun(config: SyncConfig): BridgeResult<SyncPlan> {
        return syncBridge.performSyncDryRun(config)
    }

    fun performSync(config: SyncConfig, forceSync: Boolean = false): BridgeResult<SyncResult> {
        return syncBridge.performSync(config, forceSync)
    }

    fun getSyncCapability(): SyncCapabilityData {
        return when (val result = syncBridge.getSyncCapability()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to get sync capability: ${result.message}")
                SyncCapabilityData()
            }
            BridgeResult.NotLoaded -> SyncCapabilityData()
        }
    }

    /**
     * 保存设备信息到本地 SharedPreferences。
     * 包含 deviceId、deviceClass、platform，供同步和统计使用。
     */
    fun saveDeviceInfo(deviceInfo: Map<String, String>) {
        devicePrefs.edit().apply {
            deviceInfo.forEach { (key, value) -> putString(key, value) }
            apply()
        }
    }

    /**
     * 加载已保存的设备信息。
     */
    fun loadDeviceInfo(): Map<String, String> {
        return mapOf(
            "deviceId" to (devicePrefs.getString("deviceId", null) ?: ""),
            "deviceClass" to (devicePrefs.getString("deviceClass", null) ?: ""),
            "platform" to (devicePrefs.getString("platform", null) ?: "android")
        )
    }

    /**
     * Flush 写作统计到磁盘，确保同步前数据已持久化。
     */
    fun flushWritingStats() {
        statsBridge.flushWritingStats()
    }

    /**
     * 确保设备信息已写入 app-meta/device/current_device.json。
     * 通过 Core 层 ensure_device_info 实现，不依赖 SharedPreferences。
     */
    fun ensureDeviceInfo(platform: String, deviceClass: String): Boolean {
        return when (val result = settingsBridge.ensureDeviceInfo(platform, deviceClass)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to write device info: ${result.message}")
                false
            }
            BridgeResult.NotLoaded -> false
        }
    }

    fun loadDeviceInfo(): com.xiwei.sujian.model.DeviceInfo {
        return when (val result = settingsBridge.loadDeviceInfo()) {
            is BridgeResult.Success -> result.data
            else -> com.xiwei.sujian.model.DeviceInfo()
        }
    }

    fun saveDynamicColorPaletteToCatalog(paletteJson: String) {
        try {
            val paletteObj = com.google.gson.JsonParser.parseString(paletteJson).asJsonObject
            val deviceId = paletteObj.get("device_id")?.asString ?: ""
            val source = paletteObj.get("source")?.asString ?: "android_dynamic_color"
            val variant = paletteObj.get("variant")?.asString ?: "system_selected"
            val updatedAtMs = paletteObj.get("updated_at_ms")?.asLong ?: System.currentTimeMillis()

            val lightScheme = uniffi.writer_core.ThemeColorSchemeDto(
                primary = paletteObj.get("light_primary")?.asString ?: "",
                on_primary = paletteObj.get("light_on_primary")?.asString ?: "",
                primary_container = paletteObj.get("light_primary_container")?.asString ?: "",
                on_primary_container = paletteObj.get("light_on_primary_container")?.asString ?: "",
                inverse_primary = paletteObj.get("light_inverse_primary")?.asString ?: "",
                secondary = paletteObj.get("light_secondary")?.asString ?: "",
                on_secondary = paletteObj.get("light_on_secondary")?.asString ?: "",
                secondary_container = paletteObj.get("light_secondary_container")?.asString ?: "",
                on_secondary_container = paletteObj.get("light_on_secondary_container")?.asString ?: "",
                tertiary = paletteObj.get("light_tertiary")?.asString ?: "",
                on_tertiary = paletteObj.get("light_on_tertiary")?.asString ?: "",
                tertiary_container = paletteObj.get("light_tertiary_container")?.asString ?: "",
                on_tertiary_container = paletteObj.get("light_on_tertiary_container")?.asString ?: "",
                background = paletteObj.get("light_background")?.asString ?: "",
                on_background = paletteObj.get("light_on_background")?.asString ?: "",
                surface = paletteObj.get("light_surface")?.asString ?: "",
                on_surface = paletteObj.get("light_on_surface")?.asString ?: "",
                surface_variant = paletteObj.get("light_surface_variant")?.asString ?: "",
                on_surface_variant = paletteObj.get("light_on_surface_variant")?.asString ?: "",
                surface_tint = paletteObj.get("light_surface_tint")?.asString ?: "",
                surface_dim = paletteObj.get("light_surface_dim")?.asString ?: "",
                surface_bright = paletteObj.get("light_surface_bright")?.asString ?: "",
                surface_container_lowest = paletteObj.get("light_surface_container_lowest")?.asString ?: "",
                surface_container_low = paletteObj.get("light_surface_container_low")?.asString ?: "",
                surface_container = paletteObj.get("light_surface_container")?.asString ?: "",
                surface_container_high = paletteObj.get("light_surface_container_high")?.asString ?: "",
                surface_container_highest = paletteObj.get("light_surface_container_highest")?.asString ?: "",
                inverse_surface = paletteObj.get("light_inverse_surface")?.asString ?: "",
                inverse_on_surface = paletteObj.get("light_inverse_on_surface")?.asString ?: "",
                error = paletteObj.get("light_error")?.asString ?: "",
                on_error = paletteObj.get("light_on_error")?.asString ?: "",
                error_container = paletteObj.get("light_error_container")?.asString ?: "",
                on_error_container = paletteObj.get("light_on_error_container")?.asString ?: "",
                outline = paletteObj.get("light_outline")?.asString ?: "",
                outline_variant = paletteObj.get("light_outline_variant")?.asString ?: "",
                scrim = paletteObj.get("light_scrim")?.asString ?: ""
            )
            val darkScheme = uniffi.writer_core.ThemeColorSchemeDto(
                primary = paletteObj.get("dark_primary")?.asString ?: "",
                on_primary = paletteObj.get("dark_on_primary")?.asString ?: "",
                primary_container = paletteObj.get("dark_primary_container")?.asString ?: "",
                on_primary_container = paletteObj.get("dark_on_primary_container")?.asString ?: "",
                inverse_primary = paletteObj.get("dark_inverse_primary")?.asString ?: "",
                secondary = paletteObj.get("dark_secondary")?.asString ?: "",
                on_secondary = paletteObj.get("dark_on_secondary")?.asString ?: "",
                secondary_container = paletteObj.get("dark_secondary_container")?.asString ?: "",
                on_secondary_container = paletteObj.get("dark_on_secondary_container")?.asString ?: "",
                tertiary = paletteObj.get("dark_tertiary")?.asString ?: "",
                on_tertiary = paletteObj.get("dark_on_tertiary")?.asString ?: "",
                tertiary_container = paletteObj.get("dark_tertiary_container")?.asString ?: "",
                on_tertiary_container = paletteObj.get("dark_on_tertiary_container")?.asString ?: "",
                background = paletteObj.get("dark_background")?.asString ?: "",
                on_background = paletteObj.get("dark_on_background")?.asString ?: "",
                surface = paletteObj.get("dark_surface")?.asString ?: "",
                on_surface = paletteObj.get("dark_on_surface")?.asString ?: "",
                surface_variant = paletteObj.get("dark_surface_variant")?.asString ?: "",
                on_surface_variant = paletteObj.get("dark_on_surface_variant")?.asString ?: "",
                surface_tint = paletteObj.get("dark_surface_tint")?.asString ?: "",
                surface_dim = paletteObj.get("dark_surface_dim")?.asString ?: "",
                surface_bright = paletteObj.get("dark_surface_bright")?.asString ?: "",
                surface_container_lowest = paletteObj.get("dark_surface_container_lowest")?.asString ?: "",
                surface_container_low = paletteObj.get("dark_surface_container_low")?.asString ?: "",
                surface_container = paletteObj.get("dark_surface_container")?.asString ?: "",
                surface_container_high = paletteObj.get("dark_surface_container_high")?.asString ?: "",
                surface_container_highest = paletteObj.get("dark_surface_container_highest")?.asString ?: "",
                inverse_surface = paletteObj.get("dark_inverse_surface")?.asString ?: "",
                inverse_on_surface = paletteObj.get("dark_inverse_on_surface")?.asString ?: "",
                error = paletteObj.get("dark_error")?.asString ?: "",
                on_error = paletteObj.get("dark_on_error")?.asString ?: "",
                error_container = paletteObj.get("dark_error_container")?.asString ?: "",
                on_error_container = paletteObj.get("dark_on_error_container")?.asString ?: "",
                outline = paletteObj.get("dark_outline")?.asString ?: "",
                outline_variant = paletteObj.get("dark_outline_variant")?.asString ?: "",
                scrim = paletteObj.get("dark_scrim")?.asString ?: ""
            )

            val fingerprint = settingsBridge.computePaletteFingerprint(lightScheme, darkScheme)
            val effectiveDeviceId = if (deviceId.isEmpty()) "legacy" else deviceId
            val paletteId = "$effectiveDeviceId:$fingerprint"

            val record = uniffi.writer_core.ThemePaletteRecordDto(
                schema_version = 1u,
                palette_id = paletteId,
                palette_fingerprint = fingerprint,
                source = source,
                source_platform = "android",
                source_device_id = effectiveDeviceId,
                source_device_class = "phone",
                captured_at_ms = updatedAtMs,
                variant = variant,
                light_scheme = lightScheme,
                dark_scheme = darkScheme
            )

            settingsBridge.savePaletteRecord(record)

            val local = getLocalSettings()
            if (local.selectedPaletteId.isEmpty()) {
                saveLocalSettings(local.copy(
                    selectedPaletteId = paletteId,
                    colorSource = "saved_palette"
                ))
            }
        } catch (e: Exception) {
            DiagnosticsLogger.w("SettingsRepository", "Failed to save palette to catalog", e)
        }
    }

}
