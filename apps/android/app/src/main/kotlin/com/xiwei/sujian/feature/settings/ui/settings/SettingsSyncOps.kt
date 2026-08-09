package com.xiwei.sujian.feature.settings.ui

import com.xiwei.sujian.app.state.ActiveDocumentGate
import com.xiwei.sujian.feature.settings.data.SettingsSaveResult
import com.xiwei.sujian.feature.sync.data.AppSyncProfileReadResult
import com.xiwei.sujian.feature.sync.data.ExclusiveResult
import com.xiwei.sujian.feature.sync.data.model.SyncFailureKind
import com.xiwei.sujian.feature.sync.data.SyncSession
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.withContext

// ! # 设置同步事务操作（从 SettingsRoute 拆分）

// #597 同步事务协议字符串 — 提取为常量避免 StringLiteralDuplication
// SYNC_STATUS_ERROR 已移到 SettingsSyncResultMappers.kt
private const val MSG_SAVE_CONFIG_OR_SECRETS_FAILED = "save_config_or_secrets_failed"
private const val MSG_UNEXPECTED_ERROR = "unexpected_error"

// #600 评论 #3 问题二：无活动作品时的统一错误码 — 提取为常量避免 StringLiteralDuplication。
internal const val MSG_NO_ACTIVE_PROJECT = "sync_no_active_project"

suspend fun SettingsViewModel.executeTransaction(command: SettingsTransactionCommand) {
    when (command) {
        is SettingsTransactionCommand.SaveAndRunSync -> executeSyncTransaction(command)
        is SettingsTransactionCommand.SaveAndRunDryRun -> executeDryRunTransaction(command)
        is SettingsTransactionCommand.SaveAndRunDiagnostics -> executeDiagnosticsTransaction(command)
        // #600 评论 #4 问题三：应用级同步事务路由。
        is SettingsTransactionCommand.SaveAndRunAppSync -> executeAppSyncTransaction(command)
        is SettingsTransactionCommand.SaveAndRunAppDryRun -> executeAppDryRunTransaction(command)
        is SettingsTransactionCommand.SaveAndRunAppDiagnostics -> executeAppDiagnosticsTransaction(command)
    }
}

