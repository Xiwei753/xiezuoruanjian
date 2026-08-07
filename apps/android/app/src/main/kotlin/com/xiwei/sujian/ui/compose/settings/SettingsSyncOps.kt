package com.xiwei.sujian.ui.compose.settings

//! # 设置同步事务操作（从 SettingsRoute 拆分）

import com.xiwei.sujian.data.ExclusiveResult
import com.xiwei.sujian.data.SyncCoordinator
import com.xiwei.sujian.data.SyncFailureKind
import com.xiwei.sujian.data.SyncOutcome
import com.xiwei.sujian.data.SyncDryRunOutcome
import com.xiwei.sujian.data.SyncDiagnosticsOutcome
import com.xiwei.sujian.data.SyncSession
import com.xiwei.sujian.data.WorkspaceDocumentGate
import com.xiwei.sujian.data.SettingsSaveResult
import com.xiwei.sujian.model.SyncTrigger
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
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
    config: com.xiwei.sujian.model.SyncConfig,
    configRevision: Long,
    secrets: com.xiwei.sujian.model.SyncSecrets,
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

// #597 同步事务需原子执行（保存配置→运行同步→更新状态），拆分会破坏事务一致性 — 待后续重构
@Suppress("CyclomaticComplexMethod")
suspend fun SettingsViewModel.executeSyncTransaction(
    command: SettingsTransactionCommand.SaveAndRunSync,
) {
    _uiState.update { it.copy(performSyncState = SyncCommandState.RUNNING, structuredSyncResult = null, lastCommandType = SyncCommandType.PERFORM_SYNC) }
    try {
        if (!saveTransactionConfigAndSecrets(command.config, command.configRevision, command.secrets, command.secretsRevision)) {
            _uiState.update { it.copy(performSyncState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_SAVE_CONFIG_OR_SECRETS_FAILED)) }
            return
        }
        val syncOutcome = syncCoordinator.runSync(command.trigger)
        val ioResult = when (syncOutcome) {
            is SyncOutcome.Completed -> {
                val sr = syncOutcome.result
                val counts = SyncCounts(
                    uploaded = sr.uploadedFiles.size,
                    downloaded = sr.downloadedFiles.size,
                    deletedRemote = sr.remoteDeletes.size,
                    deletedLocal = sr.localDeletes.size,
                    conflicts = sr.conflicts.size,
                    overwritten = sr.overwrittenFiles.size,
                    ignored = sr.ignoredFiles.size
                )
                SyncCommandIoResult(true, true, StructuredSyncResult(
                    statusCode = if (sr.error == null) "ok" else "error",
                    messageKey = "sync_perform_result",
                    counts = counts,
                    sanitizedDiagnostic = if (sr.error != null) "sync_failed" else null
                ))
            }
            is SyncOutcome.Unconfigured ->
                SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_unconfigured"))
            is SyncOutcome.Disabled ->
                SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_disabled"))
            is SyncOutcome.Busy ->
                SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_busy"))
            is SyncOutcome.RetryableFailure ->
                SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = syncOutcome.kind.messageKey()))
            is SyncOutcome.TerminalFailure ->
                SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = syncOutcome.kind.messageKey()))
            else ->
                SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_unknown"))
        }
        _uiState.update { current ->
            if (ioResult.isSuccess) {
                current.copy(performSyncState = SyncCommandState.SUCCESS, structuredSyncResult = ioResult.structuredResult, lastCommandType = SyncCommandType.PERFORM_SYNC)
            } else {
                current.copy(performSyncState = SyncCommandState.FAILURE, structuredSyncResult = ioResult.structuredResult, lastCommandType = SyncCommandType.PERFORM_SYNC)
            }
        }
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        _uiState.update { it.copy(performSyncState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_UNEXPECTED_ERROR, sanitizedDiagnostic = e.message), lastCommandType = SyncCommandType.PERFORM_SYNC) }
    }
    try {
        // #595 五：成功保存并运行后一次性刷新完整同步 profile 状态。
        refreshSyncProfileState()
    } catch (_: Exception) { }
}

