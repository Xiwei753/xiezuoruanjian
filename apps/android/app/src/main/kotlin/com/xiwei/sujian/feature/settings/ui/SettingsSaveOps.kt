package com.xiwei.sujian.feature.settings.ui

// ! # 设置保存操作（从 SettingsRoute 拆分）
//
// #630 评论 #1+#2：同步配置只有一份 — 全量同步覆盖设置/星图/主题/全部作品。

import com.xiwei.sujian.R
import com.xiwei.sujian.feature.settings.data.CoreSettingsEvents
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
 * #600 评论 #7 / #618 三: 外部同步拉取设置/主题后, 重新从 Core 加载设置状态.
 * 只监听 externalSettingsChanged（外部同步拉取）触发, 复用 loadInitial 的字段集,
 * 不新建第二套事件系统。本机保存不再回环触发本函数。
 *
 * #618 三 复审：外部重载不能覆盖用户尚未保存的编辑。先快照窗口期前的
 * 未保存/已保存 revision，读回 Core 后再校验：窗口期内本地没有任何编辑、
 * 没有任何保存完成，且当前没有 pending 编辑时，才应用外部同步值；否则保留
 * UI 草稿（pending 编辑 flush 后用户值写回 Core，用户编辑胜出；窗口期内已
 * flush 的保存同样代表用户值，草稿与 Core 一致），UI 与 Core 始终一致。
 */
suspend fun SettingsViewModel.reloadFromExternalSync() {
    val repo = settingsRepo
    val localRevSnapshot = localRevision
    val localPersistedSnapshot = localPersistedRevision
    val fontSizeRevSnapshot = fontSizeRevision
    val fontSizePersistedSnapshot = fontSizePersistedRevision
    val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
    val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
    val paletteRecords = withContext(Dispatchers.IO) { themeRepo.listPaletteRecords() }
    _uiState.update {
        val localStable =
            localRevSnapshot == localRevision &&
                localPersistedSnapshot == localPersistedRevision &&
                localRevision == localPersistedRevision
        val fontSizeStable =
            fontSizeRevSnapshot == fontSizeRevision &&
                fontSizePersistedSnapshot == fontSizePersistedRevision &&
                fontSizeRevision == fontSizePersistedRevision
        it.copy(
            settings = if (localStable) settings else it.settings,
            fontSize = if (fontSizeStable) fontSize else it.fontSize,
            paletteRecords = paletteRecords,
        )
    }
}

/**
 * 加载初始快照 — 从 loadInitial 拆分以降低认知复杂度。
 * 读取活动 generation 的完整 snapshot（#595 八），不再读 live legacy 槽。
 * #630 评论 #1+#2：同步 profile 只有一份，不再区分作品级/应用级。
 */
private suspend fun SettingsViewModel.loadInitialSnapshot(repo: SettingsRepository) {
    val snapshotRevisions =
        InitialSnapshotRevisions(
            local = localRevision,
            fontSize = fontSizeRevision,
            syncConfig = syncConfigRevision,
            syncSecrets = syncSecretsRevision,
        )
    val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
    val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
    val (syncProfileLoadState, syncCapability) = loadInitialSyncProfile(syncRepo)
    val loaded =
        InitialLoadedValues(
            settings = settings,
            fontSize = fontSize,
            syncProfileLoadState = syncProfileLoadState,
            syncCapability = syncCapability,
            secureStorageWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() },
            builtinThemes = withContext(Dispatchers.IO) { themeRepo.listBuiltinThemes() },
            paletteRecords = withContext(Dispatchers.IO) { themeRepo.listPaletteRecords() },
            aiAvailable = withContext(Dispatchers.IO) { repo.aiAvailable() },
            // #649 评论 5559763924：数据根目录路径由 Repository 持有的 appContext 解析。
            dataRootPath = withContext(Dispatchers.IO) { repo.dataRootPath() },
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
        syncConfig =
            if (syncConfigRevision == snapshotRevisions.syncConfig) {
                loaded.syncProfileLoadState.confirmedConfig ?: current.syncConfig
            } else {
                current.syncConfig
            },
        syncSecrets =
            if (syncSecretsRevision == snapshotRevisions.syncSecrets) {
                loaded.syncProfileLoadState.confirmedSecrets ?: current.syncSecrets
            } else {
                current.syncSecrets
            },
        syncCapability = loaded.syncCapability,
        syncProfileLoadState = loaded.syncProfileLoadState,
        // #629 根因C：loadInitialSnapshot 在 init 里异步启动，与 saveChannel 消费协程并发。
        // 旧实现创建全新 SettingsUiState，performSyncState 等事务字段默认 IDLE，
        // 会覆盖 saveChannel 消费协程已设的 RUNNING/SUCCESS/FAILURE。当
        // SyncSaveAndRunTransactionTest 连发 UpdateSyncConfig→PerformSync→UpdateSyncConfig
        // （channel 多一项 Save 使消费协程执行更久）时，loadInitialSnapshot 更易在
        // SaveAndRunSync 事务设 FAILURE 后才完成，把 state 回退为 IDLE，awaitTerminalState
        // 20s 超时。初始加载只重载设置值/sync profile/capability/themes，不得重置已在进行
        // 或已结束的同步事务状态与错误状态。
        // #630 评论 #1+#2：同步只有一份（全量），事务状态字段相应合并为单套。
        dryRunState = current.dryRunState,
        testConnectionState = current.testConnectionState,
        performSyncState = current.performSyncState,
        syncResult = current.syncResult,
        secureStorageWarning = loaded.secureStorageWarning,
        builtinThemes = loaded.builtinThemes,
        paletteRecords = loaded.paletteRecords,
        aiAvailable = loaded.aiAvailable,
        dataRootPath = loaded.dataRootPath,
        versionInfo = current.versionInfo,
        saveErrorResId = current.saveErrorResId,
        lastCommandType = current.lastCommandType,
    )

