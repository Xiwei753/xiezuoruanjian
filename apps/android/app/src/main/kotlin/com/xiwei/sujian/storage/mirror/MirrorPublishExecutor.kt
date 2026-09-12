package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult

/**
 * MirrorPublishExecutor — 项目发布、删除和清理执行器。
 *
 * manifest 事务性写入委托给 [MirrorManifestTransactionExecutor]。
 */
internal data class MirrorPublishExecutorCallbacks(
    val ensurePendingRecovered: suspend () -> Boolean,
    val logNotLoaded: (BridgeResult<*>, String) -> Unit,
    val logPublishAborted: (String, String) -> Unit,
)

/**
 * manifest 事务参数。保留类型别名供 recovery/cleanup 等同包调用方使用。
 */
internal typealias ManifestTransactionParams = MirrorManifestTransactionExecutor.ManifestTransactionParams

/**
 * manifest 事务结果。保留类型别名供 recovery/cleanup 等同包调用方使用。
 */
internal typealias ManifestTransactionResult = MirrorManifestTransactionExecutor.ManifestTransactionResult

internal class MirrorPublishExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val codec: MirrorManifestCodec,
    private val planner: MirrorPublishPlanner,
    private val rollbackExecutor: MirrorRollbackExecutor,
    private val source: MirrorSnapshotSource,
    private val router: MirrorStorageRouter,
    private val callbacks: MirrorPublishExecutorCallbacks,
    private val workspace: MirrorTransactionWorkspace,
) {
    private val manifestExecutor = MirrorManifestTransactionExecutor(stateStore, journalWriter, planner, workspace)
    private val cleanupTransactionExecutor = MirrorCleanupTransactionExecutor(stateStore, workspace)
    private val publishProjectExecutor =
        MirrorPublishProjectExecutor(
            stateStore = stateStore,
            journalWriter = journalWriter,
            planner = planner,
            rollbackExecutor = rollbackExecutor,
            manifestExecutor = manifestExecutor,
            source = source,
            router = router,
            callbacks = callbacks,
            workspace = workspace,
        )
    private val ensurePendingRecovered get() = callbacks.ensurePendingRecovered
    private val logNotLoaded get() = callbacks.logNotLoaded
    private val logPublishAborted get() = callbacks.logPublishAborted

    // ════════════════════════════════════════════════════════════════════════════
    // #649 评论 5576949398 问题 1：committed manifest 读取的三态收口
    // ════════════════════════════════════════════════════════════════════════════

    /**
     * committed manifest 解析结果（#649 评论 5576949398 问题 1）。
     *
     * - [FirstPublish]：state 全空，真正首次发布，committedManifest = null
     * - [Baseline]：找到已提交 manifest 作为 frozen plan 基线
     * - [Stop]：状态损坏或迁移失败，调用方应返回 RetryableFailure 停止本轮发布
     */
    internal sealed interface CommittedManifestResolution {
        data object FirstPublish : CommittedManifestResolution

        data class Baseline(val manifest: MirrorManifest) : CommittedManifestResolution

        data object Stop : CommittedManifestResolution
    }

    /**
     * 读取并解析 committed manifest，处理四种情况（#649 评论 5576949398 问题 1）。
     *
     * - [CommittedManifestReadResult.NotExists] → [CommittedManifestResolution.FirstPublish]
     * - [CommittedManifestReadResult.Found] → [CommittedManifestResolution.Baseline]
     * - [CommittedManifestReadResult.Corrupted] → [CommittedManifestResolution.Stop]
     *   （停止本轮镜像，不碰 Download）
     * - [CommittedManifestReadResult.NeedsMigration] → 触发 [ReadableMirrorStateMigration]，
     *   迁移成功后重新读取；迁移失败 → [CommittedManifestResolution.Stop]
     *
     * @param storage 当前事务的 storage（迁移时用来读取 manifest 文件）
     * @return 解析结果，调用方据此决定首次发布 / 复用基线 / 停止
     */

    internal fun resolveCommittedManifestForPublish(storage: ReadableMirrorStorage): CommittedManifestResolution {
        return when (val result = stateStore.getCommittedManifestStrict()) {
            is CommittedManifestReadResult.NotExists -> CommittedManifestResolution.FirstPublish
            is CommittedManifestReadResult.Found -> CommittedManifestResolution.Baseline(result.manifest)
            is CommittedManifestReadResult.Corrupted -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Committed manifest corrupted, stopping publish: ${result.cause.message}",
                )
                CommittedManifestResolution.Stop
            }
            is CommittedManifestReadResult.NeedsMigration -> {
                // #649 评论 5576949398 问题 4：触发旧 state 迁移
                val migration = ReadableMirrorStateMigration(stateStore, storage)
                when (migration.migrate()) {
                    ReadableMirrorStateMigration.Result.SUCCESS -> {
                        // 迁移成功后重新读取
                        when (val reread = stateStore.getCommittedManifestStrict()) {
                            is CommittedManifestReadResult.Found ->
                                CommittedManifestResolution.Baseline(reread.manifest)
                            is CommittedManifestReadResult.NotExists ->
                                CommittedManifestResolution.FirstPublish
                            is CommittedManifestReadResult.Corrupted -> {
                                DiagnosticsLogger.w(
                                    TAG,
                                    "Committed manifest still corrupted after migration: ${reread.cause.message}",
                                )
                                CommittedManifestResolution.Stop
                            }
                            is CommittedManifestReadResult.NeedsMigration -> {
                                DiagnosticsLogger.w(
                                    TAG,
                                    "State still needs migration after migration attempt, stopping",
                                )
                                CommittedManifestResolution.Stop
                            }
                        }
                    }
                    ReadableMirrorStateMigration.Result.FAILURE -> {
                        DiagnosticsLogger.w(TAG, "State migration failed, stopping publish")
                        CommittedManifestResolution.Stop
                    }
                }
            }
        }
    }

    /**
     * 发布整个项目：事务性发布流程。
     *
     * #649 评论 5561465552 第 4 点：准备 → 写入 → 提交 manifest → 清旧文件。
     * #649 评论 5561974464 问题 2：publishManifest() 仍用旧 state，且"失败保留旧镜像"不成立。
     *
     * 1. **准备**：读完整快照和所有章节正文到内存，计算 desired state。
     * 2. **暂存**：所有新正文先写到 staging（不能覆盖 committed ref）。
     * 3. **提升**：promote 所有暂存文件到最终位置。
     * 4. **提交 manifest**：用 desiredEntries 直接构造 manifest，manifest 成功后才批量更新 stateStore。
     * 5. **清理**：删除不再被引用的旧文件。
     *
     * pendingPublish journal 在整个流程中记录进度，成功后清除。
     *
     * @return 发布结果 [MirrorPublishResult]
     */
    internal fun cleanupCommittedTransaction(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        allLiveKeys: Set<ChapterKey>? = null,
    ): Boolean = cleanupTransactionExecutor.cleanupCommittedTransaction(journal, storage, allLiveKeys)

    /**
     * 发布单章正文。
     *
     * #649 评论 5562715833 问题 6：单章发布委托 [publishProject] 的事务性路径，
     * 不另走非事务简化路径（旧 writeChapterContent + publishManifest）。
     * 统一走 stage → promote → 事务性 manifest → cleanup → journal 的事务性发布。
     *
     * @return 发布结果 [MirrorPublishResult]
     */
    internal suspend fun publishProject(projectId: String): MirrorPublishResult =
        publishProjectExecutor.publishProject(projectId)

    internal suspend fun deleteProject(projectId: String): MirrorPublishResult {
        try {
            return executeDeleteProject(projectId)
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to delete project: ${e.message}", e)
            return MirrorPublishResult.RetryableFailure
        }
    }

    private suspend fun executeDeleteProject(projectId: String): MirrorPublishResult {
        // 门控：确保 pending 已恢复
        if (!ensurePendingRecovered()) {
            return MirrorPublishResult.PendingRecovery
        }
        // #649 评论 5565862745 问题 4：使用 currentTransactionResult() 获取完整事务上下文
        val txContextResult = router.currentTransactionResult()
        if (txContextResult.isFailure) {
            val error = txContextResult.exceptionOrNull()
            DiagnosticsLogger.e(TAG, "Failed to get transaction context for delete: ${error?.message}")
            return MirrorPublishResult.RetryableFailure
        }
        val txContext = txContextResult.getOrThrow()
        val storage = txContext.storage
        if (!storage.isSupported()) {
            DiagnosticsLogger.i(TAG, "Mirror delete skipped: storage not supported")
            return MirrorPublishResult.RetryableFailure
        }
        // 1. 获取旧条目 + 2. 读取 committed manifest 并生成 frozen plan（#649 评论 5576464076 问题 3）
        //    #649 评论 5562715833 问题 7：不在 removed.isEmpty() 时 early return，即使空作品也继续走事务流程
        //    #649 评论 5576949398 问题 1：用 getCommittedManifestStrict 三态读取，不再把损坏当首次发布
        val preparation =
            prepareDeleteFrozenPlan(projectId, storage)
                ?: return MirrorPublishResult.RetryableFailure

        // 3. 写 pending journal（transactionType=DELETE_PROJECT, phase=CLEANUP）
        //    #649 评论 5563333323 缺口 2：journal 写入失败则停止
        val txId = "${System.currentTimeMillis()}-${projectId.take(8)}"
        if (!writeDeletePendingJournal(projectId, txId, txContext, preparation)) {
            return MirrorPublishResult.RetryableFailure
        }
        // 3. 事务提交新 manifest（已不含该项目）
        //    #649 评论 5562715833 问题 7：snapshot=null 确保 manifest 不再引用该项目
        //    #649 评论 5576464076 问题 3：使用 frozen plan 生成 manifestTargetJson
        val manifestTargetJson =
            buildDeleteManifestTargetJson(projectId, preparation.frozenPlan)
                ?: return MirrorPublishResult.RetryableFailure
        // #649 评论 5562715833 问题 5：传 journalContext，manifest 事务每步落 journal
        // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
        val deleteJournalContext = buildDeleteJournalContext(projectId, txId, txContext, preparation)
        val manifestResult =
            publishManifestWithDesiredTransactional(
                ManifestTransactionParams(
                    projectId = projectId,
                    snapshot = null,
                    desiredEntries = emptyMap(),
                    txId = txId,
                    journalContext = deleteJournalContext,
                    items = emptyMap(),
                    storage = storage,
                    prebuiltTargetJson = manifestTargetJson,
                ),
            )
        if (manifestResult == null) {
            DiagnosticsLogger.w(TAG, "Delete project $projectId aborted: manifest write failed")
            // manifest 失败不清除 journal，下次恢复会重试
            return MirrorPublishResult.RetryableFailure
        }
        // 4. manifest 成功后更新 journal + committed baseline
        //    #649 评论 5562462046 问题 4：恢复时需区分 manifest 是否已提交
        //    #649 评论 5563333323 缺口 2：journal 写入失败则保留 journal 重试
        // #649 评论 5576949398 问题 2：从 manifestResult.committedJournal 继续，不再用 writePendingPublishJournal 从旧字段重建。
        if (!persistDeleteCommittedBaseline(projectId, manifestResult)) {
            return MirrorPublishResult.RetryableFailure
        }
        // 5. 从 state store 删除该项目条目
        //    #649 评论 5563333323 缺口 2：removeAllProjectEntries 返回 Result
        if (!removeDeleteProjectState(projectId)) {
            return MirrorPublishResult.RetryableFailure
        }
        // 6. 调用统一 cleanup 删旧正文 + manifestBackup + tx staging
        //    #649 评论 5563333323 缺口 3：统一 cleanupCommittedTransaction
        // #649 评论 5576949398 问题 2：直接用 manifestResult.committedJournal 作为 cleanup journal，
        return finalizeDeleteCleanup(projectId, manifestResult, storage)
    }

    private data class DeletePreparation(
        val removed: Map<ChapterKey, ChapterMirrorEntry>,
        val frozenPlan: FrozenManifestPlan?,
        val frozenPlanJson: String?,
        val frozenPlanHash: String?,
    )

    private fun prepareDeleteFrozenPlan(
        projectId: String,
        storage: ReadableMirrorStorage,
    ): DeletePreparation? {
        val removed = stateStore.getProjectEntries(projectId)
        val committedManifestResolution = resolveCommittedManifestForPublish(storage)
        if (committedManifestResolution is CommittedManifestResolution.Stop) {
            DiagnosticsLogger.w(
                TAG,
                "Delete project $projectId aborted: committed manifest corrupted or migration failed",
            )
            return null
        }
        val committedManifest =
            when (committedManifestResolution) {
                is CommittedManifestResolution.Baseline -> committedManifestResolution.manifest
                CommittedManifestResolution.FirstPublish -> null
                CommittedManifestResolution.Stop -> null // 上面已 return，这里不会走到
            }
        val frozenPlan =
            if (committedManifest != null) {
                buildFrozenDeleteManifestPlan(committedManifest, projectId)
            } else {
                // 没有已提交 manifest（首次发布），不需要 frozen plan
                null
            }
        val frozenPlanJson = frozenPlan?.let { frozenManifestPlanToJson(it) }
        val frozenPlanHash = if (frozenPlanJson != null) computeContentHash(frozenPlanJson) else null
        return DeletePreparation(removed, frozenPlan, frozenPlanJson, frozenPlanHash)
    }

    private fun writeDeletePendingJournal(
        projectId: String,
        txId: String,
        txContext: MirrorStorageTransactionContext,
        preparation: DeletePreparation,
    ): Boolean {
        // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
        if (!journalWriter.writePendingPublishJournal(
                PendingJournalParams(
                    projectId = projectId,
                    transactionType = MirrorTransactionType.DELETE_PROJECT,
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    txId = txId,
                    backend = txContext.backend,
                    treeUri = txContext.treeUri,
                    oldEntries = preparation.removed,
                    newEntries = emptyMap(),
                    stagedRefs = emptyMap(),
                    items = emptyMap(),
                    removedProjectIds = setOf(projectId),
                    manifestOldRef = null,
                    manifestStagedRef = null,
                    manifestNewRef = null,
                    manifestBackupRef = null,
                    manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                    frozenManifestPlan = preparation.frozenPlanJson,
                    frozenManifestPlanHash = preparation.frozenPlanHash,
                ),
            )
        ) {
            DiagnosticsLogger.w(TAG, "Delete project $projectId aborted: journal write failed")
            return false
        }
        return true
    }

    private fun buildDeleteManifestTargetJson(
        projectId: String,
        frozenPlan: FrozenManifestPlan?,
    ): String? {
        val manifestTargetJson =
            if (frozenPlan != null) {
                frozenPlanToManifestJson(frozenPlan, emptyMap())
            } else {
                null
            }
        if (frozenPlan != null && manifestTargetJson == null) {
            DiagnosticsLogger.w(TAG, "Delete project $projectId aborted: frozenPlanToManifestJson failed")
            return null
        }
        return manifestTargetJson
    }

    private fun buildDeleteJournalContext(
        projectId: String,
        txId: String,
        txContext: MirrorStorageTransactionContext,
        preparation: DeletePreparation,
    ): PendingMirrorPublish {
        return PendingMirrorPublish(
            txId = txId,
            backend = txContext.backend,
            treeUri = txContext.treeUri,
            projectId = projectId,
            transactionType = MirrorTransactionType.DELETE_PROJECT,
            phase = PendingMirrorPublish.PHASE_CLEANUP,
            oldEntries = preparation.removed,
            newEntries = emptyMap(),
            stagedRefs = emptyMap(),
            items = emptyMap(),
            removedProjectIds = setOf(projectId),
            manifestOldRef = null,
            manifestStagedRef = null,
            manifestNewRef = null,
            manifestBackupRef = null,
            frozenManifestPlan = preparation.frozenPlanJson,
            frozenManifestPlanHash = preparation.frozenPlanHash,
        )
    }

    private fun persistDeleteCommittedBaseline(
        projectId: String,
        manifestResult: ManifestTransactionResult,
    ): Boolean {
        if (!journalWriter.persistPendingJournal(manifestResult.committedJournal)) {
            DiagnosticsLogger.w(
                TAG,
                "Delete project $projectId: cleanup journal write failed, keeping journal for retry",
            )
            return false
        }
        // #649 评论 5576464076 问题 2：DELETE 也幂等写入 committed manifest，
        // 确保下一笔 frozen plan 基线正确（不会因 manifest 未持久化而误判为首次发布）。
        // #649 评论 5576949398 问题 2：用 persistCommittedBaselineFromJournal 统一写入。
        if (!journalWriter.persistCommittedBaselineFromJournal(manifestResult.committedJournal)) {
            DiagnosticsLogger.w(
                TAG,
                "Delete project $projectId: persistCommittedBaseline failed, keeping journal for retry",
            )
            return false
        }
        return true
    }

    private fun removeDeleteProjectState(projectId: String): Boolean {
        val removeResult = stateStore.removeAllProjectEntries(projectId)
        if (removeResult.isFailure) {
            DiagnosticsLogger.w(
                TAG,
                "Delete project $projectId: removeAllProjectEntries failed, keeping journal for retry",
            )
            return false
        }
        return true
    }

    private fun finalizeDeleteCleanup(
        projectId: String,
        manifestResult: ManifestTransactionResult,
        storage: ReadableMirrorStorage,
    ): MirrorPublishResult {
        // #649 评论 5576949398 问题 2：直接用 manifestResult.committedJournal 作为 cleanup journal，
        // 它已包含 manifestOldRef/manifestStagedRef/manifestNewRef/manifestBackupRef/
        // isManifestCommitted/manifestSwapState 等全部 manifest committed 状态。
        val deleteCleanupJournal = manifestResult.committedJournal
        if (cleanupCommittedTransaction(deleteCleanupJournal, storage, allLiveKeys = null)) {
            // #649 评论 5564820566 问题 5：delete 成功后移除 publishedProjectId
            // #649 评论 5565067997 修复 6：检查 removePublishedProjectId 返回值
            if (!stateStore.removePublishedProjectId(projectId)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Delete project $projectId: removePublishedProjectId failed, keeping journal for retry",
                )
                return MirrorPublishResult.RetryableFailure
            }
            stateStore.clearPendingPublish()
            return MirrorPublishResult.Committed
        } else {
            DiagnosticsLogger.w(
                TAG,
                "Delete project $projectId: cleanup partial failure, keeping journal for retry",
            )
            return MirrorPublishResult.RetryableFailure
        }
    }

    internal suspend fun publishManifestWithDesiredTransactional(
        params: ManifestTransactionParams,
    ): ManifestTransactionResult? = manifestExecutor.publishManifestWithDesiredTransactional(params)

    private fun projectIdKeepingJournal(projectId: String): String = "$projectId, keeping journal"

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val SKIP_NOT_SUPPORTED = "Mirror publish skipped: storage not supported"
    }
}
