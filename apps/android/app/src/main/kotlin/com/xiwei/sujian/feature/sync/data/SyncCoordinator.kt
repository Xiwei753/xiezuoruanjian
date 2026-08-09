package com.xiwei.sujian.feature.sync.data
import com.xiwei.sujian.app.state.ActiveDocumentGate
import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.RepositoryException
import com.xiwei.sujian.core.interop.common.ResultEnvelope
import com.xiwei.sujian.feature.sync.data.model.SyncFailureKind
import com.xiwei.sujian.feature.sync.data.model.SyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.IOException

sealed class SyncOutcome {
    data class Completed(val result: SyncResult) : SyncOutcome()

    data object Disabled : SyncOutcome()

    data object Unconfigured : SyncOutcome()

    data object Busy : SyncOutcome()

    /**
     * #592 三：携带具体 [SyncFailureKind]，使正式同步、试运行、连接诊断
     * 全部通过 kind.messageKey() 获得同一用户提示映射。
     */
    data class RetryableFailure(
        val status: SyncStatus,
        val kind: SyncFailureKind = SyncFailureKind.fromSyncStatus(status),
    ) : SyncOutcome()

    data class TerminalFailure(
        val status: SyncStatus,
        val kind: SyncFailureKind = SyncFailureKind.fromSyncStatus(status),
    ) : SyncOutcome()
}

