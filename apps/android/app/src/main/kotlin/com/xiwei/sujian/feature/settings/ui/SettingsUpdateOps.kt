package com.xiwei.sujian.feature.settings.ui

// ! # 设置更新/入队/调色板/初始 profile 操作（#6003 detekt 从 SettingsViewModel 与 SettingsSyncOps/SettingsSaveOps 拆分，降低 TooManyFunctions）
//
// #630 评论 #1+#2：同步配置只有一份 — 全量同步覆盖设置/星图/主题/全部作品。

import com.xiwei.sujian.feature.sync.data.SyncRepository
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** 更新同步配置 — 入队 SyncConfig 保存命令。 */
internal fun SettingsViewModel.updateSyncConfig(config: com.xiwei.sujian.feature.sync.data.model.SyncConfig) {
    _uiState.update { it.copy(syncConfig = config) }
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Save(SettingsSaveCommand.SyncConfig(config, ++syncConfigRevision)),
    )
}

/** 更新同步凭据 — 入队 SyncSecrets 保存命令。 */
internal fun SettingsViewModel.updateSyncSecrets(secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets) {
    _uiState.update { it.copy(syncSecrets = secrets) }
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Save(SettingsSaveCommand.SyncSecrets(secrets, ++syncSecretsRevision)),
    )
}

/** 入队 DryRun 事务（保存配置后跑全量试运行）。 */
internal fun SettingsViewModel.enqueueDryRun() {
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Transaction(
            SettingsTransactionCommand.SaveAndRunDryRun(
                config = _uiState.value.syncConfig,
                configRevision = syncConfigRevision,
                secrets = _uiState.value.syncSecrets,
                secretsRevision = syncSecretsRevision,
            ),
        ),
    )
}

/** 入队 TestConnection 事务（保存配置后跑全量连接诊断）。 */
internal fun SettingsViewModel.enqueueTestConnection() {
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Transaction(
            SettingsTransactionCommand.SaveAndRunDiagnostics(
                config = _uiState.value.syncConfig,
                configRevision = syncConfigRevision,
                secrets = _uiState.value.syncSecrets,
                secretsRevision = syncSecretsRevision,
            ),
        ),
    )
}

/** 入队 PerformSync 事务（保存配置后跑全量同步）。 */
internal fun SettingsViewModel.enqueuePerformSync() {
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Transaction(
            SettingsTransactionCommand.SaveAndRunSync(
                config = _uiState.value.syncConfig,
                configRevision = syncConfigRevision,
                secrets = _uiState.value.syncSecrets,
                secretsRevision = syncSecretsRevision,
                trigger = SyncTrigger.SettingsPage,
            ),
        ),
    )
}

// #6003 detekt：handleIntent 的 action 分支拆出为 extension — 降低 CyclomaticComplexity 与 TooManyFunctions。
internal fun SettingsViewModel.handleActionIntent(intent: SettingsIntent) {
    when (intent) {
        is SettingsIntent.Refresh -> mergeRefresh()
        is SettingsIntent.CaptureDynamicColor -> refreshPaletteRecords()
        is SettingsIntent.DeletePalette -> deletePaletteRecord(intent.deviceId, intent.fingerprint)
        is SettingsIntent.DryRun -> enqueueDryRun()
        is SettingsIntent.TestConnection -> enqueueTestConnection()
        is SettingsIntent.PerformSync -> enqueuePerformSync()
        else -> { }
    }
}

// #6003 detekt：从 SettingsViewModel 移出的调色板操作 extension — 降低 TooManyFunctions。
internal fun SettingsViewModel.refreshPaletteRecords() {
    val repo = themeRepo
    editorScope.launch {
        val records = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
        _uiState.update { it.copy(paletteRecords = records) }
    }
}

internal fun SettingsViewModel.deletePaletteRecord(
    deviceId: String,
    fingerprint: String,
) {
    val repo = themeRepo
    editorScope.launch {
        withContext(Dispatchers.IO) { repo.deletePaletteRecord(deviceId, fingerprint) }
        val records = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
        _uiState.update { it.copy(paletteRecords = records) }
    }
}

/** 加载初始同步 profile — 从 loadInitialSnapshot 拆分降低认知复杂度。 */
internal suspend fun SettingsViewModel.loadInitialSyncProfile(
    repo: SyncRepository,
): Pair<SyncProfileLoadState, com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData> {
    val committedProfile = withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile() }
    val syncProfileLoadState = committedProfile.toSyncProfileLoadState()
    val syncCapability = withContext(Dispatchers.IO) { repo.getSyncCapability() }
    return syncProfileLoadState to syncCapability
}

/** 返回 save command 的 revision（config 或 secrets 子类型），其他类型返回 null。 */
internal fun saveCommandRevision(command: SettingsSaveCommand?): Long? =
    when (command) {
        is SettingsSaveCommand.SyncConfig -> command.revision
        is SettingsSaveCommand.SyncSecrets -> command.revision
        else -> null
    }

/** 返回当前 config revision — 由 SaveCommand 类型决定。 */
internal fun SettingsViewModel.currentSyncConfigRevision(command: SettingsSaveCommand?): Long =
    when (command) {
        is SettingsSaveCommand.SyncConfig -> syncConfigRevision
        else -> 0L
    }

/** 返回当前 secrets revision — 由 SaveCommand 类型决定。 */
internal fun SettingsViewModel.currentSyncSecretsRevision(command: SettingsSaveCommand?): Long =
    when (command) {
        is SettingsSaveCommand.SyncSecrets -> syncSecretsRevision
        else -> 0L
    }
