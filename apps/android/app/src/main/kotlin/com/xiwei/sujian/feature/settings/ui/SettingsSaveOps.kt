package com.xiwei.sujian.feature.settings.ui

// ! # 设置保存操作（从 SettingsRoute 拆分）

import com.xiwei.sujian.R
import com.xiwei.sujian.feature.settings.data.SaveFailure
import com.xiwei.sujian.feature.settings.data.SaveField
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.settings.data.SettingsSaveResult
import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import com.xiwei.sujian.feature.sync.data.SyncRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

fun SettingsViewModel.loadInitial() {
    val repo = settingsRepo
    editorScope.launch { loadInitialSnapshot(repo) }
}

/**
 * #600 评论 #7: 外部同步拉取设置/主题后, 重新从 Core 加载设置状态.
 * 监听 CoreSettingsEvents.settingsChanged 触发, 复用 loadInitial 的字段集,
 * 不新建第二套事件系统.
 */
suspend fun SettingsViewModel.reloadFromExternalSync() {
    val repo = settingsRepo
    val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
    val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
    val paletteRecords = withContext(Dispatchers.IO) { themeRepo.listPaletteRecords() }
    _uiState.update {
        it.copy(
            settings = settings,
            fontSize = fontSize,
            paletteRecords = paletteRecords,
        )
    }
}

/**
 * #600 评论 #3 问题二：按活动作品读取 committed profile — 无活动作品时返回 Failed。
 */
internal suspend fun SettingsViewModel.loadCommittedProfileForProject(
    repo: SyncRepository,
    projectId: String?,
): com.xiwei.sujian.feature.sync.data.SyncProfileReadResult =
    if (projectId != null) {
        withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile(projectId) }
    } else {
        com.xiwei.sujian.feature.sync.data.SyncProfileReadResult.Failed(
            com.xiwei.sujian.feature.sync.data.SyncFailureKind.Fatal,
            MSG_NO_ACTIVE_PROJECT,
        )
    }

/**
 * #600 评论 #3 问题二：按活动作品读取 sync capability — 无活动作品时返回默认。
 */
internal suspend fun SettingsViewModel.loadSyncCapabilityForProject(
    repo: SyncRepository,
    projectId: String?,
): com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData =
    if (projectId != null) {
        withContext(Dispatchers.IO) { repo.getSyncCapability(projectId) }
    } else {
        com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData()
    }

/**
 * 加载初始快照 — 从 loadInitial 拆分以降低认知复杂度。
 * 读取活动 generation 的完整 snapshot（#595 八），不再读 live legacy 槽。
 * #6003 detekt：profile 读取拆分到 loadInitialProjectSyncProfile / loadInitialAppSyncProfile，
 * if 分支收敛到 buildInitialUiState，降低 CognitiveComplexity。
 */
private suspend fun SettingsViewModel.loadInitialSnapshot(repo: SettingsRepository) {
    val snapshotRevisions =
        InitialSnapshotRevisions(
            local = localRevision,
            fontSize = fontSizeRevision,
            projectSyncConfig = projectSyncConfigRevision,
            projectSyncSecrets = projectSyncSecretsRevision,
            appSyncConfig = appSyncConfigRevision,
            appSyncSecrets = appSyncSecretsRevision,
        )
    val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
    val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
    val (projectSyncProfileLoadState, projectSyncCapability) = loadInitialProjectSyncProfile(syncRepo)
    val appSyncProfileLoadState = loadInitialAppSyncProfile(syncRepo)
    val loaded =
        InitialLoadedValues(
            settings = settings,
            fontSize = fontSize,
            projectSyncProfileLoadState = projectSyncProfileLoadState,
            projectSyncCapability = projectSyncCapability,
            appSyncProfileLoadState = appSyncProfileLoadState,
            secureStorageWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() },
            builtinThemes = withContext(Dispatchers.IO) { themeRepo.listBuiltinThemes() },
            paletteRecords = withContext(Dispatchers.IO) { themeRepo.listPaletteRecords() },
            aiAvailable = withContext(Dispatchers.IO) { repo.aiAvailable() },
            dataRootPath =
                withContext(
                    Dispatchers.IO,
                ) { com.xiwei.sujian.core.platform.storage.AndroidDataRoot.rootDir().absolutePath },
        )
    _uiState.update { current -> buildInitialUiState(current, loaded, snapshotRevisions) }
}

