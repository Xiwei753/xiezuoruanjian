package com.xiwei.sujian.feature.settings.ui

// ! # 设置更新/入队/调色板/初始 profile 操作（#6003 detekt 从 SettingsViewModel 与 SettingsSyncOps/SettingsSaveOps 拆分，降低 TooManyFunctions）

import com.xiwei.sujian.feature.sync.data.SyncRepository
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// #6003 detekt：从 SettingsViewModel 移出的 update sync extension — 降低 TooManyFunctions。
internal fun SettingsViewModel.updateProjectSyncConfig(config: com.xiwei.sujian.feature.sync.data.model.SyncConfig) {
    _uiState.update { it.copy(projectSyncConfig = config) }
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Save(SettingsSaveCommand.ProjectSyncConfig(config, ++projectSyncConfigRevision)),
    )
}

internal fun SettingsViewModel.updateProjectSyncSecrets(secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets) {
    _uiState.update { it.copy(projectSyncSecrets = secrets) }
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Save(SettingsSaveCommand.ProjectSyncSecrets(secrets, ++projectSyncSecretsRevision)),
    )
}

internal fun SettingsViewModel.updateAppSyncConfig(config: com.xiwei.sujian.feature.sync.data.model.SyncConfig) {
    _uiState.update { it.copy(appSyncConfig = config) }
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Save(SettingsSaveCommand.AppSyncConfig(config, ++appSyncConfigRevision)),
    )
}

internal fun SettingsViewModel.updateAppSyncSecrets(secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets) {
    _uiState.update { it.copy(appSyncSecrets = secrets) }
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Save(SettingsSaveCommand.AppSyncSecrets(secrets, ++appSyncSecretsRevision)),
    )
}

// #6003 detekt：从 SettingsViewModel 移出的 enqueue* extension — 降低 TooManyFunctions。
internal fun SettingsViewModel.enqueueDryRun() {
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Transaction(
            SettingsTransactionCommand.SaveAndRunDryRun(
                config = _uiState.value.projectSyncConfig,
                configRevision = projectSyncConfigRevision,
                secrets = _uiState.value.projectSyncSecrets,
                secretsRevision = projectSyncSecretsRevision,
            ),
        ),
    )
}

internal fun SettingsViewModel.enqueueTestConnection() {
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Transaction(
            SettingsTransactionCommand.SaveAndRunDiagnostics(
                config = _uiState.value.projectSyncConfig,
                configRevision = projectSyncConfigRevision,
                secrets = _uiState.value.projectSyncSecrets,
                secretsRevision = projectSyncSecretsRevision,
            ),
        ),
    )
}

internal fun SettingsViewModel.enqueuePerformSync() {
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Transaction(
            SettingsTransactionCommand.SaveAndRunSync(
                config = _uiState.value.projectSyncConfig,
                configRevision = projectSyncConfigRevision,
                secrets = _uiState.value.projectSyncSecrets,
                secretsRevision = projectSyncSecretsRevision,
                trigger = SyncTrigger.SettingsPage,
            ),
        ),
    )
}

internal fun SettingsViewModel.enqueueAppDryRun() {
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Transaction(
            SettingsTransactionCommand.SaveAndRunAppDryRun(
                config = _uiState.value.appSyncConfig,
                configRevision = appSyncConfigRevision,
                secrets = _uiState.value.appSyncSecrets,
                secretsRevision = appSyncSecretsRevision,
            ),
        ),
    )
}

internal fun SettingsViewModel.enqueueAppTestConnection() {
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Transaction(
            SettingsTransactionCommand.SaveAndRunAppDiagnostics(
                config = _uiState.value.appSyncConfig,
                configRevision = appSyncConfigRevision,
                secrets = _uiState.value.appSyncSecrets,
                secretsRevision = appSyncSecretsRevision,
            ),
        ),
    )
}

internal fun SettingsViewModel.enqueueAppPerformSync() {
    saveChannel.trySend(
        SettingsViewModel.QueueItem.Transaction(
            SettingsTransactionCommand.SaveAndRunAppSync(
                config = _uiState.value.appSyncConfig,
                configRevision = appSyncConfigRevision,
                secrets = _uiState.value.appSyncSecrets,
                secretsRevision = appSyncSecretsRevision,
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
        is SettingsIntent.AppDryRun -> enqueueAppDryRun()
        is SettingsIntent.AppTestConnection -> enqueueAppTestConnection()
        is SettingsIntent.AppPerformSync -> enqueueAppPerformSync()
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

/** 加载初始作品级同步 profile — 从 loadInitialSnapshot 拆分降低认知复杂度。 */
internal suspend fun SettingsViewModel.loadInitialProjectSyncProfile(
    repo: SyncRepository,
): Pair<SyncProfileLoadState, com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData> {
    // #600 评论 #3 问题二：profile/capability 按当前活动作品路由。
    val activeProjectId = com.xiwei.sujian.core.interop.project.ActiveProjectGate.currentProjectId()
    val committedProfile = loadCommittedProfileForProject(repo, activeProjectId)
    // #595 四：类型化加载状态 — Failed 时保留当前字段值，页面显示真实错误。
    val projectSyncProfileLoadState = committedProfile.toSyncProfileLoadState()
    val projectSyncCapability = loadSyncCapabilityForProject(repo, activeProjectId)
    return projectSyncProfileLoadState to projectSyncCapability
}

/** 加载初始应用级同步 profile — 从 loadInitialSnapshot 拆分降低认知复杂度。 */
internal suspend fun SettingsViewModel.loadInitialAppSyncProfile(repo: SyncRepository): SyncProfileLoadState {
    val committedAppProfile = withContext(Dispatchers.IO) { repo.loadCommittedAppSyncProfile() }
    return committedAppProfile.toAppSyncProfileLoadState()
}

/** 返回 save command 的 revision（config 或 secrets 子类型），其他类型返回 null。 */
internal fun saveCommandRevision(command: SettingsSaveCommand?): Long? =
    when (command) {
        is SettingsSaveCommand.ProjectSyncConfig -> command.revision
        is SettingsSaveCommand.AppSyncConfig -> command.revision
        is SettingsSaveCommand.ProjectSyncSecrets -> command.revision
        is SettingsSaveCommand.AppSyncSecrets -> command.revision
        else -> null
    }

/** 返回当前作品级或应用级 config revision — 由 SaveCommand 类型决定。 */
internal fun SettingsViewModel.currentSyncConfigRevision(command: SettingsSaveCommand?): Long =
    when (command) {
        is SettingsSaveCommand.ProjectSyncConfig -> projectSyncConfigRevision
        is SettingsSaveCommand.AppSyncConfig -> appSyncConfigRevision
        else -> 0L
    }

/** 返回当前作品级或应用级 secrets revision — 由 SaveCommand 类型决定。 */
internal fun SettingsViewModel.currentSyncSecretsRevision(command: SettingsSaveCommand?): Long =
    when (command) {
        is SettingsSaveCommand.ProjectSyncSecrets -> projectSyncSecretsRevision
        is SettingsSaveCommand.AppSyncSecrets -> appSyncSecretsRevision
        else -> 0L
    }