class SyncCoordinator(
    private val settingsRepository: SyncRepository,
    private val syncStatusRepository: SyncStatusRepository,
    private val appSyncDataBarrier: AppSyncDataBarrier? = null,
) {
    /**
     * #592 四：统一异常边界 — 每条路径必须结束在明确终态。
     *
     * #592 六：一次同步操作只使用同一份不可变 ProjectSyncProfileSnapshot
     * （generation + config + secrets）。调用方（AutoSyncWorker 等）可传入
     * 预先取得的 snapshot，避免二次读取；未传入时在锁内取一次完整快照。
     * snapshot 的 secrets 通过进程级 override 注入 Rust，整个操作不再从磁盘
     * 二次读取 config/secrets。
     *
     * 所有失败路径通过 [SyncFailureKind] 唯一分类：
     * - CancellationException → 原样抛出
     * - RetryableNetwork / RetryableIo → RetryableFailure，红色
     * - Authentication / Conflict / DirtyRepository / Protocol / NativeUnavailable / Fatal → TerminalFailure，红色
     * - 未配置或关闭 → Unconfigured/Disabled，灰色
     * - performSync 返回 Syncing → 协议错误，TerminalFailure
     *
     * BridgeResult.NotLoaded 与 errorCode "NATIVE_NOT_LOADED" 统一进入 NativeUnavailable，
     * 不再分别维护字符串白名单和独立分支。
     */
    suspend fun runSync(
        trigger: SyncTrigger,
        projectId: String,
        snapshot: ProjectSyncProfileSnapshot? = null,
    ): SyncOutcome {
        DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "start")
        try {
            val profile: ProjectSyncProfileSnapshot =
                snapshot ?: run {
                    val result =
                        withContext(Dispatchers.IO) {
                            SyncProfileGate.snapshotExclusive { settingsRepository.snapshotSyncProfile(projectId) }
                        }
                    when (result) {
                        is SyncProfileReadResult.Found -> result.snapshot
                        is SyncProfileReadResult.NotConfigured -> result.snapshot
                        is SyncProfileReadResult.Failed -> {
                            DiagnosticsLogger.w("SyncCoordinator", "Sync profile snapshot failed: ${result.message}")
                            syncStatusRepository.notifySyncFailed()
                            return result.kind.toOutcome()
                        }
                    }
                }
            val config = profile.config
            if (config.enabled != true) {
                syncStatusRepository.notifyUnconfigured()
                return SyncOutcome.Unconfigured
            }
            val capability = withContext(Dispatchers.IO) { settingsRepository.getSyncCapability(projectId) }
            if (!capability.canRun) {
                syncStatusRepository.notifyUnconfigured()
                return SyncOutcome.Disabled
            }

            // #592 六：整个操作只使用这份 snapshot 的凭据。
            // #595 十：override 必须在取得同步独占锁（runExclusive）之后写入，
            // 两个同步同时触发时不可能出现“A 写入 token A、B 写入 token B、
            // A 实际使用 token B”；设置失败立即终止（不静默继续）；
            // 操作结束后 finally 清除 override，陈旧凭据不得泄漏到后续操作
            // （Core 的 refresh_secrets_override 在已有 override 时不再读磁盘）。
            // #595 三：活动正文 flush 与同步执行必须在同一独占锁内串行 —
            // flush 保存磁盘版本后同步立即以该版本为 base；如果 flush 在锁外，
            // 两个同步触发可交叉 flush/执行，正文版本屏障失效。
            val exclusiveResult =
                SyncSession.runExclusive { _ ->
                    val flushOk = ActiveDocumentGate.flushActiveDocument()
                    if (!flushOk) {
                        DiagnosticsLogger.w(
                            "SyncCoordinator",
                            "Active document flush failed before sync — aborting (typed DocumentSaveFailed)",
                        )
                        syncStatusRepository.notifySyncFailed()
                        return@runExclusive BridgeResult.Error(
                            ResultEnvelope.errorOf(
                                "DOCUMENT_FLUSH_FAILED",
                                "Active document could not be persisted before sync",
                            ),
                            SyncFailureKind.DocumentSaveFailed,
                        )
                    }
                    // #595 三：签发文档身份 lease — 同步前后校验文档是否仍是同一 target/session/epoch。
                    val identityBeforeSync = ActiveDocumentGate.activeDocumentIdentity()
                    syncStatusRepository.notifySyncStarted()
                    val overrideOk =
                        withContext(Dispatchers.IO) {
                            settingsRepository.setSyncSecretsOverrideStrict(profile.secrets)
                        }
                    if (!overrideOk) {
                        val error =
                            BridgeResult.Error(
                                ResultEnvelope.errorOf(
                                    "SYNC_CREDENTIALS_OVERRIDE_FAILED",
                                    "Failed to set sync credentials override",
                                ),
                                SyncFailureKind.Fatal,
                            )
                        resolveAndPublish(error)
                        return@runExclusive error
                    }
                    try {
                        val bridgeResult =
                            withContext(Dispatchers.IO) { settingsRepository.performSync(projectId, config) }
                        // #595 三：校验文档身份 — 同步期间章节切换/关闭导致身份变化时，
                        // 不应用同步结果，新输入作为下一代 dirty 文档继续保存。
                        val identityAfterSync = ActiveDocumentGate.activeDocumentIdentity()
                        if (identityBeforeSync != null && identityAfterSync != null &&
                            identityBeforeSync != identityAfterSync
                        ) {
                            DiagnosticsLogger.w(
                                "SyncCoordinator",
                                "Document identity changed during sync — result not applied: " +
                                    "$identityBeforeSync -> $identityAfterSync",
                            )
                            val staleError =
                                BridgeResult.Error(
                                    ResultEnvelope.errorOf(
                                        "DOCUMENT_IDENTITY_CHANGED",
                                        "Active document changed during sync — result not applied",
                                    ),
                                    SyncFailureKind.DocumentSaveFailed,
                                )
                            resolveAndPublish(staleError)
                            return@runExclusive staleError
                        }
                        resolveAndPublish(bridgeResult)
                        bridgeResult
                    } finally {
                        withContext(Dispatchers.IO) { settingsRepository.clearSyncSecretsOverride() }
                    }
                }

            return when (exclusiveResult) {
                is ExclusiveResult.Busy -> {
                    SyncOutcome.Busy
                }
                is ExclusiveResult.Success -> {
                    when (val br = exclusiveResult.value) {
                        is BridgeResult.Success -> mapToOutcome(br.data)
                        is BridgeResult.Error -> classifyFailure(br).toOutcome()
                        BridgeResult.NotLoaded -> {
                            DiagnosticsLogger.w("SyncCoordinator", "Native library not loaded — NativeUnavailable")
                            SyncFailureKind.NativeUnavailable.toOutcome()
                        }
                    }
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: IOException) {
            DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "io_exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return SyncFailureKind.RetryableIo.toOutcome()
        } catch (e: RepositoryException) {
            DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "repository_exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return e.kind.toOutcome()
        } catch (e: Exception) {
            DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return SyncFailureKind.Fatal.toOutcome()
        }
    }

    /**
     * #600 评论 #4 问题三：应用级同步执行入口 — 设置/全局星图/主题调色板。
     *
     * 镜像 [runSync] 的结构（snapshot → enabled → capability → runExclusive →
     * flush + secrets override + performAppSync → 分类结果），但**不依赖 projectId** —
     * 应用级同步目标唯一，不经过 ActiveProjectGate。
     *
     * - snapshot 未传入时在锁内取 [SyncRepository.snapshotAppSyncProfile]；
     * - secrets override 是进程级（与作品级共用同一 override 机制），由
     *   [SyncSession.runExclusive] 保证同一时刻只有一个同步在执行，不会串用 token；
     * - 应用级同步不签发文档身份 lease（应用级同步目标不含活动正文，无需文档身份校验）；
     * - 失败分类复用 [classifyFailure] / [mapToOutcome]，与作品级一致。
     */
    suspend fun runAppSync(
        trigger: SyncTrigger,
        appSnapshot: AppSyncProfileSnapshot? = null,
    ): SyncOutcome {
        DiagnosticsEvents.syncEvent("app_${trigger.name.lowercase()}", "start")
        try {
            val profile: AppSyncProfileSnapshot =
                appSnapshot ?: run {
                    val result =
                        withContext(Dispatchers.IO) {
                            SyncProfileGate.snapshotExclusive { settingsRepository.snapshotAppSyncProfile() }
                        }
                    when (result) {
                        is AppSyncProfileReadResult.Found -> result.snapshot
                        is AppSyncProfileReadResult.NotConfigured -> result.snapshot
                        is AppSyncProfileReadResult.Failed -> {
                            DiagnosticsLogger.w(
                                "SyncCoordinator",
                                "App sync profile snapshot failed: ${result.message}",
                            )
                            syncStatusRepository.notifySyncFailed()
                            return result.kind.toOutcome()
                        }
                    }
                }
            val config = profile.config
            if (config.enabled != true) {
                syncStatusRepository.notifyUnconfigured()
                return SyncOutcome.Unconfigured
            }
            // 应用级 capability：config.enabled + remoteUrl 非空即可（应用级无 projectId 路由的 capability）。
            // 复用 shouldSync 的前置判定逻辑 — 此处只检查 enabled，远程 URL 在 Core perform_app_sync 内校验。

            val exclusiveResult =
                SyncSession.runExclusive { _ ->
                    // #600 评论 #5：应用级同步数据屏障 — flush 星图 store 落盘，确保同步引擎读到完整本地数据。
                    if (appSyncDataBarrier != null) {
                        val barrierFlushOk = appSyncDataBarrier.flushBeforeSync()
                        if (!barrierFlushOk) {
                            DiagnosticsLogger.w(
                                "SyncCoordinator",
                                "App sync data barrier flush failed — aborting (typed Fatal)",
                            )
                            syncStatusRepository.notifySyncFailed()
                            return@runExclusive BridgeResult.Error(
                                ResultEnvelope.errorOf(
                                    "APP_SYNC_BARRIER_FLUSH_FAILED",
                                    "App sync data barrier flush failed before sync",
                                ),
                                SyncFailureKind.Fatal,
                            )
                        }
                    }
                    val flushOk = ActiveDocumentGate.flushActiveDocument()
                    if (!flushOk) {
                        DiagnosticsLogger.w(
                            "SyncCoordinator",
                            "Active document flush failed before app sync — aborting (typed DocumentSaveFailed)",
                        )
                        syncStatusRepository.notifySyncFailed()
                        return@runExclusive BridgeResult.Error(
                            ResultEnvelope.errorOf(
                                "DOCUMENT_FLUSH_FAILED",
                                "Active document could not be persisted before app sync",
                            ),
                            SyncFailureKind.DocumentSaveFailed,
                        )
                    }
                    syncStatusRepository.notifySyncStarted()
                    val overrideOk =
                        withContext(Dispatchers.IO) {
                            settingsRepository.setSyncSecretsOverrideStrict(profile.secrets)
                        }
                    if (!overrideOk) {
                        val error =
                            BridgeResult.Error(
                                ResultEnvelope.errorOf(
                                    "SYNC_CREDENTIALS_OVERRIDE_FAILED",
                                    "Failed to set app sync credentials override",
                                ),
                                SyncFailureKind.Fatal,
                            )
                        resolveAndPublish(error)
                        return@runExclusive error
                    }
                    try {
                        val bridgeResult =
                            withContext(Dispatchers.IO) { settingsRepository.performAppSync(config) }
                        // #600 评论 #5：同步成功后失效星图/设置/主题缓存，使后续读取拿到同步后的最新数据。
                        if (appSyncDataBarrier != null && bridgeResult is BridgeResult.Success) {
                            appSyncDataBarrier.reloadAfterSync(bridgeResult.data)
                        }
                        resolveAndPublish(bridgeResult)
                        bridgeResult
                    } finally {
                        withContext(Dispatchers.IO) { settingsRepository.clearSyncSecretsOverride() }
                    }
                }

            return when (exclusiveResult) {
                is ExclusiveResult.Busy -> {
                    SyncOutcome.Busy
                }
                is ExclusiveResult.Success -> {
                    when (val br = exclusiveResult.value) {
                        is BridgeResult.Success -> mapToOutcome(br.data)
                        is BridgeResult.Error -> classifyFailure(br).toOutcome()
                        BridgeResult.NotLoaded -> {
                            DiagnosticsLogger.w("SyncCoordinator", "Native library not loaded — NativeUnavailable")
                            SyncFailureKind.NativeUnavailable.toOutcome()
                        }
                    }
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: IOException) {
            DiagnosticsEvents.syncEvent("app_${trigger.name.lowercase()}", "io_exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return SyncFailureKind.RetryableIo.toOutcome()
        } catch (e: RepositoryException) {
            DiagnosticsEvents.syncEvent("app_${trigger.name.lowercase()}", "repository_exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return e.kind.toOutcome()
        } catch (e: Exception) {
            DiagnosticsEvents.syncEvent("app_${trigger.name.lowercase()}", "exception: " + e.message)
            syncStatusRepository.notifySyncFailed()
            return SyncFailureKind.Fatal.toOutcome()
        }
    }

    /**
     * #592 七：类型化失败直接来自 Bridge 边界（WriterException 变体），
     * 不再维护 Android 字符串错误码表；未知错误默认 Fatal。
     */
    internal fun classifyFailure(error: BridgeResult.Error): SyncFailureKind = SyncFailureKind.fromBridgeError(error)

    private fun mapToOutcome(result: SyncResult): SyncOutcome =
        when (result.status) {
            SyncStatus.Success,
            SyncStatus.NoChanges,
            SyncStatus.LatestWinsApplied,
            SyncStatus.BranchMissingRecovered,
            -> SyncOutcome.Completed(result)

            SyncStatus.RecoverableError ->
                SyncOutcome.RetryableFailure(
                    result.status,
                    SyncFailureKind.fromSyncStatus(result.status),
                )

            SyncStatus.Error,
            SyncStatus.Conflict,
            SyncStatus.PartialConflict,
            SyncStatus.FatalError,
            SyncStatus.DirtyRepoBlocked,
            ->
                SyncOutcome.TerminalFailure(
                    result.status,
                    SyncFailureKind.fromSyncStatus(result.status),
                )

            SyncStatus.Syncing -> {
                DiagnosticsLogger.w(
                    "SyncCoordinator",
                    "performSync returned Syncing — protocol error, mapping to terminal failure",
                )
                SyncOutcome.TerminalFailure(SyncStatus.FatalError, SyncFailureKind.Fatal)
            }
            SyncStatus.Idle,
            SyncStatus.ConfiguredNotTested,
            -> SyncOutcome.Unconfigured
        }

    private fun resolveAndPublish(result: BridgeResult<SyncResult>) {
        when (result) {
            is BridgeResult.Success -> resolveSyncStatus(result.data.status)
            is BridgeResult.Error -> syncStatusRepository.notifySyncFailed()
            BridgeResult.NotLoaded -> syncStatusRepository.notifySyncFailed()
        }
    }

    private fun resolveSyncStatus(status: SyncStatus) {
        when (status) {
            SyncStatus.Success,
            SyncStatus.NoChanges,
            SyncStatus.LatestWinsApplied,
            SyncStatus.BranchMissingRecovered,
            -> {
                syncStatusRepository.notifySyncSuccess()
            }
            SyncStatus.Conflict,
            SyncStatus.PartialConflict,
            SyncStatus.RecoverableError,
            SyncStatus.FatalError,
            SyncStatus.DirtyRepoBlocked,
            SyncStatus.Error,
            -> {
                syncStatusRepository.notifySyncFailed()
            }
            SyncStatus.Syncing -> {
                DiagnosticsLogger.w("SyncCoordinator", "performSync returned Syncing — forcing to Failed")
                syncStatusRepository.notifySyncFailed()
            }
            SyncStatus.Idle,
            SyncStatus.ConfiguredNotTested,
            -> {
                syncStatusRepository.notifyUnconfigured()
            }
        }
    }
}
