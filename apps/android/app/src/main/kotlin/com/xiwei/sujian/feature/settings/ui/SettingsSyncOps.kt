package com.xiwei.sujian.feature.settings.ui

import com.xiwei.sujian.app.state.ActiveDocumentGate
import com.xiwei.sujian.feature.settings.data.SettingsSaveResult
import com.xiwei.sujian.feature.sync.data.ExclusiveResult
import com.xiwei.sujian.feature.sync.data.SyncSession
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.withContext

// ! # 设置同步事务操作（从 SettingsRoute 拆分）
//
// #630 评论 #1+#2：全量同步只有一份事务 — 覆盖设置/星图/主题/全部作品，
// 不再区分作品级与应用级两套事务命令/状态字段。

// #597 同步事务协议字符串 — 提取为常量避免 StringLiteralDuplication
// SYNC_STATUS_ERROR 已移到 SettingsSyncResultMappers.kt
private const val MSG_SAVE_CONFIG_OR_SECRETS_FAILED = "save_config_or_secrets_failed"
private const val MSG_UNEXPECTED_ERROR = "unexpected_error"

suspend fun SettingsViewModel.executeTransaction(command: SettingsTransactionCommand) {
    when (command) {
        is SettingsTransactionCommand.SaveAndRunSync -> executeSyncTransaction(command)
        is SettingsTransactionCommand.SaveAndRunDryRun -> executeDryRunTransaction(command)
        is SettingsTransactionCommand.SaveAndRunDiagnostics -> executeDiagnosticsTransaction(command)
    }
}

/**
 * 事务保存配置与凭据 — 提交到唯一同步 profile（不带 projectId）。
 * 返回 false 表示提交失败，调用方应终止事务并显示错误。
 */
suspend fun SettingsViewModel.saveTransactionConfigAndSecrets(
    config: SyncConfig,
    configRevision: Long,
    secrets: SyncSecrets,
    secretsRevision: Long,
): Boolean {
    val commitResult = withContext(Dispatchers.IO) { syncRepo.commitSyncProfile(config, secrets) }
    if (commitResult is SettingsSaveResult.Failed) return false
    if (syncConfigRevision == configRevision) {
        syncConfigPersistedRevision = configRevision
    }
    if (syncSecretsRevision == secretsRevision) {
        syncSecretsPersistedRevision = secretsRevision
    }
    return true
}

// #597：三个同步事务共用同一骨架（保存配置→执行→更新状态→刷新 profile），
// 差异只有状态字段与 IO 操作本身；骨架收敛到 runCommandTransaction，
// 各事务只保留自己的状态字段 lambda 与结果映射。

private suspend fun SettingsViewModel.runCommandTransaction(
    command: SettingsTransactionCommand,
    lastCommandType: SyncCommandType,
    setState: (SyncCommandState, StructuredSyncResult?) -> Unit,
    applyResult: (SyncCommandIoResult) -> Unit,
    operation: suspend (SyncConfig, SyncSecrets) -> SyncCommandIoResult,
) {
    setState(SyncCommandState.RUNNING, null)
    try {
        val saveOk =
            saveTransactionConfigAndSecrets(
                command.config,
                command.configRevision,
                command.secrets,
                command.secretsRevision,
            )
        if (!saveOk) {
            setState(
                SyncCommandState.FAILURE,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_SAVE_CONFIG_OR_SECRETS_FAILED),
            )
            return
        }
        applyResult(operation(command.config, command.secrets))
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        setState(
            SyncCommandState.FAILURE,
            StructuredSyncResult(
                statusCode = SYNC_STATUS_ERROR,
                messageKey = MSG_UNEXPECTED_ERROR,
                sanitizedDiagnostic = e.message,
            ),
        )
    }
    try {
        // #595 五：事务结束后一次性刷新完整同步 profile 状态。
        refreshSyncProfileState()
    } catch (_: Exception) {
    }
}

/**
 * #595 三/十：在排他锁内执行同步 IO 操作 — 启动前先 flush 活动正文，
 * 锁内设置操作作用域凭据（失败立即终止），结束后清除；不得使用上次
 * 正式同步留下的旧 token。
 */
private suspend fun SettingsViewModel.runExclusiveSyncIo(
    config: SyncConfig,
    secrets: SyncSecrets,
    perform: suspend (SyncConfig) -> SyncCommandIoResult,
): ExclusiveResult<SyncCommandIoResult> =
    SyncSession.runExclusive { _ ->
        val flushOk = ActiveDocumentGate.flushActiveDocument()
        if (!flushOk) {
            return@runExclusive SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_document_save_failed"),
            )
        }
        withContext(Dispatchers.IO) {
            val overrideOk = syncRepo.setSyncSecretsOverrideStrict(secrets)
            if (!overrideOk) {
                return@withContext SyncCommandIoResult(
                    true,
                    true,
                    StructuredSyncResult(
                        statusCode = SYNC_STATUS_ERROR,
                        messageKey = "sync_credentials_override_failed",
                    ),
                )
            }
            try {
                perform(config)
            } finally {
                syncRepo.clearSyncSecretsOverride()
            }
        }
    }