// #597 DryRun 事务需原子执行（保存配置→试运行→结构化结果），拆分会破坏事务一致性 — 待后续重构
@Suppress("CyclomaticComplexMethod", "CognitiveComplexMethod")
suspend fun SettingsViewModel.executeDryRunTransaction(
    command: SettingsTransactionCommand.SaveAndRunDryRun,
) {
    _uiState.update { it.copy(dryRunState = SyncCommandState.RUNNING, structuredSyncResult = null, lastCommandType = SyncCommandType.DRY_RUN) }
    try {
        if (!saveTransactionConfigAndSecrets(command.config, command.configRevision, command.secrets, command.secretsRevision)) {
            _uiState.update { it.copy(dryRunState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_SAVE_CONFIG_OR_SECRETS_FAILED)) }
            return
        }
        val exclusiveResult = SyncSession.runExclusive { _ ->
            // #595 三：试运行与正式同步共用同一文档事务链 — 启动前先
            // flush 活动正文到 Repository，失败按类型化错误中止（不得
            // 以未落盘的本地输入为 base 做试运行）。
            val flushOk = WorkspaceDocumentGate.flushActiveDocument()
            if (!flushOk) {
                return@runExclusive SyncCommandIoResult(
                    true, true,
                    StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_document_save_failed"),
                )
            }
            withContext(Dispatchers.IO) {
                val capability = settingsRepo.getSyncCapability()
                if (!capability.canRun) {
                    return@withContext SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "blocked", messageKey = capability.blockMessageKey ?: "sync_not_ready"))
                }
                // #595 十：操作作用域凭据 — 锁内设置 override（失败立即终止），
                // 结束后清除；不得使用上次正式同步留下的旧 token。
                val overrideOk = settingsRepo.setSyncSecretsOverrideStrict(command.secrets)
                if (!overrideOk) {
                    return@withContext SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_credentials_override_failed"))
                }
                try {
                    when (val r = settingsRepo.performSyncDryRunTyped(command.config)) {
                        is SyncDryRunOutcome.Success -> {
                            val plan = r.plan
                            val counts = SyncCounts(
                                uploaded = plan.filesToUpload.size,
                                downloaded = plan.filesToDownload.size,
                                deletedRemote = plan.filesToDeleteRemote.size,
                                deletedLocal = plan.filesToDeleteLocal.size,
                                conflicts = plan.conflicts.size,
                                ignored = plan.ignoredFiles.size
                            )
                            SyncCommandIoResult(true, true, StructuredSyncResult(
                                statusCode = "ok",
                                messageKey = "sync_dry_run_result",
                                counts = counts
                            ))
                        }
                        is SyncDryRunOutcome.Error -> SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = r.syncFailureKind.messageKey(), sanitizedDiagnostic = r.message)) 
                        SyncDryRunOutcome.NotLoaded -> SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = SyncFailureKind.NativeUnavailable.messageKey(), sanitizedDiagnostic = SyncFailureKind.NativeUnavailable.name))
                    }
                } finally {
                    settingsRepo.clearSyncSecretsOverride()
                }
            }
        }
        when (exclusiveResult) {
            is ExclusiveResult.Busy -> {
                _uiState.update { it.copy(dryRunState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = "busy", messageKey = "sync_already_running"), lastCommandType = SyncCommandType.DRY_RUN) }
            }
            is ExclusiveResult.Success -> {
                val ioResult = exclusiveResult.value
                _uiState.update { current ->
                    if (ioResult.isSuccess) {
                        current.copy(dryRunState = SyncCommandState.SUCCESS, structuredSyncResult = ioResult.structuredResult)
                    } else {
                        current.copy(dryRunState = SyncCommandState.FAILURE, structuredSyncResult = ioResult.structuredResult)
                    }
                }
            }
        }
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        _uiState.update { it.copy(dryRunState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_UNEXPECTED_ERROR, sanitizedDiagnostic = e.message)) }
    }
    try {
        // #595 五：成功保存并试运行后一次性刷新完整同步 profile 状态。
        refreshSyncProfileState()
    } catch (_: Exception) { }
}