/** 组装初始 UiState — 从 loadInitialSnapshot 拆分，把 if 分支收敛到此 helper 降低认知复杂度。 */
private fun SettingsViewModel.buildInitialUiState(
    current: SettingsUiState,
    loaded: InitialLoadedValues,
    snapshotRevisions: InitialSnapshotRevisions,
): SettingsUiState =
    SettingsUiState(
        settings = if (localRevision == snapshotRevisions.local) loaded.settings else current.settings,
        fontSize = if (fontSizeRevision == snapshotRevisions.fontSize) loaded.fontSize else current.fontSize,
        projectSyncConfig =
            if (projectSyncConfigRevision == snapshotRevisions.projectSyncConfig) {
                loaded.projectSyncProfileLoadState.confirmedConfig ?: current.projectSyncConfig
            } else {
                current.projectSyncConfig
            },
        projectSyncSecrets =
            if (projectSyncSecretsRevision == snapshotRevisions.projectSyncSecrets) {
                loaded.projectSyncProfileLoadState.confirmedSecrets ?: current.projectSyncSecrets
            } else {
                current.projectSyncSecrets
            },
        projectSyncCapability = loaded.projectSyncCapability,
        projectSyncProfileLoadState = loaded.projectSyncProfileLoadState,
        appSyncConfig =
            if (appSyncConfigRevision == snapshotRevisions.appSyncConfig) {
                loaded.appSyncProfileLoadState.confirmedConfig ?: current.appSyncConfig
            } else {
                current.appSyncConfig
            },
        appSyncSecrets =
            if (appSyncSecretsRevision == snapshotRevisions.appSyncSecrets) {
                loaded.appSyncProfileLoadState.confirmedSecrets ?: current.appSyncSecrets
            } else {
                current.appSyncSecrets
            },
        appSyncProfileLoadState = loaded.appSyncProfileLoadState,
        secureStorageWarning = loaded.secureStorageWarning,
        builtinThemes = loaded.builtinThemes,
        paletteRecords = loaded.paletteRecords,
        aiAvailable = loaded.aiAvailable,
        dataRootPath = loaded.dataRootPath,
    )

/** 初始加载读取的值 — 打包避免 buildInitialUiState LongParameterList。 */
private data class InitialLoadedValues(
    val settings: LocalSettings,
    val fontSize: Float,
    val projectSyncProfileLoadState: SyncProfileLoadState,
    val projectSyncCapability: com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData,
    val appSyncProfileLoadState: SyncProfileLoadState,
    val secureStorageWarning: String?,
    val builtinThemes: List<com.xiwei.sujian.app.theme.model.BuiltinTheme>,
    val paletteRecords: List<com.xiwei.sujian.app.theme.model.ThemePaletteRecord>,
    val aiAvailable: Boolean,
    val dataRootPath: String,
)

/** 初始加载开始时各字段的 revision 快照 — 打包避免 buildInitialUiState LongParameterList。 */
private data class InitialSnapshotRevisions(
    val local: Long,
    val fontSize: Long,
    val projectSyncConfig: Long,
    val projectSyncSecrets: Long,
    val appSyncConfig: Long,
    val appSyncSecrets: Long,
)

suspend fun SettingsViewModel.flushPending() {
    val repo = settingsRepo
    val cmds = pendingCommands
    if (cmds.isEmpty()) return
    pendingCommands = PendingCommands()
    executeSave(repo, cmds)
}

internal suspend fun SettingsViewModel.executeSave(
    repo: SettingsRepository,
    commands: PendingCommands,
) {
    val failures = mutableListOf<SaveFailure>()

    saveLocalField(repo, commands.local, failures)
    saveFontSizeField(repo, commands.fontSize, failures)
    val (projConfigSaved, projSecretsSaved) =
        saveSyncProfileField(syncRepo, commands.projectSyncConfig, commands.projectSyncSecrets, failures)
    val (appConfigSaved, appSecretsSaved) =
        saveAppSyncProfileField(syncRepo, commands.appSyncConfig, commands.appSyncSecrets, failures)

    if (projConfigSaved || projSecretsSaved) {
        // #595 五：成功提交后一次性更新作品级 config/secrets/loadState/capability/warning。
        refreshSyncProfileState()
    }
    if (appConfigSaved || appSecretsSaved) {
        // #600 评论 #5：成功提交后一次性更新应用级 config/secrets/loadState。
        refreshAppSyncProfileState()
    }

    handleSaveOutcome(repo, failures, commands)
}

