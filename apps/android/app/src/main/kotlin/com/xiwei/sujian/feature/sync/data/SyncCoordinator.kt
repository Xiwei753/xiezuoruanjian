package com.xiwei.sujian.feature.sync.data
import com.xiwei.sujian.app.state.ActiveDocumentGate
import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.RepositoryException
import com.xiwei.sujian.core.interop.common.ResultEnvelope
import com.xiwei.sujian.feature.sync.data.model.FullSyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.withContext
import java.io.IOException

sealed class SyncOutcome {
    data class Completed(val result: FullSyncResult) : SyncOutcome()

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

/**
 * #630 评论 #1：全量同步完成纯事件 — 仅在 [SyncCoordinator.runFullSync] 最终映射成
 * [SyncOutcome.Completed] 后发一次。携带本次同步中发生下载的 projectId 列表，
 * 调用方据此刷新对应作品摘要（ProjectSummary 继续是列表唯一数据源）。
 */
data class FullSyncCompletedSignal(val downloadedProjectIds: List<String>)

/**
 * #625 评论5301204285 问题1 测试 seam：全量同步执行边界。
 *
 * 封装 [SyncCoordinator.runFullSync] 中依赖 native bridge 的四个操作（capability 判定、
 * 凭据 override 注入、performFullSync、override 清除）。生产实现 [RepositorySyncExecution]
 * 委托 [SyncRepository] 并在 [Dispatchers.IO] 执行，行为与原内联 withContext 调用完全一致；
 * 单测注入确定性实现，使 Completed 分支在 JVM 测试中真实执行 runFullSync 全部控制流
 * （enabled 判定 / capability 判定 / 独占锁 / flush / override / perform / 结果映射 / 信号发射）。
 *
 * 不引入第二状态机：本接口只搬运 bridge 调用与 IO 线程切换，不做任何业务判定。
 */
internal interface SyncExecutionPort {
    suspend fun capability(): SyncCapabilityData

    suspend fun setSecretsOverride(secrets: SyncSecrets): Boolean

    suspend fun perform(
        config: SyncConfig,
        forceSync: Boolean,
    ): BridgeResult<FullSyncResult>