suspend fun SettingsViewModel.saveTransactionConfigAndSecrets(
    config: SyncConfig,
    configRevision: Long,
    secrets: SyncSecrets,
    secretsRevision: Long,
): Boolean {
    // #600 评论 #3 问题二：先拿当前作品 ID，再保存该作品 profile —
    // 无活动作品时不发布任何写入，避免把配置写到错误的作品或全局槽。
    val projectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
    if (projectId == null) return false
    val commitResult = withContext(Dispatchers.IO) { syncRepo.commitSyncProfile(projectId, config, secrets) }
    if (commitResult is SettingsSaveResult.Failed) return false
    if (projectSyncConfigRevision == configRevision) {
        projectSyncConfigPersistedRevision = configRevision
    }
    if (projectSyncSecretsRevision == secretsRevision) {
        projectSyncSecretsPersistedRevision = secretsRevision
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
 * #600 评论 #3 问题二：提取 capability 检查为独立 helper — 降低 runExclusiveSyncIo 认知复杂度。
 * 返回 null 表示 capability 检查通过；非 null 表示应提前返回该结果。
 */
private fun SettingsViewModel.checkSyncCapabilityForCurrentProject(): SyncCommandIoResult? {
    val projectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
    if (projectId == null) {
        return SyncCommandIoResult(
            true,
            true,
            StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_NO_ACTIVE_PROJECT),
        )
    }
    val capability = syncRepo.getSyncCapability(projectId)
    if (!capability.canRun) {
        return SyncCommandIoResult(
            true,
            true,
            StructuredSyncResult(
                statusCode = "blocked",
                messageKey = capability.blockMessageKey ?: "sync_not_ready",
            ),
        )
    }
    return null
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
            // #600 评论 #3 问题二：capability 按 projectId 路由 — 提取为 helper 降低认知复杂度。
            val capabilityCheck = checkSyncCapabilityForCurrentProject()
            if (capabilityCheck != null) return@withContext capabilityCheck
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
                    projectPerformSyncState = state,
                    projectSyncResult = result,
                    lastCommandType = SyncCommandType.PERFORM_SYNC,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        projectPerformSyncState = SyncCommandState.SUCCESS,
                        projectSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.PERFORM_SYNC,
                    )
                } else {
                    current.copy(
                        projectPerformSyncState = SyncCommandState.FAILURE,
                        projectSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.PERFORM_SYNC,
                    )
                }
            }
        },
    ) { _, _ ->
        // #600：sync 已改为 per-project — 设置页同步针对当前活动作品。
        val projectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
        if (projectId != null) {
            syncCoordinator.runSync(command.trigger, projectId).toIoResult()
        } else {
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_NO_ACTIVE_PROJECT),
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
                it.copy(
                    projectDryRunState = state,
                    projectSyncResult = result,
                    lastCommandType = SyncCommandType.DRY_RUN,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        projectDryRunState = SyncCommandState.SUCCESS,
                        projectSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.DRY_RUN,
                    )
                } else {
                    current.copy(
                        projectDryRunState = SyncCommandState.FAILURE,
                        projectSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.DRY_RUN,
                    )
                }
            }
        },
    ) { config, secrets ->
        // #600：sync 已改为 per-project — 试运行针对当前活动作品。
        val projectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
        if (projectId != null) {
            runExclusiveSyncIo(config, secrets) {
                syncRepo.performSyncDryRunTyped(projectId, it).toIoResult()
            }.toIoResult()
        } else {
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_NO_ACTIVE_PROJECT),
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
                    projectTestConnectionState = state,
                    projectSyncResult = result,
                    lastCommandType = SyncCommandType.TEST_CONNECTION,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        projectTestConnectionState = SyncCommandState.SUCCESS,
                        projectSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.TEST_CONNECTION,
                    )
                } else {
                    current.copy(
                        projectTestConnectionState = SyncCommandState.FAILURE,
                        projectSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.TEST_CONNECTION,
                    )
                }
            }
        },
    ) { config, secrets ->
        // #600 评论 #3 问题二：连接诊断针对当前活动作品。
        val diagProjectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
        if (diagProjectId != null) {
            runExclusiveSyncIo(config, secrets) {
                syncRepo.performSyncDiagnosticsTyped(diagProjectId, it).toIoResult()
            }.toIoResult()
        } else {
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = MSG_NO_ACTIVE_PROJECT),
            )
        }
    }
    try {
        val refreshedCapabilityProjectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
        if (refreshedCapabilityProjectId != null) {
            val refreshedCapability =
                withContext(Dispatchers.IO) { syncRepo.getSyncCapability(refreshedCapabilityProjectId) }
            val refreshedWarning = withContext(Dispatchers.IO) { settingsRepo.getSecureStorageWarning() }
            _uiState.update {
                it.copy(
                    projectSyncCapability = refreshedCapability,
                    secureStorageWarning = refreshedWarning,
                )
            }
        }
    } catch (_: Exception) {
    }
}

// ── #600 评论 #4 问题三：应用级同步事务（设置/全局星图/主题调色板）──
// 与作品级事务对称，但提交到应用级 profile（commitAppSyncProfile），
// 执行应用级同步 API（runAppSync / performAppSyncDryRun / performAppSyncDiagnostics）。
// 不依赖 ActiveProjectGate — 应用级同步目标唯一。

/**
 * 应用级事务保存配置与凭据 — 提交到应用级 profile（不带 projectId）。
 * 返回 false 表示提交失败，调用方应终止事务并显示错误。
 */