/** 保存 local settings 字段 — 从 executeSave 拆分。 */
private suspend fun SettingsViewModel.saveLocalField(
    repo: SettingsRepository,
    local: SettingsSaveCommand.Local?,
    failures: MutableList<SaveFailure>,
) {
    if (local == null) return
    val result = withContext(Dispatchers.IO) { repo.saveLocalSettings(local.settings) }
    when (result) {
        is SettingsSaveResult.Success -> {
            localPersistedRevision = local.revision
            com.xiwei.sujian.app.theme.ThemeStore.reload()
        }
        is SettingsSaveResult.Failed -> {
            if (localRevision == local.revision) {
                failures.add(SaveFailure(SaveField.LOCAL_SETTINGS, local.revision))
            }
        }
    }
}

/** 保存 fontSize 字段 — 从 executeSave 拆分。 */
private suspend fun SettingsViewModel.saveFontSizeField(
    repo: SettingsRepository,
    fontSize: SettingsSaveCommand.FontSize?,
    failures: MutableList<SaveFailure>,
) {
    if (fontSize == null) return
    val result = withContext(Dispatchers.IO) { repo.setFontSize(fontSize.fontSize) }
    when (result) {
        is SettingsSaveResult.Success -> {
            fontSizePersistedRevision = fontSize.revision
        }
        is SettingsSaveResult.Failed -> {
            if (fontSizeRevision == fontSize.revision) {
                failures.add(SaveFailure(SaveField.FONT_SIZE, fontSize.revision))
            }
        }
    }
}

/**
 * 原子提交同步配置/凭据 — 从 executeSave 拆分。
 * #595 八：捕获同一时刻的完整 SyncProfileDraft，通过 commitSyncProfile 一次性原子提交，
 * 避免分别排队绕过 generation 提交协议造成 live 槽与 committed profile 双真相。
 * 返回 (configSaved, secretsSaved)。
 */
private suspend fun SettingsViewModel.saveSyncProfileField(
    repo: SyncRepository,
    syncConfig: SettingsSaveCommand.ProjectSyncConfig?,
    syncSecrets: SettingsSaveCommand.ProjectSyncSecrets?,
    failures: MutableList<SaveFailure>,
): Pair<Boolean, Boolean> {
    if (syncConfig == null && syncSecrets == null) return false to false
    // #600 评论 #3 问题二：commitSyncProfile 按当前活动作品路由 —
    // 无活动作品时直接失败，不写入任何作品。
    val projectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
    if (projectId == null) {
        failures.add(SaveFailure(SaveField.SYNC_CONFIG, syncConfig?.revision ?: 0L))
        return false to false
    }
    val draftConfig = syncConfig?.config ?: _uiState.value.projectSyncConfig
    val draftSecrets = syncSecrets?.secrets ?: _uiState.value.projectSyncSecrets
    val commitResult =
        withContext(Dispatchers.IO) {
            repo.commitSyncProfile(projectId, draftConfig, draftSecrets)
        }
    var syncConfigSaved = false
    var syncSecretsSaved = false
    when (commitResult) {
        is SettingsSaveResult.Success -> {
            if (syncConfig != null && projectSyncConfigRevision == syncConfig.revision) {
                projectSyncConfigPersistedRevision = syncConfig.revision
                syncConfigSaved = true
            }
            if (syncSecrets != null && projectSyncSecretsRevision == syncSecrets.revision) {
                projectSyncSecretsPersistedRevision = syncSecrets.revision
                syncSecretsSaved = true
            }
        }
        is SettingsSaveResult.Failed -> {
            collectSyncProfileFailures(commitResult.failures, syncConfig, syncSecrets, failures)
        }
    }
    return syncConfigSaved to syncSecretsSaved
}

