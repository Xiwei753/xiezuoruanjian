package com.xiwei.sujian.feature.sync.data
import android.content.Context
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.settings.data.SaveFailure
import com.xiwei.sujian.feature.settings.data.SaveField
import com.xiwei.sujian.feature.settings.data.SettingsSaveResult
import com.xiwei.sujian.feature.sync.data.model.FullSyncDiagnosticsResult
import com.xiwei.sujian.feature.sync.data.model.FullSyncDryRunResult
import com.xiwei.sujian.feature.sync.data.model.FullSyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncState
import com.xiwei.sujian.feature.sync.work.AutoSyncScheduler

/**
 * #630 评论 #1：全量同步统一 Repository。
 *
 * 全应用只存在一份全局 [SyncProfileStore] + 一份 config/secrets 真相，
 * 不再按 projectId 路由 config/secrets，也不再维护应用级 / 作品级两套入口。
 * 一次同步 = App target + 所有 Project target，由 Core `perform_full_sync` 编排。
 */
open class SyncRepository(
    context: Context,
    private val appBridge: AppServiceBridge,
    preferencesSuffix: String = "",
) {
    private val appContext = context.applicationContext
    private val settingsBridge = appBridge.settingsBridge
    private val syncBridge = appBridge.syncBridge
    private val profileStore by lazy { SyncProfileStore(appContext) }
    private val legacyMetadataReader by lazy { LegacySyncProfileMetadataReader(appContext, profileStore) }
    private val nativeUnavailableMessage = "Native library not loaded"
    private val configJson = com.google.gson.Gson()

    init {
        @Suppress("UNUSED_VARIABLE")
        val ignoredSuffix = preferencesSuffix
    }

    private fun warn(msg: String) {
        DiagnosticsLogger.w("SyncRepository", msg)
    }

    // ── per-target 同步状态查询（保留 projectId） ──

    fun loadSyncState(projectId: String): SyncState =
        when (val result = syncBridge.loadSyncState(projectId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load sync state: ${result.fullEnvelope}")
                SyncState()
            }
            BridgeResult.NotLoaded -> SyncState()
        }

    open fun loadAppSyncState(): SyncState =
        when (val result = syncBridge.loadAppSyncState()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load app sync state: ${result.fullEnvelope}")
                SyncState()
            }
            BridgeResult.NotLoaded -> SyncState()
        }

    fun saveAppSyncState(state: SyncState): Boolean =
        when (syncBridge.saveAppSyncState(state)) {
            is BridgeResult.Success -> true
            is BridgeResult.Error -> {
                warn("Failed to save app sync state")
                false
            }
            BridgeResult.NotLoaded -> false
        }

    // ── 全量同步持久状态（Issue #630 评论 5307423953 Part B） ──

    /**
     * 加载全量同步持久状态。文件不存在/损坏/Core 返回 null 时返回 null，
     * 不抛异常，不阻断 UI。
     */
    open fun loadFullSyncState(): com.xiwei.sujian.feature.sync.data.model.FullSyncState? =
        when (val result = syncBridge.loadFullSyncState()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load full sync state: ${result.fullEnvelope}")
                null
            }
            BridgeResult.NotLoaded -> null
        }

    // ── 全局同步配置 / 凭据 ──

    open fun loadSyncConfig(): SyncConfig =
        when (val result = syncBridge.loadSyncConfig()) {
            is BridgeResult.Success -> result.data.normalize()
            is BridgeResult.Error -> {
                warn("Failed to load sync config: ${result.fullEnvelope}")
                SyncConfig().normalize()
            }
            BridgeResult.NotLoaded -> SyncConfig().normalize()
        }

    fun loadSyncConfigStrict(): SyncConfig? =
        when (val result = syncBridge.loadSyncConfig()) {
            is BridgeResult.Success -> result.data?.normalize()
            is BridgeResult.Error -> {
                warn("Strict load sync config failed: ${result.fullEnvelope}")
                null
            }
            BridgeResult.NotLoaded -> null
        }

    fun saveSyncConfig(config: SyncConfig): SettingsSaveResult =
        when (val result = syncBridge.saveSyncConfig(config)) {
            is BridgeResult.Success -> SettingsSaveResult.Success
            is BridgeResult.Error -> {
                warn("Failed to save sync config: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
        }

    fun loadSyncSecrets(): SyncSecrets =
        when (val result = syncBridge.loadSyncSecrets()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load sync secrets: ${result.fullEnvelope}")
                SyncSecrets()
            }
            BridgeResult.NotLoaded -> SyncSecrets()
        }

    fun saveSyncSecrets(secrets: SyncSecrets): SettingsSaveResult =
        when (val result = syncBridge.saveSyncSecrets(secrets)) {
            is BridgeResult.Success -> {
                if (secrets.token != null) settingsBridge.dismissMigrationWarning()
                SettingsSaveResult.Success
            }
            is BridgeResult.Error -> {
                warn("Failed to save sync secrets: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
        }

    fun loadLegacySyncSecretsTyped(): GenerationSecretsReadResult =
        when (val result = syncBridge.loadSyncSecrets()) {
            is BridgeResult.Success -> {
                val secrets = result.data
                if (secrets.token?.isNotEmpty() == true) {
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
            BridgeResult.NotLoaded ->
                GenerationSecretsReadResult.Failed(
                    SyncFailureKind.NativeUnavailable,
                    nativeUnavailableMessage,
                )
        }

    fun saveSyncSecretsForGeneration(
        generation: Long,
        secrets: SyncSecrets,
    ): SettingsSaveResult =
        when (val result = appBridge.saveSyncSecretsForGeneration(generation.toULong(), secrets)) {
            is BridgeResult.Success -> SettingsSaveResult.Success
            is BridgeResult.Error -> {
                warn("Failed to save staged sync secrets for generation $generation: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, generation)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, generation)))
        }

    fun deleteSyncSecretsForGeneration(generation: Long): BridgeResult<Unit> =
        appBridge.deleteSyncSecretsForGeneration(generation.toULong())

    fun loadSyncSecretsForGeneration(generation: Long): GenerationSecretsReadResult =
        when (val result = appBridge.loadSyncSecretsForGeneration(generation.toULong())) {
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
            BridgeResult.NotLoaded ->
                GenerationSecretsReadResult.Failed(
                    SyncFailureKind.NativeUnavailable,
                    nativeUnavailableMessage,
                )
        }

    // ── 进程级 secrets override（操作作用域凭据） ──

    fun setSyncSecretsOverrideStrict(secrets: SyncSecrets): Boolean =
        when (val result = appBridge.setSyncSecretsOverride(secrets)) {
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

    fun clearSyncSecretsOverride(): Boolean =
        when (val result = appBridge.clearSyncSecretsOverride()) {
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

    // ── 全局同步能力 ──

    open fun getSyncCapability(): SyncCapabilityData =
        when (val result = syncBridge.getSyncCapability()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to get sync capability: ${result.fullEnvelope}")
                SyncCapabilityData()
            }
            BridgeResult.NotLoaded -> SyncCapabilityData()
        }

    // ── 全量同步执行入口 ──

    fun performFullSync(
        config: SyncConfig,
        forceSync: Boolean = false,
    ): BridgeResult<FullSyncResult> = syncBridge.performFullSync(config, forceSync)

    fun performFullSyncDryRun(config: SyncConfig): BridgeResult<FullSyncDryRunResult> =
        syncBridge.performFullSyncDryRun(config)

    fun performFullSyncDiagnostics(config: SyncConfig): BridgeResult<FullSyncDiagnosticsResult> =
        syncBridge.performFullSyncDiagnostics(config)

    fun performFullSyncDryRunTyped(config: SyncConfig): SyncDryRunOutcome =
        when (val result = performFullSyncDryRun(config)) {
            is BridgeResult.Success -> SyncDryRunOutcome.Success(result.data)
            is BridgeResult.Error ->
                SyncDryRunOutcome.Error(
                    syncFailureKind = result.syncFailureKind ?: SyncFailureKind.Fatal,
                    message = result.message,
                )
            BridgeResult.NotLoaded -> SyncDryRunOutcome.NotLoaded
        }

    fun performFullSyncDiagnosticsTyped(config: SyncConfig): SyncDiagnosticsOutcome =
        when (val result = performFullSyncDiagnostics(config)) {
            is BridgeResult.Success -> SyncDiagnosticsOutcome.Success(result.data)
            is BridgeResult.Error ->
                SyncDiagnosticsOutcome.Error(
                    syncFailureKind = result.syncFailureKind ?: SyncFailureKind.Fatal,
                    message = result.message,
                )
            BridgeResult.NotLoaded -> SyncDiagnosticsOutcome.NotLoaded
        }

    // ── 全局 SyncProfile snapshot / commit ──

    /**
     * #630 评论 5307423953 Part A：Repository 内部底层 — 读取当前 generation 的 committed profile。
     *
     * 不触发 [ensureGlobalProfileMigrated]；业务入口（设置页/顶栏手动同步/自动同步）
     * 必须走 [loadCommittedSyncProfile]，不得直接调用本函数。标记为 [internal] 仅供
     * 同模块单元测试验证迁移完成后 generation 读取行为。
     */
    internal suspend fun snapshotSyncProfile(): SyncProfileReadResult {
        val state = profileStore.readState()
        val config =
            if (state.hasCommittedProfile) {
                runCatching { configJson.fromJson(state.committedConfigJson, SyncConfig::class.java) }
                    .getOrNull()?.normalize()
                    ?: return SyncProfileReadResult.Failed(SyncFailureKind.Fatal, "Committed config JSON parse failed")
            } else {
                loadSyncConfigStrict()
                    ?: return SyncProfileReadResult.Failed(SyncFailureKind.Fatal, "Sync config load failed")
            }
        val generationSecrets = loadSyncSecretsForGeneration(state.activeGeneration)
        val secrets: SyncSecrets =
            when (generationSecrets) {
                is GenerationSecretsReadResult.Found -> generationSecrets.secrets
                is GenerationSecretsReadResult.NotConfigured -> {
                    if (state.hasCommittedProfile) {
                        SyncSecrets()
                    } else {
                        when (val legacy = loadLegacySyncSecretsTyped()) {
                            is GenerationSecretsReadResult.Found -> legacy.secrets
                            is GenerationSecretsReadResult.NotConfigured -> SyncSecrets()
                            is GenerationSecretsReadResult.Failed -> return SyncProfileReadResult.Failed(
                                legacy.kind,
                                legacy.message,
                            )
                        }
                    }
                }
                is GenerationSecretsReadResult.Failed -> return SyncProfileReadResult.Failed(
                    generationSecrets.kind,
                    generationSecrets.message,
                )
            }
        val snapshot = SyncProfileSnapshot(state.activeGeneration, config, secrets)
        return if (secrets.token?.isNotEmpty() == true) {
            SyncProfileReadResult.Found(snapshot)
        } else {
            SyncProfileReadResult.NotConfigured(snapshot)
        }
    }

    suspend fun loadCommittedSyncProfile(): SyncProfileReadResult =
        SyncProfileGate.snapshotExclusive {
            ensureGlobalProfileMigrated()?.let { failed ->
                if (failed is SettingsSaveResult.Failed) {
                    return@snapshotExclusive SyncProfileReadResult.Failed(
                        SyncFailureKind.Fatal,
                        "legacy_sync_profile_migration_failed",
                    )
                }
            }
            snapshotSyncProfile()
        }

    /**
     * #630 评论第 5 点 Part A：读时和写时共用的迁移 helper。
     *
     * 在 [loadCommittedSyncProfile]（设置页首次加载）和 [commitSyncProfile]（首次保存）
     * 前都执行一次：若已存在 committed profile 直接返回 null；否则触发
     * [migrateLegacyProfileIfNeeded]。
     *
     * 返回 null 表示无需迁移或迁移成功可继续；返回 [SettingsSaveResult.Failed] 表示
     * 迁移失败，调用方必须类型化返回错误，不得静默降级为"无 profile"。
     */
    private suspend fun ensureGlobalProfileMigrated(): SettingsSaveResult? {
        val state = profileStore.readState()
        if (state.hasCommittedProfile) return null
        return migrateLegacyProfileIfNeeded()
    }

    /**
     * #630 评论第 4 点 / D + 第 5 点 Part C：旧→新同步 profile 迁移钩子。
     *
     * 先用 [legacyMetadataReader] 从旧 DataStore 读取精确
     * active_generation / committed_config_json，构造 metadata 列表传给 Core
     * `migrate_legacy_sync_profile_with_metadata`，避免 Core 猜测 generation 上限。
     * 旧 DataStore 无 metadata 时回退空列表，Core 用 base key / 文件 fallback。
     *
     * 行为：
     * - `not_needed` / `no_legacy_config`：返回 null，让调用方继续正常提交；
     * - `migrated`：Core 已写好新全局并删旧凭据，Android 侧把 [profileStore] 推进到新 generation，
     *   完整事务：stageConfig → saveSyncSecretsForGeneration → stageSecrets → commitGeneration；
     * - `needs_reconfigure`：多项目冲突，返回 [SettingsSaveResult.Failed] 让 UI 引导用户重选，
     *   Core 不删旧凭据，用户可手动恢复；
     * - Core 抛错或原生库未加载：返回 [SettingsSaveResult.Failed]，不继续提交，不删旧凭据；
     * - `migrated` 但 outcome.config / outcome.secrets 缺失或未知 outcomeKind：
     *   返回 [SettingsSaveResult.Failed]，不静默返回 null（#630 评论第 5 点 Part B）。
     *
     * 标记为 [internal] 供同模块单元测试直接验证迁移行为（不通过 [commitSyncProfile] 间接测）。
     */
    internal suspend fun migrateLegacyProfileIfNeeded(): SettingsSaveResult? {
        val initialState = profileStore.readState()
        if (initialState.hasCommittedProfile) return null
        val metadata = legacyMetadataReader.readMetadata()
        val outcome =
            when (val result = appBridge.migrateLegacySyncProfileWithMetadata(metadata)) {
                is BridgeResult.Success -> result.data
                is BridgeResult.Error -> {
                    warn(
                        "commitSyncProfile: legacy migration failed - aborting before any write: " +
                            result.fullEnvelope,
                    )
                    return SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
                }
                BridgeResult.NotLoaded ->
                    return SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
            }
        return when (outcome.outcomeKind) {
            "not_needed", "no_legacy_config" -> null
            "needs_reconfigure" -> {
                warn("commitSyncProfile: legacy migration needs reconfigure - ${outcome.reason}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
            }
            "migrated" -> {
                // #630 评论第 5 点 Part B：完整事务 — 缺 config/secrets 直接类型化失败，
                // 不静默返回 null。secrets 通过 saveSyncSecretsForGeneration 写入安全存储，
                // 避免 hasCommittedProfile==true 但 sync_token_global_gN 不存在。
                val migratedConfig = outcome.config
                if (migratedConfig == null) {
                    return SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
                }
                val migratedSecrets = outcome.secrets
                if (migratedSecrets == null) {
                    return SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
                }
                val migrationGeneration = profileStore.nextGeneration()
                profileStore.stageConfig(migrationGeneration, configJson.toJson(migratedConfig.normalize()))
                val secretResult = saveSyncSecretsForGeneration(migrationGeneration, migratedSecrets)
                if (secretResult is SettingsSaveResult.Failed) return secretResult
                profileStore.stageSecrets(migrationGeneration)
                profileStore.commitGeneration(migrationGeneration, configJson.toJson(migratedConfig.normalize()))
                // #630 评论 5307423953 Part C：新 generation 的
                // saveSyncSecretsForGeneration → stageSecrets → commitGeneration 全部成功后，
                // 清除旧 Android DataStore metadata。失败/needs_reconfigure 分支不调用，
                // 保留旧 metadata 供用户手动恢复。
                legacyMetadataReader.clearLegacyMetadata()
                null
            }
            else -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
        }
    }

    suspend fun commitSyncProfile(
        config: SyncConfig,
        secrets: SyncSecrets,
    ): SettingsSaveResult {
        val committed =
            SyncProfileGate.commitExclusive {
                ensureGlobalProfileMigrated()?.let { return@commitExclusive it }
                val generation = profileStore.nextGeneration()
                val normalized = config.normalize()
                profileStore.stageConfig(generation, configJson.toJson(normalized))
                val secretsResult = saveSyncSecretsForGeneration(generation, secrets)
                if (secretsResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitSyncProfile: staged secrets save failed for generation $generation" +
                            " - active generation unchanged",
                    )
                    return@commitExclusive secretsResult
                }
                profileStore.stageSecrets(generation)
                profileStore.commitGeneration(generation, configJson.toJson(normalized))
                val cleanupResult = cleanupStaleGenerationCredentials(generation)
                if (cleanupResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitSyncProfile: generation cleanup reported typed failures: " +
                            "${cleanupResult.failures.joinToString { "gen=${it.revision} field=${it.field}" }}",
                    )
                }
                val liveConfigResult = saveSyncConfig(normalized)
                if (liveConfigResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitSyncProfile: live config mirror update failed for generation $generation",
                    )
                }
                val liveSecretsResult = saveSyncSecrets(secrets)
                if (liveSecretsResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitSyncProfile: live secrets mirror update failed for generation $generation",
                    )
                }
                SettingsSaveResult.Success
            }
        if (committed is SettingsSaveResult.Success) AutoSyncScheduler.scheduleFromSettings(appContext, this)
        return committed
    }

    private suspend fun cleanupStaleGenerationCredentials(current: Long): SettingsSaveResult {
        val failures = mutableListOf<SaveFailure>()
        val range = generationCleanupRange(current)
        if (range != null) {
            for (gen in range) {
                when (appBridge.deleteSyncSecretsForGeneration(gen.toULong())) {
                    is BridgeResult.Success -> { }
                    is BridgeResult.Error -> failures.add(SaveFailure(SaveField.SYNC_SECRETS, gen))
                    BridgeResult.NotLoaded -> failures.add(SaveFailure(SaveField.SYNC_SECRETS, gen))
                }
            }
        }
        try {
            profileStore.clearStaleStagedMarkers(current)
        } catch (e: Exception) {
            warn("cleanupStaleGenerationCredentials: failed to clear stale staged markers: ${e.message}")
        }
        return if (failures.isEmpty()) SettingsSaveResult.Success else SettingsSaveResult.Failed(failures)
    }
}

sealed class SyncDryRunOutcome {
    data class Success(val plan: com.xiwei.sujian.feature.sync.data.model.FullSyncDryRunResult) : SyncDryRunOutcome()

    data class Error(val syncFailureKind: SyncFailureKind, val message: String) : SyncDryRunOutcome()

    data object NotLoaded : SyncDryRunOutcome()
}

sealed class SyncDiagnosticsOutcome {
    data class Success(
        val result: com.xiwei.sujian.feature.sync.data.model.FullSyncDiagnosticsResult,
    ) : SyncDiagnosticsOutcome()

    data class Error(val syncFailureKind: SyncFailureKind, val message: String) : SyncDiagnosticsOutcome()

    data object NotLoaded : SyncDiagnosticsOutcome()
}
