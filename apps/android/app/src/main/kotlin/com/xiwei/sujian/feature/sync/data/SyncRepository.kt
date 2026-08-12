package com.xiwei.sujian.feature.sync.data
import android.content.Context
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.settings.data.SaveFailure
import com.xiwei.sujian.feature.settings.data.SaveField
import com.xiwei.sujian.feature.settings.data.SettingsSaveResult
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncDiagnosticsResult
import com.xiwei.sujian.feature.sync.data.model.SyncPlan
import com.xiwei.sujian.feature.sync.data.model.SyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncState
import com.xiwei.sujian.feature.sync.work.AutoSyncScheduler

class SyncRepository(
    context: Context,
    private val appBridge: AppServiceBridge,
    preferencesSuffix: String = "",
) {
    private val appContext = context.applicationContext
    private val settingsBridge = appBridge.settingsBridge
    private val syncBridge = appBridge.syncBridge
    private val profileStore by lazy { ProjectSyncProfileStore(appContext) }
    private val appProfileStore by lazy { AppSyncProfileStore(appContext) }
    private val nativeUnavailableMessage = "Native library not loaded"
    private val configJson = com.google.gson.Gson()

    init {
        @Suppress("UNUSED_VARIABLE")
        val ignoredSuffix = preferencesSuffix
    }

    private fun warn(msg: String) {
        DiagnosticsLogger.w("SyncRepository", msg)
    }

    fun loadSyncState(projectId: String): SyncState =
        when (val result = syncBridge.loadSyncState(projectId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load sync state: ${result.fullEnvelope}")
                SyncState()
            }
            BridgeResult.NotLoaded -> SyncState()
        }

    fun loadSyncConfig(projectId: String): SyncConfig =
        when (val result = syncBridge.loadSyncConfig(projectId)) {
            is BridgeResult.Success -> result.data.normalize()
            is BridgeResult.Error -> {
                warn("Failed to load sync config: ${result.fullEnvelope}")
                SyncConfig().normalize()
            }
            BridgeResult.NotLoaded -> SyncConfig().normalize()
        }

    fun loadSyncConfigStrict(projectId: String): SyncConfig? =
        when (val result = syncBridge.loadSyncConfig(projectId)) {
            is BridgeResult.Success -> result.data?.normalize()
            is BridgeResult.Error -> {
                warn("Strict load sync config failed: ${result.fullEnvelope}")
                null
            }
            BridgeResult.NotLoaded -> null
        }

    fun loadLegacySyncSecretsTyped(projectId: String): GenerationSecretsReadResult =
        when (val result = syncBridge.loadSyncSecrets(projectId)) {
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
            BridgeResult.NotLoaded ->
                GenerationSecretsReadResult.Failed(
                    SyncFailureKind.NativeUnavailable,
                    nativeUnavailableMessage,
                )
        }

    suspend fun snapshotSyncProfile(projectId: String): SyncProfileReadResult {
        val state = profileStore.readState(projectId)
        val config =
            if (state.hasCommittedProfile) {
                runCatching { configJson.fromJson(state.committedConfigJson, SyncConfig::class.java) }
                    .getOrNull()?.normalize()
                    ?: return SyncProfileReadResult.Failed(SyncFailureKind.Fatal, "Committed config JSON parse failed")
            } else {
                loadSyncConfigStrict(projectId)
                    ?: return SyncProfileReadResult.Failed(SyncFailureKind.Fatal, "Sync config load failed")
            }
        val generationSecrets = loadSyncSecretsForGeneration(projectId, state.activeGeneration)
        val secrets: SyncSecrets =
            when (generationSecrets) {
                is GenerationSecretsReadResult.Found -> generationSecrets.secrets
                is GenerationSecretsReadResult.NotConfigured -> {
                    if (state.hasCommittedProfile) {
                        SyncSecrets()
                    } else {
                        when (val legacy = loadLegacySyncSecretsTyped(projectId)) {
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
        val snapshot = ProjectSyncProfileSnapshot(state.activeGeneration, config, secrets)
        return if (secrets.token?.isNotEmpty() == true) {
            SyncProfileReadResult.Found(snapshot)
        } else {
            SyncProfileReadResult.NotConfigured(snapshot)
        }
    }

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

    suspend fun loadCommittedSyncProfile(projectId: String): SyncProfileReadResult =
        SyncProfileGate.snapshotExclusive { snapshotSyncProfile(projectId) }

    fun saveSyncSecretsForGeneration(
        projectId: String,
        generation: Long,
        secrets: SyncSecrets,
    ): SettingsSaveResult =
        when (val result = appBridge.saveSyncSecretsForGeneration(projectId, generation.toULong(), secrets)) {
            is BridgeResult.Success -> SettingsSaveResult.Success
            is BridgeResult.Error -> {
                warn("Failed to save staged sync secrets for generation $generation: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, generation)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, generation)))
        }

    fun deleteSyncSecretsForGeneration(
        projectId: String,
        generation: Long,
    ): BridgeResult<Unit> = appBridge.deleteSyncSecretsForGeneration(projectId, generation.toULong())

    fun loadSyncSecretsForGeneration(
        projectId: String,
        generation: Long,
    ): GenerationSecretsReadResult =
        when (val result = appBridge.loadSyncSecretsForGeneration(projectId, generation.toULong())) {
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

    fun saveSyncConfig(
        projectId: String,
        config: SyncConfig,
    ): SettingsSaveResult =
        when (val result = syncBridge.saveSyncConfig(projectId, config)) {
            is BridgeResult.Success -> SettingsSaveResult.Success
            is BridgeResult.Error -> {
                warn("Failed to save sync config: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
        }

    fun loadSyncSecrets(projectId: String): SyncSecrets =
        when (val result = syncBridge.loadSyncSecrets(projectId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load sync secrets: ${result.fullEnvelope}")
                SyncSecrets()
            }
            BridgeResult.NotLoaded -> SyncSecrets()
        }

    fun saveSyncSecrets(
        projectId: String,
        secrets: SyncSecrets,
    ): SettingsSaveResult =
        when (val result = syncBridge.saveSyncSecrets(projectId, secrets)) {
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

    private suspend fun migrateLegacyProfileIfNeeded(projectId: String): SettingsSaveResult? {
        val initialState = profileStore.readState(projectId)
        if (initialState.hasCommittedProfile) return null
        val legacyConfig = loadSyncConfigStrict(projectId)
        if (legacyConfig == null) {
            warn("commitSyncProfile: strict read of legacy config failed - aborting migration before any write")
            return SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
        }
        val legacySecrets = loadLegacySyncSecretsTyped(projectId)
        if (legacySecrets is GenerationSecretsReadResult.Failed) {
            warn("commitSyncProfile: legacy secrets read failed - aborting migration before any write")
            return SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
        }
        val migrationSecrets =
            if (legacySecrets is GenerationSecretsReadResult.Found) legacySecrets.secrets else SyncSecrets()
        val migrationGeneration = profileStore.nextGeneration(projectId)
        val migrationResult = saveSyncSecretsForGeneration(projectId, migrationGeneration, migrationSecrets)
        if (migrationResult is SettingsSaveResult.Failed) {
            warn("commitSyncProfile: legacy secrets migration to generation $migrationGeneration failed - aborting")
            return migrationResult
        }
        profileStore.stageConfig(projectId, migrationGeneration, configJson.toJson(legacyConfig.normalize()))
        profileStore.stageSecrets(projectId, migrationGeneration)
        profileStore.commitGeneration(projectId, migrationGeneration, configJson.toJson(legacyConfig.normalize()))
        return null
    }

    suspend fun commitSyncProfile(
        projectId: String,
        config: SyncConfig,
        secrets: SyncSecrets,
    ): SettingsSaveResult {
        val committed =
            SyncProfileGate.commitExclusive {
                migrateLegacyProfileIfNeeded(projectId)?.let { return@commitExclusive it }
                val generation = profileStore.nextGeneration(projectId)
                val normalized = config.normalize()
                profileStore.stageConfig(projectId, generation, configJson.toJson(normalized))
                val secretsResult = saveSyncSecretsForGeneration(projectId, generation, secrets)
                if (secretsResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitSyncProfile: staged secrets save failed for generation $generation" +
                            " - active generation unchanged",
                    )
                    return@commitExclusive secretsResult
                }
                profileStore.stageSecrets(projectId, generation)
                profileStore.commitGeneration(projectId, generation, configJson.toJson(normalized))
                val cleanupResult = cleanupStaleGenerationCredentials(projectId, generation)
                if (cleanupResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitSyncProfile: generation cleanup reported typed failures: " +
                            "${cleanupResult.failures.joinToString { "gen=${it.revision} field=${it.field}" }}",
                    )
                }
                val liveConfigResult = saveSyncConfig(projectId, normalized)
                if (liveConfigResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitSyncProfile: live config mirror update failed for generation $generation",
                    )
                }
                val liveSecretsResult = saveSyncSecrets(projectId, secrets)
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

    private suspend fun cleanupStaleGenerationCredentials(
        projectId: String,
        current: Long,
    ): SettingsSaveResult {
        val failures = mutableListOf<SaveFailure>()
        val range = generationCleanupRange(current)
        if (range != null) {
            for (gen in range) {
                when (appBridge.deleteSyncSecretsForGeneration(projectId, gen.toULong())) {
                    is BridgeResult.Success -> { }
                    is BridgeResult.Error -> failures.add(SaveFailure(SaveField.SYNC_SECRETS, gen))
                    BridgeResult.NotLoaded -> failures.add(SaveFailure(SaveField.SYNC_SECRETS, gen))
                }
            }
        }
        try {
            profileStore.clearStaleStagedMarkers(projectId, current)
        } catch (e: Exception) {
            warn("cleanupStaleGenerationCredentials: failed to clear stale staged markers: ${e.message}")
        }
        return if (failures.isEmpty()) SettingsSaveResult.Success else SettingsSaveResult.Failed(failures)
    }

    fun performSyncDiagnostics(
        projectId: String,
        config: SyncConfig,
    ): BridgeResult<SyncDiagnosticsResult> =
        syncBridge.performSyncDiagnostics(
            projectId,
            config,
        )

    fun performSyncDryRun(
        projectId: String,
        config: SyncConfig,
    ): BridgeResult<SyncPlan> =
        syncBridge.performSyncDryRun(
            projectId,
            config,
        )

    fun performSync(
        projectId: String,
        config: SyncConfig,
        forceSync: Boolean = false,
    ): BridgeResult<SyncResult> =
        syncBridge.performSync(
            projectId,
            config,
            forceSync,
        )

    fun getSyncCapability(projectId: String): SyncCapabilityData =
        when (val result = syncBridge.getSyncCapability(projectId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to get sync capability: ${result.fullEnvelope}")
                SyncCapabilityData()
            }
            BridgeResult.NotLoaded -> SyncCapabilityData()
        }

    fun performSyncDryRunTyped(
        projectId: String,
        config: SyncConfig,
    ): SyncDryRunOutcome =
        when (val result = performSyncDryRun(projectId, config)) {
            is BridgeResult.Success -> SyncDryRunOutcome.Success(result.data)
            is BridgeResult.Error ->
                SyncDryRunOutcome.Error(
                    syncFailureKind = result.syncFailureKind ?: SyncFailureKind.Fatal,
                    message = result.message,
                )
            BridgeResult.NotLoaded -> SyncDryRunOutcome.NotLoaded
        }

    fun performSyncDiagnosticsTyped(
        projectId: String,
        config: SyncConfig,
    ): SyncDiagnosticsOutcome =
        when (val result = performSyncDiagnostics(projectId, config)) {
            is BridgeResult.Success -> SyncDiagnosticsOutcome.Success(result.data)
            is BridgeResult.Error ->
                SyncDiagnosticsOutcome.Error(
                    syncFailureKind = result.syncFailureKind ?: SyncFailureKind.Fatal,
                    message = result.message,
                )
            BridgeResult.NotLoaded -> SyncDiagnosticsOutcome.NotLoaded
        }

    fun loadAppSyncConfig(): SyncConfig =
        when (val result = syncBridge.loadAppSyncConfig()) {
            is BridgeResult.Success -> result.data.normalize()
            is BridgeResult.Error -> {
                warn("Failed to load app sync config: ${result.fullEnvelope}")
                SyncConfig().normalize()
            }
            BridgeResult.NotLoaded -> SyncConfig().normalize()
        }

    fun saveAppSyncConfig(config: SyncConfig): SettingsSaveResult =
        when (val result = syncBridge.saveAppSyncConfig(config)) {
            is BridgeResult.Success -> SettingsSaveResult.Success
            is BridgeResult.Error -> {
                warn("Failed to save app sync config: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
        }

    fun loadAppSyncSecrets(): SyncSecrets =
        when (val result = syncBridge.loadAppSyncSecrets()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to load app sync secrets: ${result.fullEnvelope}")
                SyncSecrets()
            }
            BridgeResult.NotLoaded -> SyncSecrets()
        }

    fun saveAppSyncSecrets(secrets: SyncSecrets): SettingsSaveResult =
        when (val result = syncBridge.saveAppSyncSecrets(secrets)) {
            is BridgeResult.Success -> SettingsSaveResult.Success
            is BridgeResult.Error -> {
                warn("Failed to save app sync secrets: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
        }

    fun performAppSync(
        config: SyncConfig,
        forceSync: Boolean = false,
    ): BridgeResult<SyncResult> =
        syncBridge.performAppSync(
            config,
            forceSync,
        )

    fun performAppSyncDryRun(config: SyncConfig): BridgeResult<SyncPlan> = syncBridge.performAppSyncDryRun(config)

    fun performAppSyncDiagnostics(config: SyncConfig): BridgeResult<SyncDiagnosticsResult> =
        syncBridge.performAppSyncDiagnostics(
            config,
        )

    fun performAppSyncDryRunTyped(config: SyncConfig): SyncDryRunOutcome =
        when (val result = performAppSyncDryRun(config)) {
            is BridgeResult.Success -> SyncDryRunOutcome.Success(result.data)
            is BridgeResult.Error ->
                SyncDryRunOutcome.Error(
                    syncFailureKind = result.syncFailureKind ?: SyncFailureKind.Fatal,
                    message = result.message,
                )
            BridgeResult.NotLoaded -> SyncDryRunOutcome.NotLoaded
        }

    fun performAppSyncDiagnosticsTyped(config: SyncConfig): SyncDiagnosticsOutcome =
        when (val result = performAppSyncDiagnostics(config)) {
            is BridgeResult.Success -> SyncDiagnosticsOutcome.Success(result.data)
            is BridgeResult.Error ->
                SyncDiagnosticsOutcome.Error(
                    syncFailureKind = result.syncFailureKind ?: SyncFailureKind.Fatal,
                    message = result.message,
                )
            BridgeResult.NotLoaded -> SyncDiagnosticsOutcome.NotLoaded
        }

    fun loadAppSyncState(): SyncState =
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

    fun loadAppSyncConfigStrict(): SyncConfig? =
        when (val result = syncBridge.loadAppSyncConfig()) {
            is BridgeResult.Success -> result.data?.normalize()
            is BridgeResult.Error -> {
                warn("Strict load app sync config failed: ${result.fullEnvelope}")
                null
            }
            BridgeResult.NotLoaded -> null
        }

    fun loadLegacyAppSyncSecretsTyped(): GenerationSecretsReadResult =
        when (val result = syncBridge.loadAppSyncSecrets()) {
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
                warn("Legacy app sync secrets read failed: ${result.fullEnvelope}")
                GenerationSecretsReadResult.Failed(kind, result.fullEnvelope)
            }
            BridgeResult.NotLoaded ->
                GenerationSecretsReadResult.Failed(
                    SyncFailureKind.NativeUnavailable,
                    nativeUnavailableMessage,
                )
        }

    fun saveAppSyncSecretsForGeneration(
        generation: Long,
        secrets: SyncSecrets,
    ): SettingsSaveResult =
        when (val result = appBridge.saveAppSyncSecretsForGeneration(generation.toULong(), secrets)) {
            is BridgeResult.Success -> SettingsSaveResult.Success
            is BridgeResult.Error -> {
                warn("Failed to save staged app sync secrets for generation $generation: ${result.fullEnvelope}")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, generation)))
            }
            BridgeResult.NotLoaded -> SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, generation)))
        }

    fun deleteAppSyncSecretsForGeneration(generation: Long): BridgeResult<Unit> =
        appBridge.deleteAppSyncSecretsForGeneration(
            generation.toULong(),
        )

    fun loadAppSyncSecretsForGeneration(generation: Long): GenerationSecretsReadResult =
        when (val result = appBridge.loadAppSyncSecretsForGeneration(generation.toULong())) {
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
                warn("Failed to load staged app sync secrets for generation $generation: ${result.fullEnvelope}")
                GenerationSecretsReadResult.Failed(kind, result.fullEnvelope)
            }
            BridgeResult.NotLoaded ->
                GenerationSecretsReadResult.Failed(
                    SyncFailureKind.NativeUnavailable,
                    nativeUnavailableMessage,
                )
        }

    suspend fun snapshotAppSyncProfile(): AppSyncProfileReadResult {
        val state = appProfileStore.readState()
        val config =
            if (state.hasCommittedProfile) {
                runCatching { configJson.fromJson(state.committedConfigJson, SyncConfig::class.java) }
                    .getOrNull()?.normalize()
                    ?: return AppSyncProfileReadResult.Failed(
                        SyncFailureKind.Fatal, "Committed app config JSON parse failed",
                    )
            } else {
                loadAppSyncConfigStrict()
                    ?: return AppSyncProfileReadResult.Failed(SyncFailureKind.Fatal, "App sync config load failed")
            }
        val generationSecrets = loadAppSyncSecretsForGeneration(state.activeGeneration)
        val secrets: SyncSecrets =
            when (generationSecrets) {
                is GenerationSecretsReadResult.Found -> generationSecrets.secrets
                is GenerationSecretsReadResult.NotConfigured -> {
                    if (state.hasCommittedProfile) {
                        SyncSecrets()
                    } else {
                        when (val legacy = loadLegacyAppSyncSecretsTyped()) {
                            is GenerationSecretsReadResult.Found -> legacy.secrets
                            is GenerationSecretsReadResult.NotConfigured -> SyncSecrets()
                            is GenerationSecretsReadResult.Failed -> return AppSyncProfileReadResult.Failed(
                                legacy.kind,
                                legacy.message,
                            )
                        }
                    }
                }
                is GenerationSecretsReadResult.Failed -> return AppSyncProfileReadResult.Failed(
                    generationSecrets.kind,
                    generationSecrets.message,
                )
            }
        val snapshot = AppSyncProfileSnapshot(state.activeGeneration, config, secrets)
        return if (secrets.token?.isNotEmpty() == true) {
            AppSyncProfileReadResult.Found(snapshot)
        } else {
            AppSyncProfileReadResult.NotConfigured(snapshot)
        }
    }

    suspend fun loadCommittedAppSyncProfile(): AppSyncProfileReadResult =
        SyncProfileGate.snapshotExclusive {
            snapshotAppSyncProfile()
        }

    private suspend fun migrateLegacyAppProfileIfNeeded(): SettingsSaveResult? {
        val initialState = appProfileStore.readState()
        if (initialState.hasCommittedProfile) return null
        val legacyConfig = loadAppSyncConfigStrict()
        if (legacyConfig == null) {
            warn("commitAppSyncProfile: strict read of legacy app config failed - aborting migration before any write")
            return SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_CONFIG, 0L)))
        }
        val legacySecrets = loadLegacyAppSyncSecretsTyped()
        if (legacySecrets is GenerationSecretsReadResult.Failed) {
            warn("commitAppSyncProfile: legacy app secrets read failed - aborting migration before any write")
            return SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.SYNC_SECRETS, 0L)))
        }
        val migrationSecrets =
            if (legacySecrets is GenerationSecretsReadResult.Found) legacySecrets.secrets else SyncSecrets()
        val migrationGeneration = appProfileStore.nextGeneration()
        val migrationResult = saveAppSyncSecretsForGeneration(migrationGeneration, migrationSecrets)
        if (migrationResult is SettingsSaveResult.Failed) {
            warn(
                "commitAppSyncProfile: legacy app secrets migration to generation $migrationGeneration" +
                    " failed - aborting",
            )
            return migrationResult
        }
        appProfileStore.stageConfig(migrationGeneration, configJson.toJson(legacyConfig.normalize()))
        appProfileStore.stageSecrets(migrationGeneration)
        appProfileStore.commitGeneration(migrationGeneration, configJson.toJson(legacyConfig.normalize()))
        return null
    }

    private suspend fun cleanupStaleAppGenerationCredentials(current: Long): SettingsSaveResult {
        val failures = mutableListOf<SaveFailure>()
        val range = generationCleanupRange(current)
        if (range != null) {
            for (gen in range) {
                when (appBridge.deleteAppSyncSecretsForGeneration(gen.toULong())) {
                    is BridgeResult.Success -> { }
                    is BridgeResult.Error -> failures.add(SaveFailure(SaveField.SYNC_SECRETS, gen))
                    BridgeResult.NotLoaded -> failures.add(SaveFailure(SaveField.SYNC_SECRETS, gen))
                }
            }
        }
        try {
            appProfileStore.clearStaleStagedMarkers(current)
        } catch (e: Exception) {
            warn("cleanupStaleAppGenerationCredentials: failed to clear stale staged markers: ${e.message}")
        }
        return if (failures.isEmpty()) SettingsSaveResult.Success else SettingsSaveResult.Failed(failures)
    }

    suspend fun commitAppSyncProfile(
        config: SyncConfig,
        secrets: SyncSecrets,
    ): SettingsSaveResult {
        val committed =
            SyncProfileGate.commitExclusive {
                migrateLegacyAppProfileIfNeeded()?.let { return@commitExclusive it }
                val generation = appProfileStore.nextGeneration()
                val normalized = config.normalize()
                appProfileStore.stageConfig(generation, configJson.toJson(normalized))
                val secretsResult = saveAppSyncSecretsForGeneration(generation, secrets)
                if (secretsResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitAppSyncProfile: staged app secrets save failed for generation $generation" +
                            " - active generation unchanged",
                    )
                    return@commitExclusive secretsResult
                }
                appProfileStore.stageSecrets(generation)
                appProfileStore.commitGeneration(generation, configJson.toJson(normalized))
                val cleanupResult = cleanupStaleAppGenerationCredentials(generation)
                if (cleanupResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitAppSyncProfile: app generation cleanup reported typed failures: " +
                            "${cleanupResult.failures.joinToString { "gen=${it.revision} field=${it.field}" }}",
                    )
                }
                val liveConfigResult = saveAppSyncConfig(normalized)
                if (liveConfigResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitAppSyncProfile: live app config mirror update failed for generation $generation",
                    )
                }
                val liveSecretsResult = saveAppSyncSecrets(secrets)
                if (liveSecretsResult is SettingsSaveResult.Failed) {
                    warn(
                        "commitAppSyncProfile: live app secrets mirror update failed for generation $generation",
                    )
                }
                SettingsSaveResult.Success
            }
        if (committed is SettingsSaveResult.Success) AutoSyncScheduler.scheduleFromSettings(appContext, this)
        return committed
    }
}

sealed class SyncDryRunOutcome {
    data class Success(val plan: com.xiwei.sujian.feature.sync.data.model.SyncPlan) : SyncDryRunOutcome()

    data class Error(val syncFailureKind: SyncFailureKind, val message: String) : SyncDryRunOutcome()

    data object NotLoaded : SyncDryRunOutcome()
}

sealed class SyncDiagnosticsOutcome {
    data class Success(
        val result: com.xiwei.sujian.feature.sync.data.model.SyncDiagnosticsResult,
    ) : SyncDiagnosticsOutcome()

    data class Error(val syncFailureKind: SyncFailureKind, val message: String) : SyncDiagnosticsOutcome()

    data object NotLoaded : SyncDiagnosticsOutcome()
}
