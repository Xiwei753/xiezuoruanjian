package com.xiwei.sujian.ui.compose.settings

// ! # 设置同步事务操作（从 SettingsRoute 拆分）

import com.xiwei.sujian.data.ActiveDocumentGate
import com.xiwei.sujian.data.ExclusiveResult
import com.xiwei.sujian.data.SettingsSaveResult
import com.xiwei.sujian.data.SyncDiagnosticsOutcome
import com.xiwei.sujian.data.SyncDryRunOutcome
import com.xiwei.sujian.data.SyncFailureKind
import com.xiwei.sujian.data.SyncOutcome
import com.xiwei.sujian.data.SyncSession
import com.xiwei.sujian.model.SyncConfig
import com.xiwei.sujian.model.SyncSecrets
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.withContext

// #597 同步事务协议字符串 — 提取为常量避免 StringLiteralDuplication
private const val SYNC_STATUS_ERROR = "error"
private const val MSG_SAVE_CONFIG_OR_SECRETS_FAILED = "save_config_or_secrets_failed"
private const val MSG_UNEXPECTED_ERROR = "unexpected_error"

suspend fun SettingsViewModel.executeTransaction(command: SettingsTransactionCommand) {
    when (command) {
        is SettingsTransactionCommand.SaveAndRunSync -> executeSyncTransaction(command)
        is SettingsTransactionCommand.SaveAndRunDryRun -> executeDryRunTransaction(command)
        is SettingsTransactionCommand.SaveAndRunDiagnostics -> executeDiagnosticsTransaction(command)
    }
}

suspend fun SettingsViewModel.saveTransactionConfigAndSecrets(
    config: SyncConfig,
    configRevision: Long,
    secrets: SyncSecrets,
    secretsRevision: Long,
): Boolean {
    val commitResult = withContext(Dispatchers.IO) { settingsRepo.commitSyncProfile(config, secrets) }
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
            val capability = settingsRepo.getSyncCapability()
            if (!capability.canRun) {
                return@withContext SyncCommandIoResult(
                    true,
                    true,
                    StructuredSyncResult(
                        statusCode = "blocked",
                        messageKey = capability.blockMessageKey ?: "sync_not_ready",
                    ),
                )
            }
            val overrideOk = settingsRepo.setSyncSecretsOverrideStrict(secrets)
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
                settingsRepo.clearSyncSecretsOverride()
            }
        }
    }

/** 把 runExclusive 的忙/成功结果统一成调用方可直接消费的 [SyncCommandIoResult]。 */

private fun ExclusiveResult<SyncCommandIoResult>.toIoResult(): SyncCommandIoResult =
    when (this) {
        is ExclusiveResult.Busy ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = "busy", messageKey = "sync_already_running"),
            )
        is ExclusiveResult.Success -> value
    }

suspend fun SettingsViewModel.executeSyncTransaction(command: SettingsTransactionCommand.SaveAndRunSync) {
    runCommandTransaction(
        command = command,
        lastCommandType = SyncCommandType.PERFORM_SYNC,
        setState = { state, result ->
            _uiState.update {
                it.copy(
                    performSyncState = state,
                    structuredSyncResult = result,
                    lastCommandType = SyncCommandType.PERFORM_SYNC,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        performSyncState = SyncCommandState.SUCCESS,
                        structuredSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.PERFORM_SYNC,
                    )
                } else {
                    current.copy(
                        performSyncState = SyncCommandState.FAILURE,
                        structuredSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.PERFORM_SYNC,
                    )
                }
            }
        },
    ) { _, _ ->
        // #600：sync 已改为 per-project — 设置页同步针对当前活动作品。
        val projectId = com.xiwei.sujian.data.ActiveProjectGate.currentProjectId()
        if (projectId != null) {
            syncCoordinator.runSync(command.trigger, projectId).toIoResult()
        } else {
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_no_active_project"),
            )
        }
    }
}

suspend fun SettingsViewModel.executeDryRunTransaction(command: SettingsTransactionCommand.SaveAndRunDryRun) {
    runCommandTransaction(
        command = command,
        lastCommandType = SyncCommandType.DRY_RUN,
        setState = { state, result ->
            _uiState.update {
                it.copy(dryRunState = state, structuredSyncResult = result, lastCommandType = SyncCommandType.DRY_RUN)
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        dryRunState = SyncCommandState.SUCCESS,
                        structuredSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.DRY_RUN,
                    )
                } else {
                    current.copy(
                        dryRunState = SyncCommandState.FAILURE,
                        structuredSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.DRY_RUN,
                    )
                }
            }
        },
    ) { config, secrets ->
        // #600：sync 已改为 per-project — 试运行针对当前活动作品。
        val projectId = com.xiwei.sujian.data.ActiveProjectGate.currentProjectId()
        if (projectId != null) {
            runExclusiveSyncIo(config, secrets) {
                settingsRepo.performSyncDryRunTyped(projectId, it).toIoResult()
            }.toIoResult()
        } else {
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_no_active_project"),
            )
        }
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
                    structuredSyncResult = result,
                    lastCommandType = SyncCommandType.TEST_CONNECTION,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        testConnectionState = SyncCommandState.SUCCESS,
                        structuredSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.TEST_CONNECTION,
                    )
                } else {
                    current.copy(
                        testConnectionState = SyncCommandState.FAILURE,
                        structuredSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.TEST_CONNECTION,
                    )
                }
            }
        },
    ) { config, secrets ->
        runExclusiveSyncIo(config, secrets) { settingsRepo.performSyncDiagnosticsTyped(it).toIoResult() }.toIoResult()
    }
    try {
        val refreshedCapability = withContext(Dispatchers.IO) { settingsRepo.getSyncCapability() }
        val refreshedWarning = withContext(Dispatchers.IO) { settingsRepo.getSecureStorageWarning() }
        _uiState.update { it.copy(syncCapability = refreshedCapability, secureStorageWarning = refreshedWarning) }
    } catch (_: Exception) {
    }
}

