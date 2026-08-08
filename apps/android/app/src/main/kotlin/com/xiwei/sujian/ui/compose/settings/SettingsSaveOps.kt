package com.xiwei.sujian.ui.compose.settings

// ! # 设置保存操作（从 SettingsRoute 拆分）

import com.xiwei.sujian.R
import com.xiwei.sujian.data.SaveFailure
import com.xiwei.sujian.data.SaveField
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SettingsSaveResult
import com.xiwei.sujian.model.LocalSettings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * 是否存在任意待保存命令 — 拆分复杂条件，避免 ComplexCondition。
 */
private fun hasAnySaveCommand(
    local: SettingsSaveCommand.Local?,
    fontSize: SettingsSaveCommand.FontSize?,
    syncConfig: SettingsSaveCommand.SyncConfig?,
    syncSecrets: SettingsSaveCommand.SyncSecrets?,
): Boolean = local != null || fontSize != null || syncConfig != null || syncSecrets != null

fun SettingsViewModel.loadInitial() {
    val repo = settingsRepo
    editorScope.launch { loadInitialSnapshot(repo) }
}

/**
 * #600 评论 #3 问题二：按活动作品读取 committed profile — 无活动作品时返回 Failed。
 */
private suspend fun SettingsViewModel.loadCommittedProfileForProject(
    repo: SettingsRepository,
    projectId: String?,
): com.xiwei.sujian.data.SyncProfileReadResult =
    if (projectId != null) {
        withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile(projectId) }
    } else {
        com.xiwei.sujian.data.SyncProfileReadResult.Failed(
            com.xiwei.sujian.data.SyncFailureKind.Fatal,
            MSG_NO_ACTIVE_PROJECT,
        )
    }

/**
 * #600 评论 #3 问题二：按活动作品读取 sync capability — 无活动作品时返回默认。
 */
private suspend fun SettingsViewModel.loadSyncCapabilityForProject(
    repo: SettingsRepository,
    projectId: String?,
): com.xiwei.sujian.model.SyncCapabilityData =
    if (projectId != null) {
        withContext(Dispatchers.IO) { repo.getSyncCapability(projectId) }
    } else {
        com.xiwei.sujian.model.SyncCapabilityData()
    }

/**
 * 加载初始快照 — 从 loadInitial 拆分以降低认知复杂度。
 * 读取活动 generation 的完整 snapshot（#595 八），不再读 live legacy 槽。
 */
private suspend fun SettingsViewModel.loadInitialSnapshot(repo: SettingsRepository) {
    val snapshotLocalRev = localRevision
    val snapshotFontSizeRev = fontSizeRevision
    val snapshotSyncConfigRev = syncConfigRevision
    val snapshotSyncSecretsRev = syncSecretsRevision

    val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
    val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
    // #600 评论 #3 问题二：profile/capability 按当前活动作品路由 — 提取为 helper 降低认知复杂度。
    val activeProjectId = com.xiwei.sujian.data.ActiveProjectGate.currentProjectId()
    val committedProfile = loadCommittedProfileForProject(repo, activeProjectId)
    // #595 四：类型化加载状态 — Failed 时保留当前字段值，页面显示真实错误。
    val syncProfileLoadState = committedProfile.toSyncProfileLoadState()
    val syncCapability = loadSyncCapabilityForProject(repo, activeProjectId)
    val secureStorageWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() }
    val builtinThemes = withContext(Dispatchers.IO) { repo.listBuiltinThemes() }
    val paletteRecords = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
    val aiAvailable = withContext(Dispatchers.IO) { repo.aiAvailable() }
    val dataRootPath = withContext(Dispatchers.IO) { repo.dataRootDir() }

    _uiState.update { current ->
        SettingsUiState(
            settings = if (localRevision == snapshotLocalRev) settings else current.settings,
            fontSize = if (fontSizeRevision == snapshotFontSizeRev) fontSize else current.fontSize,
            syncConfig =
                if (syncConfigRevision == snapshotSyncConfigRev) {
                    syncProfileLoadState.confirmedConfig ?: current.syncConfig
                } else {
                    current.syncConfig
                },
            syncSecrets =
                if (syncSecretsRevision == snapshotSyncSecretsRev) {
                    syncProfileLoadState.confirmedSecrets ?: current.syncSecrets
                } else {
                    current.syncSecrets
                },
            syncCapability = syncCapability,
            secureStorageWarning = secureStorageWarning,
            builtinThemes = builtinThemes,
            paletteRecords = paletteRecords,
            aiAvailable = aiAvailable,
            dataRootPath = dataRootPath,
            syncProfileLoadState = syncProfileLoadState,
        )
    }
}

suspend fun SettingsViewModel.flushPending() {
    val repo = settingsRepo
    val cmds = pendingCommands
    if (cmds.isEmpty()) return
    pendingCommands = PendingCommands()
    executeSave(repo, cmds.local, cmds.fontSize, cmds.syncConfig, cmds.syncSecrets)
}