private suspend fun SettingsViewModel.saveAppTransactionConfigAndSecrets(
    config: SyncConfig,
    configRevision: Long,
    secrets: SyncSecrets,
    secretsRevision: Long,
): Boolean {
    val commitResult = withContext(Dispatchers.IO) { syncRepo.commitAppSyncProfile(config, secrets) }
    if (commitResult is SettingsSaveResult.Failed) return false
    if (appSyncConfigRevision == configRevision) {
        appSyncConfigPersistedRevision = configRevision
    }
    if (appSyncSecretsRevision == secretsRevision) {
        appSyncSecretsPersistedRevision = secretsRevision
    }
    return true
}

/**
 * 应用级事务共用骨架 — 镜像 [runCommandTransaction] 但用应用级保存与状态字段。
 */
private suspend fun SettingsViewModel.runAppCommandTransaction(
    command: SettingsTransactionCommand,
    setState: (SyncCommandState, StructuredSyncResult?) -> Unit,
    applyResult: (SyncCommandIoResult) -> Unit,
    operation: suspend (SyncConfig, SyncSecrets) -> SyncCommandIoResult,
) {
    setState(SyncCommandState.RUNNING, null)
    try {
        val saveOk =
            saveAppTransactionConfigAndSecrets(
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
        refreshAppSyncProfileState()
    } catch (_: Exception) {
    }
}

/**
 * 应用级排他锁内执行同步 IO — 镜像 [runExclusiveSyncIo] 但不检查当前作品 capability
 * （应用级无 projectId 路由的 capability）。
 */
private suspend fun SettingsViewModel.runExclusiveAppSyncIo(
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

suspend fun SettingsViewModel.executeAppSyncTransaction(command: SettingsTransactionCommand.SaveAndRunAppSync) {
    runAppCommandTransaction(
        command = command,
        setState = { state, result ->
            _uiState.update {
                it.copy(
                    appPerformSyncState = state,
                    appSyncResult = result,
                    lastCommandType = SyncCommandType.PERFORM_APP_SYNC,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        appPerformSyncState = SyncCommandState.SUCCESS,
                        appSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.PERFORM_APP_SYNC,
                    )
                } else {
                    current.copy(
                        appPerformSyncState = SyncCommandState.FAILURE,
                        appSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.PERFORM_APP_SYNC,
                    )
                }
            }
        },
    ) { _, _ ->
        // 应用级同步通过 SyncCoordinator.runAppSync 执行（含排他锁、secrets override、flush）。
        syncCoordinator.runAppSync(command.trigger).toIoResult()
    }
}

suspend fun SettingsViewModel.executeAppDryRunTransaction(command: SettingsTransactionCommand.SaveAndRunAppDryRun) {
    runAppCommandTransaction(
        command = command,
        setState = { state, result ->
            _uiState.update {
                it.copy(
                    appDryRunState = state,
                    appSyncResult = result,
                    lastCommandType = SyncCommandType.DRY_RUN_APP,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        appDryRunState = SyncCommandState.SUCCESS,
                        appSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.DRY_RUN_APP,
                    )
                } else {
                    current.copy(
                        appDryRunState = SyncCommandState.FAILURE,
                        appSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.DRY_RUN_APP,
                    )
                }
            }
        },
    ) { config, secrets ->
        runExclusiveAppSyncIo(config, secrets) {
            syncRepo.performAppSyncDryRunTyped(it).toAppIoResult()
        }.toIoResult()
    }
}

suspend fun SettingsViewModel.executeAppDiagnosticsTransaction(
    command: SettingsTransactionCommand.SaveAndRunAppDiagnostics,
) {
    runAppCommandTransaction(
        command = command,
        setState = { state, result ->
            _uiState.update {
                it.copy(
                    appTestConnectionState = state,
                    appSyncResult = result,
                    lastCommandType = SyncCommandType.TEST_CONNECTION_APP,
                )
            }
        },
        applyResult = { ioResult ->
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(
                        appTestConnectionState = SyncCommandState.SUCCESS,
                        appSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.TEST_CONNECTION_APP,
                    )
                } else {
                    current.copy(
                        appTestConnectionState = SyncCommandState.FAILURE,
                        appSyncResult = ioResult.structuredResult,
                        lastCommandType = SyncCommandType.TEST_CONNECTION_APP,
                    )
                }
            }
        },
    ) { config, secrets ->
        runExclusiveAppSyncIo(config, secrets) {
            syncRepo.performAppSyncDiagnosticsTyped(it).toAppIoResult()
        }.toIoResult()
    }
}