/**
 * #600 评论 #5：原子提交应用级同步配置/凭据 — 提交到应用级 profile（不带 projectId）。
 * 返回 (configSaved, secretsSaved)。
 */
private suspend fun SettingsViewModel.saveAppSyncProfileField(
    repo: SyncRepository,
    syncConfig: SettingsSaveCommand.AppSyncConfig?,
    syncSecrets: SettingsSaveCommand.AppSyncSecrets?,
    failures: MutableList<SaveFailure>,
): Pair<Boolean, Boolean> {
    if (syncConfig == null && syncSecrets == null) return false to false
    val draftConfig = syncConfig?.config ?: _uiState.value.appSyncConfig
    val draftSecrets = syncSecrets?.secrets ?: _uiState.value.appSyncSecrets
    val commitResult =
        withContext(Dispatchers.IO) {
            repo.commitAppSyncProfile(draftConfig, draftSecrets)
        }
    var syncConfigSaved = false
    var syncSecretsSaved = false
    when (commitResult) {
        is SettingsSaveResult.Success -> {
            if (syncConfig != null && appSyncConfigRevision == syncConfig.revision) {
                appSyncConfigPersistedRevision = syncConfig.revision
                syncConfigSaved = true
            }
            if (syncSecrets != null && appSyncSecretsRevision == syncSecrets.revision) {
                appSyncSecretsPersistedRevision = syncSecrets.revision
                syncSecretsSaved = true
            }
        }
        is SettingsSaveResult.Failed -> {
            collectSyncProfileFailures(commitResult.failures, syncConfig, syncSecrets, failures)
        }
    }
    return syncConfigSaved to syncSecretsSaved
}

/** 收集同步配置提交失败 — 从 saveSyncProfileField 拆分。 */
private fun SettingsViewModel.collectSyncProfileFailures(
    commitFailures: List<SaveFailure>,
    syncConfig: SettingsSaveCommand?,
    syncSecrets: SettingsSaveCommand?,
    failures: MutableList<SaveFailure>,
) {
    val syncConfigRev = saveCommandRevision(syncConfig)
    val syncSecretsRev = saveCommandRevision(syncSecrets)
    commitFailures.forEach { failure ->
        collectFailureIfRevisionMatches(failure, syncConfig, syncSecrets, syncConfigRev, syncSecretsRev, failures)
    }
}

/** 判断单条提交失败是否匹配当前 revision，匹配则收集 — 从 collectSyncProfileFailures 拆分降低圈复杂度。 */
private fun SettingsViewModel.collectFailureIfRevisionMatches(
    failure: SaveFailure,
    syncConfig: SettingsSaveCommand?,
    syncSecrets: SettingsSaveCommand?,
    syncConfigRev: Long?,
    syncSecretsRev: Long?,
    failures: MutableList<SaveFailure>,
) {
    when (failure.field) {
        SaveField.SYNC_CONFIG -> {
            if (syncConfigRev != null && syncConfigRev == currentSyncConfigRevision(syncConfig)) {
                failures.add(failure)
            }
        }
        SaveField.SYNC_SECRETS -> {
            if (syncSecretsRev != null && syncSecretsRev == currentSyncSecretsRevision(syncSecrets)) {
                failures.add(failure)
            }
        }
        else -> { }
    }
}

/** 处理保存结果：失败回滚+报错 或 成功清错 — 从 executeSave 拆分。 */
private suspend fun SettingsViewModel.handleSaveOutcome(
    repo: SettingsRepository,
    failures: List<SaveFailure>,
    commands: PendingCommands,
) {
    if (failures.isNotEmpty()) {
        rollbackFailures(repo, failures)
        val errorResId =
            when (failures.first().field) {
                SaveField.LOCAL_SETTINGS -> R.string.save_local_settings_failed
                SaveField.FONT_SIZE -> R.string.save_font_size_failed
                SaveField.SYNC_CONFIG -> R.string.save_sync_config_failed
                SaveField.SYNC_SECRETS -> R.string.save_sync_secrets_failed
            }
        _uiState.update { it.copy(saveErrorResId = errorResId) }
        _saveFailureEvents.send(errorResId)
    } else if (!commands.isEmpty()) {
        _uiState.update { it.copy(saveErrorResId = null) }
    }
}