// #597 诊断事务聚合多步检查（连接/凭据/配置/可达性），结果结构化汇总 — 待后续重构拆分各诊断步骤
@Suppress("CyclomaticComplexMethod", "CognitiveComplexMethod")
suspend fun SettingsViewModel.executeDiagnosticsTransaction(
    command: SettingsTransactionCommand.SaveAndRunDiagnostics,
) {
    _uiState.update { it.copy(testConnectionState = SyncCommandState.RUNNING, structuredSyncResult = null, lastCommandType = SyncCommandType.TEST_CONNECTION) }
    try {
        if (!saveTransactionConfigAndSecrets(command.config, command.configRevision, command.secrets, command.secretsRevision)) {
            _uiState.update { it.copy(testConnectionState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_SAVE_CONFIG_OR_SECRETS_FAILED)) }
            return
        }
        val exclusiveResult = SyncSession.runExclusive { _ ->
            // #595 三：连接诊断与正式同步共用同一文档事务链 — 启动前先
            // flush 活动正文到 Repository，失败按类型化错误中止。
            val flushOk = WorkspaceDocumentGate.flushActiveDocument()
            if (!flushOk) {
                return@runExclusive SyncCommandIoResult(
                    true, true,
                    StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_document_save_failed"),
                )
            }
            withContext(Dispatchers.IO) {
                val capability = settingsRepo.getSyncCapability()
                if (!capability.canRun) {
                    return@withContext SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "blocked", messageKey = capability.blockMessageKey ?: "sync_not_ready"))
                }
                // #595 十：操作作用域凭据 — 锁内设置 override（失败立即终止），
                // 结束后清除；不得使用上次正式同步留下的旧 token。
                val overrideOk = settingsRepo.setSyncSecretsOverrideStrict(command.secrets)
                if (!overrideOk) {
                    return@withContext SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_credentials_override_failed"))
                }
                try {
                    when (val r = settingsRepo.performSyncDiagnosticsTyped(command.config)) {
                        is SyncDiagnosticsOutcome.Success -> {
                            val diag = r.result
                            SyncCommandIoResult(true, true, StructuredSyncResult(
                                statusCode = if (diag.success) "ok" else "fail",
                                messageKey = "sync_test_connection_result",
                                messageArgs = mapOf(
                                    "network" to if (diag.networkOk) "ok" else "fail",
                                    "auth" to if (diag.authOk) "ok" else "fail",
                                    "repo" to if (diag.repoOk) "ok" else "fail",
                                    "branch" to if (diag.branchOk) "ok" else "fail"
                                ),
                                sanitizedDiagnostic = if (!diag.success) "connection_failed" else null
                            ))
                        }
                        is SyncDiagnosticsOutcome.Error -> SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = r.syncFailureKind.messageKey(), sanitizedDiagnostic = r.message)) 
                        SyncDiagnosticsOutcome.NotLoaded -> SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = SyncFailureKind.NativeUnavailable.messageKey(), sanitizedDiagnostic = SyncFailureKind.NativeUnavailable.name))
                    }
                } finally {
                    settingsRepo.clearSyncSecretsOverride()
                }
            }
        }
        when (exclusiveResult) {
            is ExclusiveResult.Busy -> {
                _uiState.update { it.copy(testConnectionState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = "busy", messageKey = "sync_already_running"), lastCommandType = SyncCommandType.TEST_CONNECTION) }
            }
            is ExclusiveResult.Success -> {
                val ioResult = exclusiveResult.value
                _uiState.update { current ->
                    if (ioResult.isSuccess) {
                        current.copy(testConnectionState = SyncCommandState.SUCCESS, structuredSyncResult = ioResult.structuredResult)
                    } else {
                        current.copy(testConnectionState = SyncCommandState.FAILURE, structuredSyncResult = ioResult.structuredResult)
                    }
                }
            }
        }
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        _uiState.update { it.copy(testConnectionState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_UNEXPECTED_ERROR, sanitizedDiagnostic = e.message)) }
    }
    try {
        val refreshedCapability = withContext(Dispatchers.IO) { settingsRepo.getSyncCapability() }
        val refreshedWarning = withContext(Dispatchers.IO) { settingsRepo.getSecureStorageWarning() }
        _uiState.update { it.copy(syncCapability = refreshedCapability, secureStorageWarning = refreshedWarning) }
    } catch (_: Exception) { }
}

suspend fun SettingsViewModel.refreshSyncProfileState() {
    val repo = settingsRepo
    val current = _uiState.value
    val committedProfile = withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile() }
    val refreshedCapability = withContext(Dispatchers.IO) { repo.getSyncCapability() }
    val refreshedWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() }
    _uiState.update {
        it.copy(
            syncConfig = if (!hasUnsavedSyncConfig()) {
                committedProfile.toSyncProfileLoadState().confirmedConfig ?: it.syncConfig
            } else {
                it.syncConfig
            },
            syncSecrets = if (!hasUnsavedSyncSecrets()) {
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

