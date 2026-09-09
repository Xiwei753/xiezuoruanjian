package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult

/**
 * 发布结果密封接口。
 *
 * - [Committed]：发布成功，事件已提交，可以安全移除
 * - [PendingRecovery]：存在未恢复的 pending publish，需要等待恢复完成
 * - [RetryableFailure]：可重试的失败，事件应保留并稍后重试
 */
sealed interface MirrorPublishResult {
    data object Committed : MirrorPublishResult

    data object PendingRecovery : MirrorPublishResult

    data object RetryableFailure : MirrorPublishResult
}

/**
 * ReadableMirrorPublisher — 异步发布正文到 Download/Sujian 镜像。
 *
 * 详见各执行器：[MirrorRollbackExecutor]、[MirrorPublishExecutor]、[MirrorRecoveryExecutor]。
 *
 * ## 循环依赖
 * 只依赖 [MirrorSnapshotSource]（只读快照），不持有 AppServiceBridge/ProjectBridge。
 *
 * ## 安全约束
 * - 不把 `content://` URI 传给 Rust——只把文本写入存储。
 * - 所有 I/O 失败只记日志，不阻断业务。
 */
class ReadableMirrorPublisher(
    private val source: MirrorSnapshotSource,
    private val router: MirrorStorageRouter,
    private val stateStore: ReadableMirrorStateStore,
) {
    private val codec = MirrorManifestCodec()
    private val journalWriter = MirrorJournalWriter(stateStore)
    private val planner = MirrorPublishPlanner(source, stateStore, codec)
    private val rollbackExecutor = MirrorRollbackExecutor(stateStore, journalWriter)
    private val publishExecutor =
        MirrorPublishExecutor(
            stateStore = stateStore,
            journalWriter = journalWriter,
            codec = codec,
            planner = planner,
            rollbackExecutor = rollbackExecutor,
            source = source,
            router = router,
            callbacks =
                MirrorPublishExecutorCallbacks(
                    ensurePendingRecovered = ::ensurePendingRecovered,
                    logNotLoaded = ::logNotLoaded,
                    logPublishAborted = ::logPublishAborted,
                ),
        )
    private val recoveryExecutor =
        MirrorRecoveryExecutor(
            stateStore = stateStore,
            journalWriter = journalWriter,
            rollbackExecutor = rollbackExecutor,
            publishExecutor = publishExecutor,
        )

    private suspend fun ensurePendingRecovered(): Boolean {
        return when (stateStore.readPendingPublish()) {
            is PendingPublishResult.NotExists -> true
            is PendingPublishResult.Corrupted -> false
            is PendingPublishResult.Success -> {
                recoverPendingPublishIfNeeded()
                stateStore.readPendingPublish() is PendingPublishResult.NotExists
            }
        }
    }

    /**
     * 检查并恢复 pending publish（如果存在）。
     */
    suspend fun recoverPendingPublishIfNeeded() {
        val pendingResult = stateStore.readPendingPublish()
        when (pendingResult) {
            is PendingPublishResult.NotExists -> return
            is PendingPublishResult.Corrupted -> {
                DiagnosticsLogger.e(
                    TAG,
                    "Pending publish journal is corrupted, cannot start new transaction",
                    pendingResult.error,
                )
                return
            }
            is PendingPublishResult.Success -> {
                val journalJson = pendingResult.json
                val journal = PendingMirrorPublish.fromJson(journalJson) ?: return
                DiagnosticsLogger.i(
                    TAG,
                    "Recovering pending publish: phase=${journal.phase}, " +
                        "projectId=${journal.projectId}, txType=${journal.transactionType}",
                )

                val storageResult = router.forBackendResult(journal.backend, journal.treeUri)
                if (storageResult.isFailure) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Storage not available during recovery: ${storageResult.exceptionOrNull()?.message}, " +
                            "keeping journal",
                    )
                    return
                }
                val storage = storageResult.getOrThrow()
                if (!storage.isSupported()) {
                    DiagnosticsLogger.w(TAG, "Storage not supported during recovery, keeping journal")
                    return
                }

                when (journal.phase) {
                    PendingMirrorPublish.PHASE_STAGE -> {
                        if (storage.rollback(journal.txId)) {
                            stateStore.clearPendingPublish()
                        } else {
                            DiagnosticsLogger.w(TAG, "Recover stage: rollback failed, keeping journal for retry")
                        }
                    }
                    PendingMirrorPublish.PHASE_PROMOTE -> {
                        recoveryExecutor.recoverPromotePhase(journal, storage)
                    }
                    PendingMirrorPublish.PHASE_CLEANUP -> {
                        recoveryExecutor.recoverCleanupPhase(journal, storage)
                    }
                    PendingMirrorPublish.PHASE_ROLLBACK -> {
                        recoveryExecutor.recoverRollbackPhase(journal, storage)
                    }
                    else -> {
                        DiagnosticsLogger.w(TAG, "Unknown phase in pending publish: ${journal.phase}")
                        stateStore.clearPendingPublish()
                    }
                }
            }
        }
    }

    suspend fun publishProject(projectId: String): MirrorPublishResult = publishExecutor.publishProject(projectId)

    suspend fun deleteProject(projectId: String): MirrorPublishResult = publishExecutor.deleteProject(projectId)

    suspend fun publishChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): MirrorPublishResult = publishProject(projectId)

    suspend fun publishAll(): MirrorPublishResult {
        try {
            if (!ensurePendingRecovered()) {
                return MirrorPublishResult.PendingRecovery
            }
            val storageResult = router.currentResult()
            if (storageResult.isFailure) {
                val error = storageResult.exceptionOrNull()
                DiagnosticsLogger.e(TAG, "Failed to get storage for publishAll: ${error?.message}")
                return MirrorPublishResult.RetryableFailure
            }
            val storage = storageResult.getOrThrow()
            if (!storage.isSupported()) {
                DiagnosticsLogger.i(TAG, SKIP_NOT_SUPPORTED)
                return MirrorPublishResult.RetryableFailure
            }
            val projectsResult = source.listProjects()
            if (projectsResult !is BridgeResult.Success) {
                logNotLoaded(projectsResult, "publishAll")
                return MirrorPublishResult.RetryableFailure
            }
            val liveProjectIds = projectsResult.data.map { it.id }.toSet()
            val cleanupResult = cleanupStaleProjects(liveProjectIds)
            if (cleanupResult !is MirrorPublishResult.Committed) return cleanupResult
            for (project in projectsResult.data) {
                val result = publishProject(project.id)
                if (result !is MirrorPublishResult.Committed) {
                    return result
                }
            }
            return MirrorPublishResult.Committed
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish all: ${e.message}", e)
            return MirrorPublishResult.RetryableFailure
        }
    }

    private suspend fun cleanupStaleProjects(liveProjectIds: Set<String>): MirrorPublishResult {
        for (staleProjectId in stateStore.getAllProjectIds()) {
            if (staleProjectId !in liveProjectIds) {
                val result = deleteProject(staleProjectId)
                if (result !is MirrorPublishResult.Committed) return result
            }
        }
        return MirrorPublishResult.Committed
    }

    private fun logNotLoaded(
        result: BridgeResult<*>,
        op: String,
    ) {
        when (result) {
            is BridgeResult.Error -> DiagnosticsLogger.w(TAG, "$op failed: ${result.fullEnvelope}")
            BridgeResult.NotLoaded -> DiagnosticsLogger.w(TAG, "Native library not loaded, skip $op")
            else -> {}
        }
    }

    private fun logPublishAborted(
        projectId: String,
        detail: String,
    ) {
        DiagnosticsLogger.w(TAG, "Publish project $projectId aborted: $detail")
    }

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val SKIP_NOT_SUPPORTED = "Mirror publish skipped: storage not supported"
    }
}