suspend fun SettingsViewModel.executeSyncTransaction(command: SettingsTransactionCommand.SaveAndRunSync) {
    runCommandTransaction(
        command = command,
        lastCommandType = SyncCommandType.PERFORM_SYNC,
        setState = { state, result ->
            _uiState.update {
                it.copy(
                    performSyncState = state,
                    syncResult = result,
                    lastCommandType = SyncCommandType.PERFORM_SYNC,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        performSyncState = SyncCommandState.SUCCESS,
                        syncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.PERFORM_SYNC,
                    )
                } else {
                    current.copy(
                        performSyncState = SyncCommandState.FAILURE,
                        syncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.PERFORM_SYNC,
                    )
                }
            }
        },
    ) { _, _ ->
        // #630 评论 #1：全量同步 — 设置页同步覆盖设置/星图/主题/全部作品。
        syncCoordinator.runFullSync(command.trigger).toIoResult()
    }
}

suspend fun SettingsViewModel.executeDryRunTransaction(command: SettingsTransactionCommand.SaveAndRunDryRun) {
    runCommandTransaction(
        command = command,
        lastCommandType = SyncCommandType.DRY_RUN,
        setState = { state, result ->
            _uiState.update {
                it.copy(
                    dryRunState = state,
                    syncResult = result,
                    lastCommandType = SyncCommandType.DRY_RUN,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        dryRunState = SyncCommandState.SUCCESS,
                        syncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.DRY_RUN,
                    )
                } else {
                    current.copy(
                        dryRunState = SyncCommandState.FAILURE,
                        syncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.DRY_RUN,
                    )
                }
            }
        },
    ) { config, secrets ->
        runExclusiveSyncIo(config, secrets) {
            syncRepo.performFullSyncDryRunTyped(it).toIoResult()
        }.toIoResult()
    }
}

suspend fun SettingsViewModel.executeDiagnosticsTransaction(
    command: SettingsTransactionCommand.SaveAndRunDiagnostics,
) {
    runCommandTransaction(
        command = command,
        lastCommandType = SyncCommandType.TEST_CONNECTION,
        setState = { state, result ->
            _uiState.update {
                it.copy(
                    testConnectionState = state,
                    syncResult = result,
                    lastCommandType = SyncCommandType.TEST_CONNECTION,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        testConnectionState = SyncCommandState.SUCCESS,
                        syncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.TEST_CONNECTION,
                    )
                } else {
                    current.copy(
                        testConnectionState = SyncCommandState.FAILURE,
                        syncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.TEST_CONNECTION,
                    )
                }
            }
        },
    ) { config, secrets ->
        runExclusiveSyncIo(config, secrets) {
            syncRepo.performFullSyncDiagnosticsTyped(it).toIoResult()
        }.toIoResult()
    }
    try {
        val refreshedCapability = withContext(Dispatchers.IO) { syncRepo.getSyncCapability() }
        val refreshedWarning = withContext(Dispatchers.IO) { settingsRepo.getSecureStorageWarning() }
        _uiState.update {
            it.copy(
                syncCapability = refreshedCapability,
                secureStorageWarning = refreshedWarning,
            )
        }
    } catch (_: Exception) {
    }
}

/**
 * #595 五：刷新同步 profile 状态 — 读取 committed profile，
 * 一次性更新 syncConfig/syncSecrets/syncProfileLoadState/syncCapability/secureStorageWarning。
 * #630 评论 #1+#2：profile 只有一份，不再按 projectId 路由。
 */
suspend fun SettingsViewModel.refreshSyncProfileState() {
    val repo = syncRepo
    val settingsRepoLocal = settingsRepo
    val committedProfile = withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile() }
    val refreshedCapability = withContext(Dispatchers.IO) { repo.getSyncCapability() }
    val refreshedWarning = withContext(Dispatchers.IO) { settingsRepoLocal.getSecureStorageWarning() }
    val loadState = committedProfile.toSyncProfileLoadState()
    _uiState.update {
        it.copy(
            syncConfig =
                if (!hasUnsavedSyncConfig()) {
                    loadState.confirmedConfig ?: it.syncConfig
                } else {
                    it.syncConfig
                },
            syncSecrets =
                if (!hasUnsavedSyncSecrets()) {
                    loadState.confirmedSecrets ?: it.syncSecrets
                } else {
                    it.syncSecrets
                },
            syncProfileLoadState = loadState,
            syncCapability = refreshedCapability,
            secureStorageWarning = refreshedWarning,
        )
    }
}