suspend fun SettingsViewModel.refreshSyncProfileState() {
    val repo = settingsRepo
    val current = _uiState.value
    val committedProfile = withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile() }
    val refreshedCapability = withContext(Dispatchers.IO) { repo.getSyncCapability() }
    val refreshedWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() }
    _uiState.update {
        it.copy(
            syncConfig =
                if (!hasUnsavedSyncConfig()) {
                    committedProfile.toSyncProfileLoadState().confirmedConfig ?: it.syncConfig
                } else {
                    it.syncConfig
                },
            syncSecrets =
                if (!hasUnsavedSyncSecrets()) {
                    committedProfile.toSyncProfileLoadState().confirmedSecrets ?: it.syncSecrets
                } else {
                    it.syncSecrets
                },
            syncProfileLoadState = committedProfile.toSyncProfileLoadState(),
            syncCapability = refreshedCapability,
            secureStorageWarning = refreshedWarning,
        )
    }
}

// ── #597：结果映射 — 各同步结果类型 → SyncCommandIoResult（isSuccess 由 statusCode 决定）──

private fun SyncOutcome.Completed.completedToIoResult(): SyncCommandIoResult {
    val sr = result
    return SyncCommandIoResult(
        true,
        true,
        StructuredSyncResult(
            statusCode = if (sr.error == null) "ok" else "error",
            messageKey = "sync_perform_result",
            counts =
                SyncCounts(
                    uploaded = sr.uploadedFiles.size,
                    downloaded = sr.downloadedFiles.size,
                    deletedRemote = sr.remoteDeletes.size,
                    deletedLocal = sr.localDeletes.size,
                    conflicts = sr.conflicts.size,
                    overwritten = sr.overwrittenFiles.size,
                    ignored = sr.ignoredFiles.size,
                ),
            sanitizedDiagnostic = if (sr.error != null) "sync_failed" else null,
        ),
    )
}

private fun SyncOutcome.toIoResult(): SyncCommandIoResult =
    when (this) {
        is SyncOutcome.Completed -> completedToIoResult()
        is SyncOutcome.Unconfigured ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_unconfigured"),
            )
        is SyncOutcome.Disabled ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_disabled"),
            )
        is SyncOutcome.Busy ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_busy"),
            )
        is SyncOutcome.RetryableFailure ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = kind.messageKey()),
            )
        is SyncOutcome.TerminalFailure ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = kind.messageKey()),
            )
        else ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_unknown"),
            )
    }

private fun SyncDryRunOutcome.toIoResult(): SyncCommandIoResult =
    when (this) {
        is SyncDryRunOutcome.Success -> {
            val plan = plan
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = "ok",
                    messageKey = "sync_dry_run_result",
                    counts =
                        SyncCounts(
                            uploaded = plan.filesToUpload.size,
                            downloaded = plan.filesToDownload.size,
                            deletedRemote = plan.filesToDeleteRemote.size,
                            deletedLocal = plan.filesToDeleteLocal.size,
                            conflicts = plan.conflicts.size,
                            ignored = plan.ignoredFiles.size,
                        ),
                ),
            )
        }
        is SyncDryRunOutcome.Error ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = syncFailureKind.messageKey(),
                    sanitizedDiagnostic = message,
                ),
            )
        SyncDryRunOutcome.NotLoaded ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = SyncFailureKind.NativeUnavailable.messageKey(),
                    sanitizedDiagnostic = SyncFailureKind.NativeUnavailable.name,
                ),
            )
    }

private fun SyncDiagnosticsOutcome.Success.diagnosticsSuccessToIoResult(): SyncCommandIoResult {
    val diag = result
    return SyncCommandIoResult(
        true,
        true,
        StructuredSyncResult(
            statusCode = if (diag.success) "ok" else "fail",
            messageKey = "sync_test_connection_result",
            messageArgs =
                mapOf(
                    "network" to if (diag.networkOk) "ok" else "fail",
                    "auth" to if (diag.authOk) "ok" else "fail",
                    "repo" to if (diag.repoOk) "ok" else "fail",
                    "branch" to if (diag.branchOk) "ok" else "fail",
                ),
            sanitizedDiagnostic = if (!diag.success) "connection_failed" else null,
        ),
    )
}

private fun SyncDiagnosticsOutcome.toIoResult(): SyncCommandIoResult =
    when (this) {
        is SyncDiagnosticsOutcome.Success -> diagnosticsSuccessToIoResult()
        is SyncDiagnosticsOutcome.Error ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = syncFailureKind.messageKey(),
                    sanitizedDiagnostic = message,
                ),
            )
        SyncDiagnosticsOutcome.NotLoaded ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = SyncFailureKind.NativeUnavailable.messageKey(),
                    sanitizedDiagnostic = SyncFailureKind.NativeUnavailable.name,
                ),
            )
    }