suspend fun SettingsViewModel.executeSave(
    repo: SettingsRepository,
    local: SettingsSaveCommand.Local?,
    fontSize: SettingsSaveCommand.FontSize?,
    syncConfig: SettingsSaveCommand.SyncConfig?,
    syncSecrets: SettingsSaveCommand.SyncSecrets?,
) {
    val failures = mutableListOf<SaveFailure>()

    saveLocalField(repo, local, failures)
    saveFontSizeField(repo, fontSize, failures)
    val (syncConfigSaved, syncSecretsSaved) = saveSyncProfileField(repo, syncConfig, syncSecrets, failures)

    if (syncConfigSaved || syncSecretsSaved) {
        // #595 五：成功提交后一次性更新 config/secrets/loadState/capability/warning。
        refreshSyncProfileState()
    }

    handleSaveOutcome(repo, failures, local, fontSize, syncConfig, syncSecrets)
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
            com.xiwei.sujian.ui.compose.theme.ThemeStore.reload()
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
    repo: SettingsRepository,
    syncConfig: SettingsSaveCommand.SyncConfig?,
    syncSecrets: SettingsSaveCommand.SyncSecrets?,
    failures: MutableList<SaveFailure>,
): Pair<Boolean, Boolean> {
    if (syncConfig == null && syncSecrets == null) return false to false
    // #600 评论 #3 问题二：commitSyncProfile 按当前活动作品路由 —
    // 无活动作品时直接失败，不写入任何作品。
    val projectId = com.xiwei.sujian.data.ActiveProjectGate.currentProjectId()
    if (projectId == null) {
        failures.add(SaveFailure(SaveField.SYNC_CONFIG, syncConfig?.revision ?: 0L))
        return false to false
    }
    val draftConfig = syncConfig?.config ?: _uiState.value.syncConfig
    val draftSecrets = syncSecrets?.secrets ?: _uiState.value.syncSecrets
    val commitResult =
        withContext(Dispatchers.IO) {
            repo.commitSyncProfile(projectId, draftConfig, draftSecrets)
        }
    var syncConfigSaved = false
    var syncSecretsSaved = false
    when (commitResult) {
        is SettingsSaveResult.Success -> {
            if (syncConfig != null && syncConfigRevision == syncConfig.revision) {
                syncConfigPersistedRevision = syncConfig.revision
                syncConfigSaved = true
            }
            if (syncSecrets != null && syncSecretsRevision == syncSecrets.revision) {
                syncSecretsPersistedRevision = syncSecrets.revision
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
    syncConfig: SettingsSaveCommand.SyncConfig?,
    syncSecrets: SettingsSaveCommand.SyncSecrets?,
    failures: MutableList<SaveFailure>,
) {
    for (failure in commitFailures) {
        when (failure.field) {
            SaveField.SYNC_CONFIG -> {
                if (syncConfig != null && syncConfigRevision == syncConfig.revision) {
                    failures.add(failure)
                }
            }
            SaveField.SYNC_SECRETS -> {
                if (syncSecrets != null && syncSecretsRevision == syncSecrets.revision) {
                    failures.add(failure)
                }
            }
            else -> { }
        }
    }
}

/** 处理保存结果：失败回滚+报错 或 成功清错 — 从 executeSave 拆分。 */
private suspend fun SettingsViewModel.handleSaveOutcome(
    repo: SettingsRepository,
    failures: List<SaveFailure>,
    local: SettingsSaveCommand.Local?,
    fontSize: SettingsSaveCommand.FontSize?,
    syncConfig: SettingsSaveCommand.SyncConfig?,
    syncSecrets: SettingsSaveCommand.SyncSecrets?,
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
    } else if (hasAnySaveCommand(local, fontSize, syncConfig, syncSecrets)) {
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
        SaveField.SYNC_CONFIG -> rollbackSyncConfig(repo, failure.revision)
        SaveField.SYNC_SECRETS -> rollbackSyncSecrets(repo, failure.revision)
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
    repo: SettingsRepository,
    expectedRevision: Long,
) {
    if (syncConfigRevision != expectedRevision) return
    // #595 八/五：回滚读取活动 generation 的完整 snapshot，不再读 live 槽；
    // 类型化处理 — Failed 保留当前 UI 值（不静默退化为默认值/null）。
    // #600 评论 #3 问题二：按当前活动作品路由。
    val rollbackProjectId = com.xiwei.sujian.data.ActiveProjectGate.currentProjectId()
    val profile =
        if (rollbackProjectId != null) {
            withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile(rollbackProjectId) }
        } else {
            com.xiwei.sujian.data.SyncProfileReadResult.Failed(
                com.xiwei.sujian.data.SyncFailureKind.Fatal,
                MSG_NO_ACTIVE_PROJECT,
            )
        }
    if (syncConfigRevision != expectedRevision) return
    syncConfigRevision = syncConfigPersistedRevision
    _uiState.update { it.copy(syncConfig = profile.toSyncProfileLoadState().confirmedConfig ?: it.syncConfig) }
}

private suspend fun SettingsViewModel.rollbackSyncSecrets(
    repo: SettingsRepository,
    expectedRevision: Long,
) {
    if (syncSecretsRevision != expectedRevision) return
    // #600 评论 #3 问题二：按当前活动作品路由。
    val rollbackSecretsProjectId = com.xiwei.sujian.data.ActiveProjectGate.currentProjectId()
    val profile =
        if (rollbackSecretsProjectId != null) {
            withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile(rollbackSecretsProjectId) }
        } else {
            com.xiwei.sujian.data.SyncProfileReadResult.Failed(
                com.xiwei.sujian.data.SyncFailureKind.Fatal,
                MSG_NO_ACTIVE_PROJECT,
            )
        }
    if (syncSecretsRevision != expectedRevision) return
    syncSecretsRevision = syncSecretsPersistedRevision
    _uiState.update {
        it.copy(
            syncSecrets = profile.toSyncProfileLoadState().confirmedSecrets ?: it.syncSecrets,
        )
    }
}

// #597 刷新合并需一次性读取全部字段并按未保存状态合并，与 loadInitial 结构对称。
fun SettingsViewModel.mergeRefresh() {
    val repo = settingsRepo
    editorScope.launch {
        val current = _uiState.value
        val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
        val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
        // #595 八：刷新读取活动 generation 的完整 snapshot，不再读 live legacy 槽。
        // #600 评论 #3 问题二：按当前活动作品路由。
        val mergeProjectId = com.xiwei.sujian.data.ActiveProjectGate.currentProjectId()
        val committedProfile =
            if (mergeProjectId != null) {
                withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile(mergeProjectId) }
            } else {
                com.xiwei.sujian.data.SyncProfileReadResult.Failed(
                    com.xiwei.sujian.data.SyncFailureKind.Fatal,
                    MSG_NO_ACTIVE_PROJECT,
                )
            }
        // #595 四：类型化加载状态 — Failed 时保留字段值，页面显示真实错误。
        val syncProfileLoadState = committedProfile.toSyncProfileLoadState()
        val syncCapability =
            if (mergeProjectId != null) {
                withContext(Dispatchers.IO) { repo.getSyncCapability(mergeProjectId) }
            } else {
                com.xiwei.sujian.model.SyncCapabilityData()
            }
        val secureStorageWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() }
        val builtinThemes = withContext(Dispatchers.IO) { repo.listBuiltinThemes() }
        val paletteRecords = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
        val aiAvailable = withContext(Dispatchers.IO) { repo.aiAvailable() }
        val dataRootPath = withContext(Dispatchers.IO) { repo.dataRootDir() }
        _uiState.update {
            SettingsUiState(
                settings = mergeLoadedLocal(current.settings, settings),
                fontSize = mergeLoadedFontSize(current.fontSize, fontSize),
                syncConfig = mergeLoadedSyncConfig(current.syncConfig, syncProfileLoadState),
                syncSecrets = mergeLoadedSyncSecrets(current.syncSecrets, syncProfileLoadState),
                syncCapability = syncCapability,
                secureStorageWarning = secureStorageWarning,
                builtinThemes = builtinThemes,
                paletteRecords = paletteRecords,
                aiAvailable = aiAvailable,
                dataRootPath = dataRootPath,
                dryRunState = current.dryRunState,
                testConnectionState = current.testConnectionState,
                performSyncState = current.performSyncState,
                structuredSyncResult = current.structuredSyncResult,
                lastCommandType = current.lastCommandType,
                syncProfileLoadState = syncProfileLoadState,
            )
        }
    }
}

// #597：未保存字段合并 — 刷新加载值只在用户没有未保存编辑时覆盖当前值。
private fun SettingsViewModel.mergeLoadedLocal(
    current: LocalSettings,
    loaded: LocalSettings,
): LocalSettings = if (!hasUnsavedLocal()) loaded else current

private fun SettingsViewModel.mergeLoadedFontSize(
    current: Float,
    loaded: Float,
): Float = if (!hasUnsavedFontSize()) loaded else current

private fun SettingsViewModel.mergeLoadedSyncConfig(
    current: com.xiwei.sujian.model.SyncConfig,
    loadState: SyncProfileLoadState,
): com.xiwei.sujian.model.SyncConfig = if (!hasUnsavedSyncConfig()) loadState.confirmedConfig ?: current else current

private fun SettingsViewModel.mergeLoadedSyncSecrets(
    current: com.xiwei.sujian.model.SyncSecrets,
    loadState: SyncProfileLoadState,
): com.xiwei.sujian.model.SyncSecrets = if (!hasUnsavedSyncSecrets()) loadState.confirmedSecrets ?: current else current