/** 初始加载读取的值 — 打包避免 buildInitialUiState LongParameterList。 */
private data class InitialLoadedValues(
    val settings: LocalSettings,
    val fontSize: Float,
    val syncProfileLoadState: SyncProfileLoadState,
    val syncCapability: com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData,
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
    val syncConfig: Long,
    val syncSecrets: Long,
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
    val (configSaved, secretsSaved) =
        saveSyncProfileField(syncRepo, commands.syncConfig, commands.syncSecrets, failures)

    if (configSaved || secretsSaved) {
        // #595 五：成功提交后一次性更新 config/secrets/loadState/capability/warning。
        refreshSyncProfileState()
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
            // #617 评论三：只有真正影响主题的本地保存才重建主题 — 自动保存、诊断、
            // 编辑器选项、沉浸式全屏等普通设置保存不再触发 ThemeStore.reload()。
            if (local.affectsTheme) {
                com.xiwei.sujian.app.theme.ThemeStore.reload()
            }
            // #630 评论二：只有真正影响正文运行时的本地保存才通知编辑器重读设置 —
            // 自动保存、AI、诊断、沉浸式全屏、主题颜色等保存不再触发编辑器重载。
            if (local.affectsEditor) {
                CoreSettingsEvents.notifyLocalEditorSettingsChanged()
            }
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
 * #630 评论 #1+#2：commitSyncProfile 不带 projectId — 全量同步覆盖全部作品。
 * 返回 (configSaved, secretsSaved)。
 */
private suspend fun SettingsViewModel.saveSyncProfileField(
    repo: SyncRepository,
    syncConfig: SettingsSaveCommand.SyncConfig?,
    syncSecrets: SettingsSaveCommand.SyncSecrets?,
    failures: MutableList<SaveFailure>,
): Pair<Boolean, Boolean> {
    if (syncConfig == null && syncSecrets == null) return false to false
    val draftConfig = syncConfig?.config ?: _uiState.value.syncConfig
    val draftSecrets = syncSecrets?.secrets ?: _uiState.value.syncSecrets
    val commitResult =
        withContext(Dispatchers.IO) {
            repo.commitSyncProfile(draftConfig, draftSecrets)
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
    if (syncConfigRevision != expectedRevision) return
    // #595 八/五：回滚读取活动 generation 的完整 snapshot，不再读 live 槽；
    // 类型化处理 — Failed 保留当前 UI 值（不静默退化为默认值/null）。
    val profile = withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile() }
    if (syncConfigRevision != expectedRevision) return
    syncConfigRevision = syncConfigPersistedRevision
    _uiState.update {
        it.copy(syncConfig = profile.toSyncProfileLoadState().confirmedConfig ?: it.syncConfig)
    }
}

private suspend fun SettingsViewModel.rollbackSyncSecrets(
    repo: SyncRepository,
    expectedRevision: Long,
) {
    if (syncSecretsRevision != expectedRevision) return
    val profile = withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile() }
    if (syncSecretsRevision != expectedRevision) return
    syncSecretsRevision = syncSecretsPersistedRevision
    _uiState.update {
        it.copy(
            syncSecrets = profile.toSyncProfileLoadState().confirmedSecrets ?: it.syncSecrets,
        )
    }
}