/**
 * #600 评论 #4 问题三：刷新应用级同步 profile 状态 — 读取应用级 committed profile，
 * 更新 syncConfig/syncSecrets（应用级与作品级共用同一 UI 字段，因为设置页只有一个同步配置区域；
 * 应用级 profile 是设置页"应用级同步"开关的真相来源）。
 *
 * 注意：此处不覆盖作品级 profile 加载状态（syncProfileLoadState 仍由作品级
 * [refreshSyncProfileState] 维护）。应用级事务结束后只刷新 config/secrets 的已确认值。
 */
suspend fun SettingsViewModel.refreshAppSyncProfileState() {
    val repo = syncRepo
    val committedAppProfile = withContext(Dispatchers.IO) { repo.loadCommittedAppSyncProfile() }
    val appLoadState = committedAppProfile.toAppSyncProfileLoadState()
    _uiState.update {
        it.copy(
            appSyncConfig =
                if (!hasUnsavedAppSyncConfig()) {
                    appLoadState.confirmedConfig ?: it.appSyncConfig
                } else {
                    it.appSyncConfig
                },
            appSyncSecrets =
                if (!hasUnsavedAppSyncSecrets()) {
                    appLoadState.confirmedSecrets ?: it.appSyncSecrets
                } else {
                    it.appSyncSecrets
                },
            appSyncProfileLoadState = appLoadState,
        )
    }
}

/** AppSyncProfileReadResult → 设置页加载状态（与作品级 toSyncProfileLoadState 对称）。 */
internal fun AppSyncProfileReadResult.toAppSyncProfileLoadState(): SyncProfileLoadState =
    when (this) {
        is AppSyncProfileReadResult.Found ->
            SyncProfileLoadState.Ready(snapshot.config, snapshot.secrets)
        is AppSyncProfileReadResult.NotConfigured ->
            SyncProfileLoadState.Unconfigured(snapshot.config, snapshot.secrets)
        is AppSyncProfileReadResult.Failed ->
            SyncProfileLoadState.Failed(kind, message)
    }

suspend fun SettingsViewModel.refreshSyncProfileState() {
    val repo = syncRepo
    val settingsRepoLocal = settingsRepo
    // #600 评论 #3 问题二：profile/capability 按 projectId 路由 —
    // 无活动作品时显示"未选择作品"状态，不读取任何作品数据。
    val projectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
    if (projectId == null) {
        _uiState.update {
            it.copy(
                projectSyncProfileLoadState =
                    com.xiwei.sujian.feature.settings.ui.SyncProfileLoadState.Failed(
                        com.xiwei.sujian.feature.sync.data.model.SyncFailureKind.Fatal,
                        MSG_NO_ACTIVE_PROJECT,
                    ),
            )
        }
        return
    }
    val committedProfile = withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile(projectId) }
    val refreshedCapability = withContext(Dispatchers.IO) { repo.getSyncCapability(projectId) }
    val refreshedWarning = withContext(Dispatchers.IO) { settingsRepoLocal.getSecureStorageWarning() }
    _uiState.update {
        it.copy(
            projectSyncConfig =
                if (!hasUnsavedProjectSyncConfig()) {
                    committedProfile.toSyncProfileLoadState().confirmedConfig ?: it.projectSyncConfig
                } else {
                    it.projectSyncConfig
                },
            projectSyncSecrets =
                if (!hasUnsavedProjectSyncSecrets()) {
                    committedProfile.toSyncProfileLoadState().confirmedSecrets ?: it.projectSyncSecrets
                } else {
                    it.projectSyncSecrets
                },
            projectSyncProfileLoadState = committedProfile.toSyncProfileLoadState(),
            projectSyncCapability = refreshedCapability,
            secureStorageWarning = refreshedWarning,
        )
    }
}