    suspend fun clearSecretsOverride(): Boolean
}

class SyncCoordinator internal constructor(
    private val settingsRepository: SyncRepository,
    private val syncStatusRepository: SyncStatusRepository,
    private val appSyncDataBarrier: AppSyncDataBarrier? = null,
    private val syncExecution: SyncExecutionPort = RepositorySyncExecution(settingsRepository),
) {
    /**
     * #630 评论 #1：全量同步完成纯事件流。
     *
     * 仅在 [runFullSync] 最终映射成 [SyncOutcome.Completed] 后用 [MutableSharedFlow.tryEmit]
     * 非阻塞发射一次。`extraBufferCapacity = 1` 保证无订阅者时不丢最近一次完成事件。
     */
    private val _fullSyncCompleted =
        MutableSharedFlow<FullSyncCompletedSignal>(extraBufferCapacity = 1)
    val fullSyncCompleted: SharedFlow<FullSyncCompletedSignal> =
        _fullSyncCompleted.asSharedFlow()

    /**
     * #630 评论 #1：全量同步统一执行入口 — App target + 所有 Project target 一次同步。
     *
     * 流程只跑一次：
     * 1. 取得一份全局 profile snapshot；
     * 2. flush 应用级数据屏障；
     * 3. flush 当前正文；
     * 4. 写一次 secrets override；
     * 5. 调 Core `performFullSync`；
     * 6. 清 override；
     * 7. 根据聚合结果更新同步状态，发完成信号（携带下载的 projectId 列表）。
     *
     * 所有失败路径通过 [SyncFailureKind] 唯一分类：
     * - CancellationException → 原样抛出
     * - RetryableNetwork / RetryableIo → RetryableFailure，红色
     * - Authentication / Conflict / DirtyRepository / Protocol / NativeUnavailable / Fatal → TerminalFailure，红色
     * - 未配置或关闭 → Unconfigured/Disabled，灰色
     *
     * BridgeResult.NotLoaded 与 errorCode "NATIVE_NOT_LOADED" 统一进入 NativeUnavailable。
     */
    suspend fun runFullSync(
        trigger: SyncTrigger,
        snapshot: SyncProfileSnapshot? = null,
        forceSync: Boolean = false,
    ): SyncOutcome {
        DiagnosticsEvents.syncEvent(trigger.name.lowercase(), "start")
        try {
            val profile: SyncProfileSnapshot =
                snapshot ?: run {
                    val result =
                        // #630 评论 5307423953 Part A：顶栏手动同步入口直接走
                        // loadCommittedSyncProfile()，它内部已持有 SyncProfileGate.snapshotExclusive
                        // 并执行 ensureGlobalProfileMigrated()。不再外层套 SyncProfileGate。
                        withContext(Dispatchers.IO) {
                            settingsRepository.loadCommittedSyncProfile()
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
            val capability = syncExecution.capability()
            if (!capability.canRun) {
                syncStatusRepository.notifyUnconfigured()
                return SyncOutcome.Disabled
            }

            // #595 三：活动正文 flush 与同步执行必须在同一独占锁内串行 —
            // flush 保存磁盘版本后同步立即以该版本为 base；如果 flush 在锁外，
            // 两个同步触发可交叉 flush/执行，正文版本屏障失效。
            // #595 十：override 必须在取得同步独占锁（runExclusive）之后写入，
            // 设置失败立即终止（不静默继续）；操作结束后 finally 清除 override。
            val exclusiveResult =
                SyncSession.runExclusive { _ ->
                    // #600 评论 #5：应用级同步数据屏障 — flush 星图 store 落盘。
                    if (appSyncDataBarrier != null) {
                        val barrierFlushOk = appSyncDataBarrier.flushBeforeSync()
                        if (!barrierFlushOk) {
                            DiagnosticsLogger.w(
                                "SyncCoordinator",
                                "App sync data barrier flush failed — aborting (typed Fatal)",
                            )
                            syncStatusRepository.notifySyncFailed()
                            // #630 评论 5308040939 Part 1：预处理失败写同一份 Core
                            // FullSyncState（FatalError / preflight），重启后顶部红灯
                            // 不被旧 Success 覆盖。
                            settingsRepository.recordFullSyncPreflightFailure(
                                SyncStatus.FatalError,
                                "preflight",
                            )
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
                            "Active document flush failed before sync — aborting (typed DocumentSaveFailed)",
                        )
                        syncStatusRepository.notifySyncFailed()
                        settingsRepository.recordFullSyncPreflightFailure(
                            SyncStatus.FatalError,
                            "preflight",
                        )
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
                    val overrideOk = syncExecution.setSecretsOverride(profile.secrets)
                    if (!overrideOk) {
                        val error =
                            BridgeResult.Error(
                                ResultEnvelope.errorOf(
                                    "SYNC_CREDENTIALS_OVERRIDE_FAILED",
                                    "Failed to set sync credentials override",
                                ),
                                SyncFailureKind.Fatal,
                            )
                        // #630 评论 5308040939 Part 1：credentials override 失败同样写
                        // 同一份 Core FullSyncState（FatalError / preflight）。
                        settingsRepository.recordFullSyncPreflightFailure(
                            SyncStatus.FatalError,
                            "preflight",
                        )
                        resolveAndPublish(error)
                        return@runExclusive error
                    }
                    try {
                        val bridgeResult = syncExecution.perform(config, forceSync)
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
                        // #600 评论 #5：同步成功后失效星图/设置/主题缓存。
                        if (appSyncDataBarrier != null && bridgeResult is BridgeResult.Success) {
                            appSyncDataBarrier.reloadAfterFullSync(bridgeResult.data)
                        }
                        resolveAndPublish(bridgeResult)
                        bridgeResult
                    } finally {
                        syncExecution.clearSecretsOverride()
                    }
                }

            val outcome =
                when (exclusiveResult) {
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
            // 仅 Completed 发一次全量完成信号，携带下载的 projectId 列表。
            if (outcome is SyncOutcome.Completed) {
                val downloadedProjectIds =
                    outcome.result.targets
                        .filter { it.result.downloadedFiles.isNotEmpty() || it.result.localDeletes.isNotEmpty() }
                        .mapNotNull { it.projectId }
                _fullSyncCompleted.tryEmit(FullSyncCompletedSignal(downloadedProjectIds))
            }
            return outcome
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
     * #592 七：类型化失败直接来自 Bridge 边界（WriterException 变体），
     * 不再维护 Android 字符串错误码表；未知错误默认 Fatal。
     */
    internal fun classifyFailure(error: BridgeResult.Error): SyncFailureKind = SyncFailureKind.fromBridgeError(error)

    private fun mapToOutcome(result: FullSyncResult): SyncOutcome =
        when (result.overallStatus) {
            SyncStatus.Success,
            SyncStatus.NoChanges,
            SyncStatus.LatestWinsApplied,
            SyncStatus.BranchMissingRecovered,
            -> SyncOutcome.Completed(result)

            SyncStatus.RecoverableError ->
                SyncOutcome.RetryableFailure(
                    result.overallStatus,
                    SyncFailureKind.fromSyncStatus(result.overallStatus),
                )

            SyncStatus.Error,
            SyncStatus.Conflict,
            SyncStatus.PartialConflict,
            SyncStatus.FatalError,
            SyncStatus.DirtyRepoBlocked,
            ->
                SyncOutcome.TerminalFailure(
                    result.overallStatus,
                    SyncFailureKind.fromSyncStatus(result.overallStatus),
                )

            SyncStatus.Syncing -> {
                DiagnosticsLogger.w(
                    "SyncCoordinator",
                    "performFullSync returned Syncing — protocol error, mapping to terminal failure",
                )
                SyncOutcome.TerminalFailure(SyncStatus.FatalError, SyncFailureKind.Fatal)
            }
            SyncStatus.Idle,
            SyncStatus.ConfiguredNotTested,
            -> SyncOutcome.Unconfigured
        }

    private fun resolveAndPublish(result: BridgeResult<FullSyncResult>) {
        when (result) {
            is BridgeResult.Success -> resolveSyncStatus(result.data.overallStatus)
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
                DiagnosticsLogger.w("SyncCoordinator", "performFullSync returned Syncing — forcing to Failed")
                syncStatusRepository.notifySyncFailed()
            }
            SyncStatus.Idle,
            SyncStatus.ConfiguredNotTested,
            -> {
                syncStatusRepository.notifyUnconfigured()
            }
        }
    }

    /**
     * 生产默认实现 — 委托 [settingsRepository] 并在 [Dispatchers.IO] 执行，
     * 行为与原 runFullSync 内联 withContext(Dispatchers.IO) 调用完全一致。
     */
    private class RepositorySyncExecution(private val repo: SyncRepository) : SyncExecutionPort {
        override suspend fun capability(): SyncCapabilityData = withContext(Dispatchers.IO) { repo.getSyncCapability() }

        override suspend fun setSecretsOverride(secrets: SyncSecrets): Boolean =
            withContext(Dispatchers.IO) { repo.setSyncSecretsOverrideStrict(secrets) }

        override suspend fun perform(
            config: SyncConfig,
            forceSync: Boolean,
        ): BridgeResult<FullSyncResult> = withContext(Dispatchers.IO) { repo.performFullSync(config, forceSync) }

        override suspend fun clearSecretsOverride(): Boolean =
            withContext(Dispatchers.IO) { repo.clearSyncSecretsOverride() }
    }
}