suspend fun SettingsViewModel.rollbackFailures(
    repo: SettingsRepository,
    failures: List<SaveFailure>,
) {
    for (failure in failures) {
        rollbackIfRevisionMatches(repo, failure)
    }
}

// #597 回滚按字段分支检查 revision 后原子恢复，4 分支结构对称；拆分为4个单行方法反而降低可读性 — 待后续重构
// #597：回滚按字段分支校验 revision 后原子恢复 — 各字段结构对称，
// 分支体收敛到独立私有函数，分发器只保留字段→函数的映射。
suspend fun SettingsViewModel.rollbackIfRevisionMatches(
    repo: SettingsRepository,
    failure: SaveFailure,
) {
    when (failure.field) {
        SaveField.LOCAL_SETTINGS -> rollbackLocalSettings(repo, failure.revision)
        SaveField.FONT_SIZE -> rollbackFontSize(repo, failure.revision)
        SaveField.SYNC_CONFIG -> rollbackSyncConfig(syncRepo, failure.revision)
        SaveField.SYNC_SECRETS -> rollbackSyncSecrets(syncRepo, failure.revision)
    }
}

private suspend fun SettingsViewModel.rollbackLocalSettings(
    repo: SettingsRepository,
    expectedRevision: Long,
) {
    if (localRevision != expectedRevision) return
    val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
    if (localRevision != expectedRevision) return
    localRevision = localPersistedRevision
    _uiState.update { it.copy(settings = settings) }
}

private suspend fun SettingsViewModel.rollbackFontSize(
    repo: SettingsRepository,
    expectedRevision: Long,
) {
    if (fontSizeRevision != expectedRevision) return
    val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
    if (fontSizeRevision != expectedRevision) return
    fontSizeRevision = fontSizePersistedRevision
    _uiState.update { it.copy(fontSize = fontSize) }
}

private suspend fun SettingsViewModel.rollbackSyncConfig(
    repo: SyncRepository,
    expectedRevision: Long,
) {
    if (projectSyncConfigRevision != expectedRevision) return
    // #595 八/五：回滚读取活动 generation 的完整 snapshot，不再读 live 槽；
    // 类型化处理 — Failed 保留当前 UI 值（不静默退化为默认值/null）。
    // #600 评论 #3 问题二：按当前活动作品路由。
    val rollbackProjectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
    val profile =
        if (rollbackProjectId != null) {
            withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile(rollbackProjectId) }
        } else {
            com.xiwei.sujian.feature.sync.data.SyncProfileReadResult.Failed(
                com.xiwei.sujian.feature.sync.data.SyncFailureKind.Fatal,
                MSG_NO_ACTIVE_PROJECT,
            )
        }
    if (projectSyncConfigRevision != expectedRevision) return
    projectSyncConfigRevision = projectSyncConfigPersistedRevision
    _uiState.update {
        it.copy(projectSyncConfig = profile.toSyncProfileLoadState().confirmedConfig ?: it.projectSyncConfig)
    }
}

private suspend fun SettingsViewModel.rollbackSyncSecrets(
    repo: SyncRepository,
    expectedRevision: Long,
) {
    if (projectSyncSecretsRevision != expectedRevision) return
    // #600 评论 #3 问题二：按当前活动作品路由。
    val rollbackSecretsProjectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
    val profile =
        if (rollbackSecretsProjectId != null) {
            withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile(rollbackSecretsProjectId) }
        } else {
            com.xiwei.sujian.feature.sync.data.SyncProfileReadResult.Failed(
                com.xiwei.sujian.feature.sync.data.SyncFailureKind.Fatal,
                MSG_NO_ACTIVE_PROJECT,
            )
        }
    if (projectSyncSecretsRevision != expectedRevision) return
    projectSyncSecretsRevision = projectSyncSecretsPersistedRevision
    _uiState.update {
        it.copy(
            projectSyncSecrets = profile.toSyncProfileLoadState().confirmedSecrets ?: it.projectSyncSecrets,
        )
    }
}
