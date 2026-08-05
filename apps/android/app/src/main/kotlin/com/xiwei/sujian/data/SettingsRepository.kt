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
class SettingsRepository(
    context: Context,
    bridge: AppServiceBridge? = null,
    preferencesSuffix: String = ""
) {
    private val appContext = context.applicationContext
    private val appBridge = bridge ?: BridgeProvider.getAppServiceBridge(context)
    private val settingsBridge = appBridge.settingsBridge
    private val syncBridge = appBridge.syncBridge
    private val statsBridge = appBridge.statsBridge
    private val diagPrefs = appContext.getSharedPreferences(
        if (preferencesSuffix.isNotEmpty()) "sujian_diagnostics_$preferencesSuffix" else "sujian_diagnostics",
        android.content.Context.MODE_PRIVATE
    )
    private val profileStore by lazy { SyncProfileStore(appContext) }
    private val configJson = com.google.gson.Gson()

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
                warn("Failed to load local settings: ${result.fullEnvelope}")
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
        val coreSettings = settings.copy(diagnosticsEnabled = false, diagnosticsVerbose = false, useSelfRenderEditorOnAndroid = false, experimentalFullscreenMode = false)
        return when (val result = settingsBridge.saveLocalSettings(coreSettings)) {
            is BridgeResult.Success -> {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.settingsSaved("local_settings", "ok")
                val effectiveVerbose = if (settings.diagnosticsEnabled) settings.diagnosticsVerbose else false
                diagPrefs.edit()
                    .putBoolean("diagnostics_enabled", settings.diagnosticsEnabled)
                    .putBoolean("diagnostics_verbose", effectiveVerbose)
                    .putBoolean("use_self_render_editor_on_android", settings.useSelfRenderEditorOnAndroid)
                    .putBoolean("experimental_fullscreen_mode", settings.experimentalFullscreenMode)
                    .apply()
                CoreSettingsEvents.record(result.envelope)
                SettingsSaveResult.Success
            }
            is BridgeResult.Error -> {
                warn("Failed to save local settings: ${result.fullEnvelope}")
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.settingsSaved("local_settings", "error")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.LOCAL_SETTINGS, 0L)))
            }
            BridgeResult.NotLoaded -> {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.settingsSaved("local_settings", "not_loaded")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.LOCAL_SETTINGS, 0L)))
            }
        }
    }

    fun getSyncableSettings(): SyncableSettings {
        return when (val result = settingsBridge.getSyncableSettings()) {
            is BridgeResult.Success -> result.data ?: SyncableSettings()
            is BridgeResult.Error -> {
                warn("Failed to load syncable settings: ${result.fullEnvelope}")
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
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.settingsSaved("font_size", "ok")
                SettingsSaveResult.Success
            }
            is BridgeResult.Error -> {
                warn("Failed to save syncable settings: ${result.fullEnvelope}")
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.settingsSaved("font_size", "error")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.FONT_SIZE, 0L)))
            }
            BridgeResult.NotLoaded -> {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.settingsSaved("font_size", "not_loaded")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.FONT_SIZE, 0L)))
            }
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
                warn("Failed to load sync state: ${result.fullEnvelope}")
                SyncState()
            }
            BridgeResult.NotLoaded -> SyncState()
        }
    }

    fun loadSyncConfig(): SyncConfig {
        return when (val result = syncBridge.loadSyncConfig()) {
            is BridgeResult.Success -> result.data.normalize()
            is BridgeResult.Error -> {
                warn("Failed to load sync config: ${result.fullEnvelope}")
                SyncConfig().normalize()
            }
            BridgeResult.NotLoaded -> SyncConfig().normalize()
        }
    }

    /**
     * #592 五：严格读取同步配置 — Bridge 失败时返回 null 而非默认配置。
     * 提交协议用它区分"读取失败"与"真实默认值"，避免把默认配置当旧值回滚。
     */
    fun loadSyncConfigStrict(): SyncConfig? {
        return when (val result = syncBridge.loadSyncConfig()) {
            is BridgeResult.Success -> result.data?.normalize()
            is BridgeResult.Error -> {
                warn("Strict load sync config failed: ${result.fullEnvelope}")
                null
            }
            BridgeResult.NotLoaded -> null
        }
    }

    /** #592 五：严格读取同步凭据 — Bridge 失败时返回 null。 */
    fun loadSyncSecretsStrict(): SyncSecrets? {
        return when (val result = syncBridge.loadSyncSecrets()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Strict load sync secrets failed: ${result.fullEnvelope}")
                null
            }
            BridgeResult.NotLoaded -> null
        }
    }

    /**
     * #592 六：统一仓库读取一次完整不可变快照（generation + config + secrets）。
     * 必须在 SyncProfileGate.snapshotExclusive 内调用，保证与提交互斥。
     * 读取失败返回 null（调用方按 Fatal 处理，不再回退默认配置）。
     */
    suspend fun snapshotSyncProfile(): SyncProfileSnapshot? {
        val state = profileStore.readState()
        // #592 五：读取者只读取 activeGeneration 对应的完整版本。
        // committed_config_json 只随 activeGeneration 原子推进，永远属于活动版本；
        // staged 但未提交的 config（失败/崩溃遗留）不会被当作完整版本。
        val config = if (state.hasCommittedProfile) {
            runCatching { configJson.fromJson(state.committedConfigJson, SyncConfig::class.java) }
                .getOrNull()
                ?.normalize()
                ?: return null
        } else {
            loadSyncConfigStrict() ?: return null
        }
        // 凭据按 generation 保存在安全存储；legacy（从未提交过）回退 live 槽。
        val generationSecrets = loadSyncSecretsForGeneration(state.activeGeneration)
        val secrets = when {
            generationSecrets != null && (generationSecrets.token?.isNotEmpty() == true) -> generationSecrets
            state.hasCommittedProfile -> null
            else -> loadSyncSecretsStrict()
        } ?: return null
        return SyncProfileSnapshot(state.activeGeneration, config, secrets)
    }

    /**
     * #592 六：把一次同步操作使用的完整 snapshot 凭据写入进程级 override，
     * 使 performSync/dryRun/diagnostics 的 Rust 侧不再从磁盘二次读取 secrets。
     */
    fun setSyncSecretsOverride(secrets: SyncSecrets) {
        when (val result = appBridge.setSyncSecretsOverride(secrets)) {
            is BridgeResult.Success -> { }
            is BridgeResult.Error -> warn("Failed to set sync secrets override: ${result.fullEnvelope}")
            BridgeResult.NotLoaded -> warn("Sync secrets override skipped: native library not loaded")
        }
    }

    /** 按 generation 保存凭据到安全存储（#592 五）。 */
    fun saveSyncSecretsForGeneration(generation: Long, secrets: SyncSecrets): SettingsSaveResult {
        return when (val result = appBridge.saveSyncSecretsForGeneration(generation.toULong(), secrets)) {
            is BridgeResult.Success -> SettingsSaveResult.Success
            is BridgeResult.Error -> {
                warn("Failed to save staged sync secrets for generation $generation: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, generation)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, generation)))
        }
    }

    /** 读取指定 generation 的安全存储凭据；缺失返回 null。 */
    fun loadSyncSecretsForGeneration(generation: Long): SyncSecrets? {
        return when (val result = appBridge.loadSyncSecretsForGeneration(generation.toULong())) {
            is BridgeResult.Success -> result.data?.takeIf { it.token?.isNotEmpty() == true }
            is BridgeResult.Error -> {
                warn("Failed to load staged sync secrets for generation $generation: ${result.fullEnvelope}")
                null
            }
            BridgeResult.NotLoaded -> null
        }
    }

    fun saveSyncConfig(config: SyncConfig): SettingsSaveResult {
        return when (val result = syncBridge.saveSyncConfig(config)) {
            is BridgeResult.Success -> {
                SettingsSaveResult.Success
            }
            is BridgeResult.Error -> {
                warn("Failed to save sync config: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
        }
    }

    fun loadSyncSecrets(): SyncSecrets {
        return when (val result = syncBridge.loadSyncSecrets()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load sync secrets: ${result.fullEnvelope}")
                SyncSecrets()
            }
            BridgeResult.NotLoaded -> SyncSecrets()
        }
    }

    fun saveSyncSecrets(secrets: SyncSecrets): SettingsSaveResult {
        return when (val result = syncBridge.saveSyncSecrets(secrets)) {
            is BridgeResult.Success -> {
                if (secrets.token != null) {
                    settingsBridge.dismissMigrationWarning()
                }
                SettingsSaveResult.Success
            }
            is BridgeResult.Error -> {
                warn("Failed to save sync secrets: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
        }
    }

    /**
     * #592 五：版本化提交同步配置与凭据 — 写 stagedConfig(generation=N) →
     * 写 stagedSecrets(generation=N) → 两项成功后原子更新 activeGeneration=N。
     *
     * - 旧 config/secrets 在写入前通过严格读取捕获；读取失败时直接中止，
     *   不产生任何写入，因此不再需要"失败后写回旧配置"（旧实现无法区分
     *   读取失败与默认值，可能用默认配置覆盖真实配置）。
     * - secrets 按 generation 保存到安全存储（saveSyncSecretsForGeneration），
     *   同时更新 live 槽供 Rust 内部读取。
     * - 只有 activeGeneration 提交成功后 AutoSyncScheduler 才更新 WorkManager，
     *   且直接使用应用容器中的仓库（本仓库），不新建 SettingsRepository 读半成品。
     * - 失败时旧 generation 继续有效：读取者只读取 activeGeneration 对应的完整版本。
     */
    suspend fun commitSyncProfile(config: SyncConfig, secrets: SyncSecrets): SettingsSaveResult {
        return SyncProfileGate.commitExclusive {
            val oldConfig = loadSyncConfigStrict()
            if (oldConfig == null) {
                warn("commitSyncProfile: strict read of old config failed — aborting before any write")
                return@commitExclusive SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
            }
            if (loadSyncSecretsStrict() == null) {
                warn("commitSyncProfile: strict read of old secrets failed — aborting before any write")
                return@commitExclusive SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
            }
            val generation = profileStore.nextGeneration()
            val normalized = config.normalize()

            // 1) stagedConfig(generation=N)：写入 Core 配置存储并记录 staged 标记。
            val configResult = saveSyncConfig(normalized)
            if (configResult is SettingsSaveResult.Failed) return@commitExclusive configResult
            profileStore.stageConfig(generation, configJson.toJson(normalized))

            // 2) stagedSecrets(generation=N)：凭据按 generation 写入安全存储 + live 槽。
            val secretsResult = saveSyncSecretsForGeneration(generation, secrets)
            if (secretsResult is SettingsSaveResult.Failed) {
                // 不写回旧配置（旧 generation 仍由 activeGeneration 标记继续有效）。
                warn("commitSyncProfile: staged secrets save failed for generation $generation — " +
                    "active generation unchanged, readers keep using the committed version")
                return@commitExclusive secretsResult
            }
            val liveSecretsResult = saveSyncSecrets(secrets)
            if (liveSecretsResult is SettingsSaveResult.Failed) {
                warn("commitSyncProfile: live secrets save failed for generation $generation — " +
                    "generation-staged secrets remain valid for readers")
            }
            profileStore.stageSecrets(generation)

            // 3) 原子更新 activeGeneration=N（单一 DataStore updateData，
            //    committed_config_json 与 activeGeneration 同时推进）。
            profileStore.commitGeneration(generation, configJson.toJson(normalized))

            // 4) 只有提交成功后调度 WorkManager；使用本仓库（应用容器实例）。
            AutoSyncScheduler.scheduleFromSettings(appContext, this)
            SettingsSaveResult.Success
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
                warn("Failed to get sync capability: ${result.fullEnvelope}")
                SyncCapabilityData()
            }
            BridgeResult.NotLoaded -> SyncCapabilityData()
        }
    }

    fun getSecureStorageWarning(): String? {
        return settingsBridge.getSecureStorageWarning()
    }

    fun dismissMigrationWarning() {
        settingsBridge.dismissMigrationWarning()
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
                warn("Failed to write device info: ${result.fullEnvelope}")
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
        return when (val result = settingsBridge.listBuiltinThemes()) {
            is BridgeResult.Success -> result.data ?: emptyList()
            is BridgeResult.Error -> {
                warn("Failed to list builtin themes: ${result.fullEnvelope}")
                emptyList()
            }
            BridgeResult.NotLoaded -> emptyList()
        }
    }

    fun listPaletteRecords(): List<uniffi.writer_core.ThemePaletteRecordDto> {
        return when (val result = settingsBridge.listPaletteRecords()) {
            is BridgeResult.Success -> result.data ?: emptyList()
            is BridgeResult.Error -> {
                warn("Failed to list palette records: ${result.fullEnvelope}")
                emptyList()
            }
            BridgeResult.NotLoaded -> emptyList()
        }
    }

    fun loadPaletteRecord(deviceId: String, fingerprint: String): uniffi.writer_core.ThemePaletteRecordDto? {
        return when (val result = settingsBridge.loadPaletteRecord(deviceId, fingerprint)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load palette record: ${result.fullEnvelope}")
                null
            }
            BridgeResult.NotLoaded -> null
        }
    }

    fun deletePaletteRecord(deviceId: String, fingerprint: String): Boolean {
        return when (val result = settingsBridge.deletePaletteRecord(deviceId, fingerprint)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to delete palette record: ${result.fullEnvelope}")
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

            val fingerprint = when (val r = settingsBridge.computePaletteFingerprint(lightScheme, darkScheme)) {
                is BridgeResult.Success -> r.data
                is BridgeResult.Error -> {
                    warn("Failed to compute palette fingerprint: ${r.fullEnvelope}")
                    return
                }
                BridgeResult.NotLoaded -> return
            }
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

            when (val saveResult = settingsBridge.savePaletteRecord(record)) {
                is BridgeResult.Error -> warn("Failed to save palette record: ${saveResult.fullEnvelope}")
                BridgeResult.NotLoaded -> warn("Native library not loaded, cannot save palette record")
                is BridgeResult.Success -> {}
            }
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
