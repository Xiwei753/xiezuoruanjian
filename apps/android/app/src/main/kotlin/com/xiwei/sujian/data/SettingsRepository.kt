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
 * - Compose SettingsRoute 保存用户设置
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

    fun saveLocalSettings(settings: LocalSettings): SettingsSaveResult {
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
                SettingsSaveResult.Success
            }
            is BridgeResult.Error -> {
                warn("Failed to save local settings: ${result.message}")
                SettingsSaveResult.Failed(SaveField.LOCAL_SETTINGS)
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(SaveField.LOCAL_SETTINGS)
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

    fun saveSyncableSettings(settings: SyncableSettings): SettingsSaveResult {
        return when (val result = settingsBridge.saveSyncableSettings(settings)) {
            is BridgeResult.Success -> {
                CoreSettingsEvents.record(result.envelope)
                SettingsSaveResult.Success
            }
            is BridgeResult.Error -> {
                warn("Failed to save syncable settings: ${result.message}")
                SettingsSaveResult.Failed(SaveField.FONT_SIZE)
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(SaveField.FONT_SIZE)
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

    fun setFontSize(fontSize: Float): SettingsSaveResult {
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

    fun saveSyncConfig(config: SyncConfig): SettingsSaveResult {
        return when (val result = syncBridge.saveSyncConfig(config)) {
            is BridgeResult.Success -> {
                AutoSyncScheduler.scheduleFromSettings(appContext)
                SettingsSaveResult.Success
            }
            is BridgeResult.Error -> {
                warn("Failed to save sync config: ${result.message}")
                SettingsSaveResult.Failed(SaveField.SYNC_CONFIG)
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(SaveField.SYNC_CONFIG)
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

    fun saveSyncSecrets(secrets: SyncSecrets): SettingsSaveResult {
        return when (val result = syncBridge.saveSyncSecrets(secrets)) {
            is BridgeResult.Success -> SettingsSaveResult.Success
            is BridgeResult.Error -> {
                warn("Failed to save sync secrets: ${result.message}")
                SettingsSaveResult.Failed(SaveField.SYNC_SECRETS)
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(SaveField.SYNC_SECRETS)
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
    @Deprecated("Use loadDeviceInfo(): DeviceInfo from Core instead", replaceWith = ReplaceWith("loadDeviceInfo()"))
    fun saveDeviceInfo(deviceInfo: Map<String, String>) {
        devicePrefs.edit().apply {
            deviceInfo.forEach { (key, value) -> putString(key, value) }
            apply()
        }
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

    fun listBuiltinThemes(): List<uniffi.writer_core.BuiltinThemeDto> {
        return try {
            settingsBridge.listBuiltinThemes()
        } catch (e: Exception) {
            DiagnosticsLogger.w("SettingsRepository", "Failed to list builtin themes", e)
            emptyList()
        }
    }

    fun listPaletteRecords(): List<uniffi.writer_core.ThemePaletteRecordDto> {
        return when (val result = settingsBridge.listPaletteRecords()) {
            is BridgeResult.Success -> result.data ?: emptyList()
            is BridgeResult.Error -> {
                warn("Failed to list palette records: ${result.message}")
                emptyList()
            }
            BridgeResult.NotLoaded -> emptyList()
        }
    }

    fun loadPaletteRecord(deviceId: String, fingerprint: String): uniffi.writer_core.ThemePaletteRecordDto? {
        return when (val result = settingsBridge.loadPaletteRecord(deviceId, fingerprint)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load palette record: ${result.message}")
                null
            }
            BridgeResult.NotLoaded -> null
        }
    }

    fun deletePaletteRecord(deviceId: String, fingerprint: String): Boolean {
        return when (val result = settingsBridge.deletePaletteRecord(deviceId, fingerprint)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to delete palette record: ${result.message}")
                false
            }
            BridgeResult.NotLoaded -> false
        }
    }

    fun saveDynamicColorPaletteToCatalog(
        lightScheme: uniffi.writer_core.ThemeColorSchemeDto,
        darkScheme: uniffi.writer_core.ThemeColorSchemeDto,
        deviceClass: String? = null
    ) {
        try {
            val deviceInfo = loadDeviceInfo()
            val deviceId = deviceInfo.deviceId.ifEmpty { "legacy" }
            val effectiveDeviceClass = deviceClass
                ?: deviceInfo.deviceClass.ifEmpty { detectDeviceClass() }

            val fingerprint = settingsBridge.computePaletteFingerprint(lightScheme, darkScheme)
            val paletteId = "$deviceId:$fingerprint"

            val record = uniffi.writer_core.ThemePaletteRecordDto(
                schemaVersion = 1u,
                paletteId = paletteId,
                paletteFingerprint = fingerprint,
                source = "android_dynamic_color",
                sourcePlatform = "android",
                sourceDeviceId = deviceId,
                sourceDeviceClass = effectiveDeviceClass,
                capturedAtMs = System.currentTimeMillis(),
                variant = "system_selected",
                lightScheme = lightScheme,
                darkScheme = darkScheme
            )

            settingsBridge.savePaletteRecord(record)
        } catch (e: Exception) {
            DiagnosticsLogger.w("SettingsRepository", "Failed to save palette to catalog", e)
        }
    }

    private fun detectDeviceClass(): String {
        val config = appContext.resources?.configuration ?: return "phone"
        val smallestWidthDp = config.smallestScreenWidthDp
        return when {
            smallestWidthDp >= 600 -> "tablet"
            else -> "phone"
        }
    }

    fun detectDeviceClassFromFoldFeature(hasFoldFeature: Boolean, smallestWidthDp: Int): String {
        return when {
            hasFoldFeature -> "foldable"
            smallestWidthDp >= 600 -> "tablet"
            else -> "phone"
        }
    }

}
