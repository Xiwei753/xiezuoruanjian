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

    /**
     * #595 四：legacy 槽凭据读取的类型化结果 — 不再把"读取失败"伪装成"未配置"。
     *
     * - [GenerationSecretsReadResult.Found]：legacy 槽存在有效凭据。
     * - [GenerationSecretsReadResult.NotConfigured]：读取成功但 token 为空/缺失。
     * - [GenerationSecretsReadResult.Failed]：原生库未加载、安全存储/解密失败。
     */
    fun loadLegacySyncSecretsTyped(): GenerationSecretsReadResult {
        return when (val result = syncBridge.loadSyncSecrets()) {
            is BridgeResult.Success -> {
                val secrets = result.data
                if (secrets != null && secrets.token?.isNotEmpty() == true) {
                    GenerationSecretsReadResult.Found(secrets)
                } else {
                    GenerationSecretsReadResult.NotConfigured
                }
            }
            is BridgeResult.Error -> {
                val kind = result.syncFailureKind ?: SyncFailureKind.Fatal
                warn("Legacy sync secrets read failed: ${result.fullEnvelope}")
                GenerationSecretsReadResult.Failed(kind, result.fullEnvelope)
            }
            BridgeResult.NotLoaded -> GenerationSecretsReadResult.Failed(
                SyncFailureKind.NativeUnavailable, "Native library not loaded"
            )
        }
    }

    /**
     * #592 六 / #595 五：统一仓库读取一次完整不可变快照（generation + config + secrets）。
     * 必须在 SyncProfileGate.snapshotExclusive 内调用，保证与提交互斥。
     *
     * #595 五：返回 [SyncProfileReadResult] — 不再把"没有 token"和"读取失败"压成
     * 同一个 null。config 解析失败或凭据读取失败返回 [SyncProfileReadResult.Failed]，
     * 凭据为空返回 [SyncProfileReadResult.NotConfigured]，凭据非空返回 [SyncProfileReadResult.Found]。
     */
    suspend fun snapshotSyncProfile(): SyncProfileReadResult {
        val state = profileStore.readState()
        // #592 五：读取者只读取 activeGeneration 对应的完整版本。
        // committed_config_json 只随 activeGeneration 原子推进，永远属于活动版本；
        // staged 但未提交的 config（失败/崩溃遗留）不会被当作完整版本。
        val config = if (state.hasCommittedProfile) {
            runCatching { configJson.fromJson(state.committedConfigJson, SyncConfig::class.java) }
                .getOrNull()
                ?.normalize()
                ?: return SyncProfileReadResult.Failed(
                    SyncFailureKind.Fatal, "Committed config JSON parse failed"
                )
        } else {
            loadSyncConfigStrict() ?: return SyncProfileReadResult.Failed(
                SyncFailureKind.Fatal, "Sync config load failed"
            )
        }
        // 凭据按 generation 保存在安全存储；legacy（从未提交过）回退 live 槽。
        // #595 五：类型化区分"未配置"与"读取失败" — Failed 不再转换成 SyncSecrets()。
        val generationSecrets = loadSyncSecretsForGeneration(state.activeGeneration)
        val secrets: SyncSecrets = when (generationSecrets) {
            is GenerationSecretsReadResult.Found -> generationSecrets.secrets
            is GenerationSecretsReadResult.NotConfigured -> {
                if (state.hasCommittedProfile) {
                    SyncSecrets()
                } else {
                    // #595 四：legacy 槽读取失败不得伪装成"未配置" —
                    // 原生库未加载/安全存储失败必须返回 Failed，设置页据此显示错误。
                    when (val legacy = loadLegacySyncSecretsTyped()) {
                        is GenerationSecretsReadResult.Found -> legacy.secrets
                        is GenerationSecretsReadResult.NotConfigured -> SyncSecrets()
                        is GenerationSecretsReadResult.Failed -> {
                            return SyncProfileReadResult.Failed(legacy.kind, legacy.message)
                        }
                    }
                }
            }
            is GenerationSecretsReadResult.Failed -> {
                return SyncProfileReadResult.Failed(generationSecrets.kind, generationSecrets.message)
            }
        }
        val snapshot = SyncProfileSnapshot(state.activeGeneration, config, secrets)
        return if (secrets.token?.isNotEmpty() == true) {
            SyncProfileReadResult.Found(snapshot)
        } else {
            SyncProfileReadResult.NotConfigured(snapshot)
        }
    }

    /**
     * #595 十：严格设置进程级 secrets override — 失败返回 false，调用方必须终止操作。
     * 只允许在 SyncSession.runExclusive 内调用（取得同步独占锁之后写入），
     * 杜绝两个同步并发时串用彼此的 token。
     */
    fun setSyncSecretsOverrideStrict(secrets: SyncSecrets): Boolean {
        return when (val result = appBridge.setSyncSecretsOverride(secrets)) {
            is BridgeResult.Success -> true
            is BridgeResult.Error -> {
                warn("Failed to set sync secrets override: ${result.fullEnvelope}")
                false
            }
            BridgeResult.NotLoaded -> {
                warn("Sync secrets override failed: native library not loaded")
                false
            }
        }
    }

    /**
     * #595 十：操作结束后清除进程级 override — 陈旧凭据不得泄漏到后续操作
     * （Core 的 refresh_secrets_override 在已有 override 时不会重新读取磁盘）。
     */
    fun clearSyncSecretsOverride(): Boolean {
        return when (val result = appBridge.clearSyncSecretsOverride()) {
            is BridgeResult.Success -> true
            is BridgeResult.Error -> {
                warn("Failed to clear sync secrets override: ${result.fullEnvelope}")
                false
            }
            BridgeResult.NotLoaded -> {
                warn("Sync secrets override clear failed: native library not loaded")
                false
            }
        }
    }

    /**
     * #595 八 / #595 五：UI 初始化和刷新读取活动 generation 的完整 snapshot，
     * 不再读取 live legacy 槽（镜像槽不参与权威读取）。
     *
     * #595 五：返回 [SyncProfileReadResult] — 设置页可据此区分"未配置"（NotConfigured，
     * 显示空 token）与"读取失败"（Failed，显示类型化错误），不再把两者压成同一个 null。
     */
    suspend fun loadCommittedSyncProfile(): SyncProfileReadResult {
        return SyncProfileGate.snapshotExclusive { snapshotSyncProfile() }
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

    /**
     * #595 五：读取指定 generation 的安全存储凭据 — 类型化结果，不再把"没有 token"
     * 和"读取失败"压成同一个 null。
     *
     * - [GenerationSecretsReadResult.Found]：安全存储中存在有效凭据。
     * - [GenerationSecretsReadResult.NotConfigured]：凭据条目缺失或 token 为空。
     * - [GenerationSecretsReadResult.Failed]：安全存储读取失败、解密失败或原生库未加载。
     */
    fun loadSyncSecretsForGeneration(generation: Long): GenerationSecretsReadResult {
        return when (val result = appBridge.loadSyncSecretsForGeneration(generation.toULong())) {
            is BridgeResult.Success -> {
                val secrets = result.data
                if (secrets != null && secrets.token?.isNotEmpty() == true) {
                    GenerationSecretsReadResult.Found(secrets)
                } else {
                    GenerationSecretsReadResult.NotConfigured
                }
            }
            is BridgeResult.Error -> {
                val kind = result.syncFailureKind ?: SyncFailureKind.Fatal
                warn("Failed to load staged sync secrets for generation $generation: ${result.fullEnvelope}")
                GenerationSecretsReadResult.Failed(kind, result.fullEnvelope)
            }
            BridgeResult.NotLoaded -> GenerationSecretsReadResult.Failed(
                SyncFailureKind.NativeUnavailable, "Native library not loaded"
            )
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
     * #592 五/#595 九：版本化提交同步配置与凭据 — 先完成 legacy → generation 迁移，
     * 再写 stagedConfig(generation=N) → stagedSecrets(generation=N) →
     * 原子提交 activeGeneration=N；新 config 不得在 marker 提交前发布到 live
     * Core 配置文件（镜像槽在提交成功后更新，不参与权威读取）。
     *
     * 崩溃原子性：
     * - 首次提交（无 committed profile）先做迁移：把 legacy config/secrets 写为
     *   首个 generation 并原子提交 marker；此后所有读取只认 generation store，
     *   不存在“live config 已写、marker 未提交”的半提交窗口。
     * - 每次新提交：staged 载荷与凭据先写，marker 最后原子推进；失败时旧
     *   generation 继续有效，读取者只读 activeGeneration 对应的完整版本。
     * - live 槽（loadSyncConfig/loadSyncSecrets 镜像）在提交成功后尽力更新，
     *   仅供兼容旧 Core API 使用，不参与权威读取。
     * - 只有 activeGeneration 提交成功后 AutoSyncScheduler 才更新 WorkManager，
     *   且直接使用应用容器中的仓库（本仓库），不新建 SettingsRepository 读半成品。
     */
    suspend fun commitSyncProfile(config: SyncConfig, secrets: SyncSecrets): SettingsSaveResult {
        val committed = SyncProfileGate.commitExclusive {
            // 1) 首次提交前完成 legacy → generation 迁移（解决八）。
            //    迁移只写 generation-staged 凭据 + 原子 marker，不发布新 config；
            //    legacy 内容仍在 live 槽，marker 提交后 generation store 成为权威。
            val initialState = profileStore.readState()
            if (!initialState.hasCommittedProfile) {
                val legacyConfig = loadSyncConfigStrict()
                if (legacyConfig == null) {
                    warn("commitSyncProfile: strict read of legacy config failed — aborting migration before any write")
                    return@commitExclusive SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
                }
                val legacySecrets = loadLegacySyncSecretsTyped()
                if (legacySecrets is GenerationSecretsReadResult.Failed) {
                    warn("commitSyncProfile: legacy secrets read failed — aborting migration before any write")
                    return@commitExclusive SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
                }
                val migrationSecrets = if (legacySecrets is GenerationSecretsReadResult.Found) {
                    legacySecrets.secrets
                } else {
                    // #595 四：legacy 明确未配置（读取成功但无 token）→ 迁移空凭据；
                    // 读取失败已在上面返回，不会把失败伪装成"未配置"。
                    SyncSecrets()
                }
                val migrationGeneration = profileStore.nextGeneration()
                val migrationResult = saveSyncSecretsForGeneration(migrationGeneration, migrationSecrets)
                if (migrationResult is SettingsSaveResult.Failed) {
                    warn("commitSyncProfile: legacy secrets migration to generation $migrationGeneration failed — aborting")
                    return@commitExclusive migrationResult
                }
                profileStore.stageConfig(migrationGeneration, configJson.toJson(legacyConfig.normalize()))
                profileStore.stageSecrets(migrationGeneration)
                profileStore.commitGeneration(migrationGeneration, configJson.toJson(legacyConfig.normalize()))
            }

            val generation = profileStore.nextGeneration()
            val normalized = config.normalize()

            // 2) stagedConfig(generation=N)：只写 DataStore staging 标记与载荷，
            //    不触碰 live Core 配置文件（marker 提交前不得发布新 config）。
            profileStore.stageConfig(generation, configJson.toJson(normalized))

            // 3) stagedSecrets(generation=N)：凭据按 generation 写入安全存储。
            val secretsResult = saveSyncSecretsForGeneration(generation, secrets)
            if (secretsResult is SettingsSaveResult.Failed) {
                // 不写回旧配置（旧 generation 仍由 activeGeneration 标记继续有效）。
                warn("commitSyncProfile: staged secrets save failed for generation $generation — " +
                    "active generation unchanged, readers keep using the committed version")
                return@commitExclusive secretsResult
            }
            profileStore.stageSecrets(generation)

            // 4) 原子更新 activeGeneration=N（单一 DataStore updateData，
            //    committed_config_json 与 activeGeneration 同时推进）。
            profileStore.commitGeneration(generation, configJson.toJson(normalized))

            // 5) 提交成功后镜像到 live 槽（兼容旧 Core API；失败不影响权威读取）。
            val liveConfigResult = saveSyncConfig(normalized)
            if (liveConfigResult is SettingsSaveResult.Failed) {
                warn("commitSyncProfile: live config mirror update failed for generation $generation — " +
                    "generation store remains authoritative")
            }
            val liveSecretsResult = saveSyncSecrets(secrets)
            if (liveSecretsResult is SettingsSaveResult.Failed) {
                warn("commitSyncProfile: live secrets mirror update failed for generation $generation — " +
                    "generation-staged secrets remain valid for readers")
            }
            SettingsSaveResult.Success
        }
        // 6) 只有提交成功后调度 WorkManager，且必须在 commitExclusive 释放之后：
        //    scheduleFromSettings 会获取 snapshotExclusive，锁内调用会自死锁。
        //    直接使用本仓库（应用容器实例），不新建 SettingsRepository 读半成品。
        if (committed is SettingsSaveResult.Success) {
            AutoSyncScheduler.scheduleFromSettings(appContext, this)
        }
        return committed
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
