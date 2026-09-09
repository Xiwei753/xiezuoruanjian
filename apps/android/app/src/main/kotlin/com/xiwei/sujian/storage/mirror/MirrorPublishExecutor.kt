package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import java.time.Instant
import java.time.format.DateTimeFormatter

/**
 * MirrorPublishExecutor — manifest 事务性写入和项目发布执行器。
 */
internal data class MirrorPublishExecutorCallbacks(
    val ensurePendingRecovered: suspend () -> Boolean,
    val logNotLoaded: (BridgeResult<*>, String) -> Unit,
    val logPublishAborted: (String, String) -> Unit,
)

internal class MirrorPublishExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val codec: MirrorManifestCodec,
    private val planner: MirrorPublishPlanner,
    private val rollbackExecutor: MirrorRollbackExecutor,
    private val source: MirrorSnapshotSource,
    private val router: MirrorStorageRouter,
    private val callbacks: MirrorPublishExecutorCallbacks,
) {
    private val ensurePendingRecovered get() = callbacks.ensurePendingRecovered
    private val logNotLoaded get() = callbacks.logNotLoaded
    private val logPublishAborted get() = callbacks.logPublishAborted
    /**
     * committed manifest 解析结果（#649 评论 5576949398 问题 1）。
     *
     * - [FirstPublish]：state 全空，真正首次发布，committedManifest = null
     * - [Baseline]：找到已提交 manifest 作为 frozen plan 基线
     * - [Stop]：状态损坏或迁移失败，调用方应返回 RetryableFailure 停止本轮发布
     */

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


    internal data class ManifestTransactionResult(
        val committedJournal: PendingMirrorPublish,
        val newRef: MirrorFileRef,
    )


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
    ): Boolean {
        // 1. 每个 COMMITTED item 的 backupOldRef（oldRef 已被 backupCommitted 移走，不删 oldRef）
        //    #649 评论 5564379115 问题 3：只删 backupOldRef，不要同时删 oldRef（同一 MediaStore row）
        //    #649 评论 5565067997 修复 5：用 lookup() 三态查询，Failed 时停止。
        val backupOk = cleanupBackupOldRefs(journal, storage)
        // 2. 已删除章节的旧 ref（snapshot 中已不存在的旧 key）
        //    先 lookup 确认文件存在再删，避免 MediaStore 同一条 row 被删两次
        //    #649 评论 5565067997 修复 5：用 lookup() 三态查询
        val oldEntriesOk = cleanupOldEntries(journal, storage, allLiveKeys)
        // 3. manifestBackupRef（manifest 事务备份）
        //    #649 评论 5565067997 修复 5：用 lookup() 三态查询
        val manifestBackupOk = cleanupManifestBackupRef(journal, storage)
        // 4. tx staging/backup 根（事务暂存目录）
        //    #649 评论 5566303837 问题 6：检查 rollback 返回值，失败时不清 journal
        val txOk = rollbackTxStaging(journal, storage)
        return backupOk && oldEntriesOk && manifestBackupOk && txOk
    }

    private fun cleanupBackupOldRefs(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ): Boolean {
        var allSuccess = true
        for ((_, item) in journal.items) {
            if (!deleteBackupOldRef(item, storage)) allSuccess = false
        }
        return allSuccess
    }

    private fun deleteBackupOldRef(item: PendingItem, storage: ReadableMirrorStorage): Boolean {
        val ref = item.backupOldRef ?: return true
        return try {
            deleteRefByLookup(storage, ref.relativePath) { cause ->
                "cleanup: lookup backupOldRef failed ${ref.uri}: ${cause?.message}"
            }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to delete backupOldRef ${item.backupOldRef?.uri}", e)
            false
        }
    }

    private fun deleteRefByLookup(
        storage: ReadableMirrorStorage,
        relativePath: String,
        failMessage: (cause: Throwable?) -> String,
    ): Boolean {
        // #649 评论 5565067997 修复 5：用 lookup() 区分 Missing 和 Failed
        return when (val lookupResult = storage.lookup(relativePath)) {
            is MirrorLookupResult.Found -> storage.delete(lookupResult.ref)
            is MirrorLookupResult.Missing -> true // 文件已不存在，目标已达到
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(TAG, failMessage(lookupResult.cause))
                false
            }
        }
    }

    private fun cleanupOldEntries(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        allLiveKeys: Set<ChapterKey>?,
    ): Boolean {
        var allSuccess = true
        for ((key, entry) in journal.oldEntries) {
            val shouldDelete = allLiveKeys?.let { key !in it } ?: true
            if (shouldDelete) {
                if (!deleteOldEntry(entry, storage)) allSuccess = false
                // 同时从 stateStore 移除该条目
                // #649 评论 5564379115 问题 3：必须检查 removeChapterEntry 返回值
                if (!removeOldChapterEntry(key)) allSuccess = false
            }
        }
        return allSuccess
    }

    private fun deleteOldEntry(entry: ChapterMirrorEntry, storage: ReadableMirrorStorage): Boolean {
        return try {
            deleteRefByLookup(storage, entry.relativePath) { cause ->
                "cleanup: lookup old entry failed ${entry.uri}: ${cause?.message}"
            }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to delete old entry ${entry.uri}", e)
            false
        }
    }

    private fun removeOldChapterEntry(key: ChapterKey): Boolean {
        return try {
            stateStore.removeChapterEntry(key.projectId, key.volumeId, key.chapterId)
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to removeChapterEntry for ${key.chapterId}", e)
            false
        }
    }

    private fun cleanupManifestBackupRef(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ): Boolean {
        val ref = journal.manifestBackupRef ?: return true
        return try {
            deleteRefByLookup(storage, ref.relativePath) { cause ->
                "cleanup: lookup manifestBackupRef failed ${ref.uri}: ${cause?.message}"
            }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to delete manifestBackupRef ${journal.manifestBackupRef?.uri}", e)
            false
        }
    }

    private fun rollbackTxStaging(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ): Boolean {
        var allSuccess = true
        try {
            if (!storage.rollback(journal.txId)) {
                DiagnosticsLogger.w(TAG, "cleanup: tx staging cleanup failed for ${journal.txId}")
                allSuccess = false
            }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to rollback tx ${journal.txId}", e)
            allSuccess = false
        }
        return allSuccess
    }

    /**
     * 发布单章正文。
     *
     * #649 评论 5562715833 问题 6：单章发布委托 [publishProject] 的事务性路径，
     * 不另走非事务简化路径（旧 writeChapterContent + publishManifest）。
     * 统一走 stage → promote → 事务性 manifest → cleanup → journal 的事务性发布。
     *
     * @return 发布结果 [MirrorPublishResult]
     */
    internal suspend fun publishProject(projectId: String): MirrorPublishResult {
        try {
            return executePublishProject(projectId)
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish project: ${e.message}", e)
            return MirrorPublishResult.RetryableFailure
        }
    }

    private suspend fun executePublishProject(projectId: String): MirrorPublishResult {
            // #649 评论 5564379115 问题 4：每次开启新事务前都要检查当前 journal，
            // 不再用一次性 AtomicBoolean 表示"整个进程以后都没有 pending"。
            if (!ensurePendingRecovered()) {
                return MirrorPublishResult.PendingRecovery
            }
            // 1. 准备阶段：读完整快照、计算 desired state、生成事务 ID
            val context = preparePublishContext(projectId)
                ?: return MirrorPublishResult.RetryableFailure
            // #649 评论 5573750754 修复 6：第一笔 stageText() 之前先落 PHASE_STAGE journal。
            // 这样死在任何一章 staging 中间，下次都能按 txId 清掉整棵 staging。
            if (!writePublishStageJournal(projectId, context)) {
                return MirrorPublishResult.RetryableFailure
            }
            // 2. 暂存阶段：所有新正文先写到 staging（不能覆盖 committed ref）
            val stageResult = stageAllContent(projectId, context)
                ?: return MirrorPublishResult.RetryableFailure
            // #649 评论 5575551884 问题 3 / 5575950895 问题 3+5：冻结全局 manifest 计划。
            // #649 评论 5576949398 问题 1：用 getCommittedManifestStrict 三态读取。
            val frozenContext = buildPublishFrozenPlanAndJournal(projectId, context, stageResult)
                ?: return MirrorPublishResult.RetryableFailure
            // 3. 提升阶段：promote 所有暂存文件到最终位置（逐项更新 journal）
            // #649 评论 5564820566 问题 3：两步 journalable backup — prepareBackup + vacateCommitted
            val promoteResult = promoteAllStaged(projectId, context, stageResult, frozenContext)
                ?: return MirrorPublishResult.RetryableFailure
            // 4. 提交 manifest：走事务性 manifest 写入（stage → promote → setManifestUri → 删 backup）
            //    #649 评论 5562715833 问题 5：传 currentJournal，manifest 事务每步落 journal
            //    #649 评论 5576464076 问题 1：正常 UPSERT 使用 frozen plan 生成 manifestTargetJson
            val manifestResult = commitPublishManifest(projectId, context, stageResult, promoteResult, frozenContext)
                ?: return MirrorPublishResult.RetryableFailure
            // #649 评论 5573310799 问题 5：先写 PHASE_CLEANUP journal（在 stateStore 更新之前），
            //    这样死在 stateStore 已更新、cleanup journal 还没落盘之间，磁盘 journal 仍是 PHASE_PROMOTE。
            val cleanupJournal = persistPublishCommittedBaseline(projectId, promoteResult, manifestResult)
                ?: return MirrorPublishResult.RetryableFailure
            // 5. 清理阶段：调用统一 cleanup 函数
            //    #649 评论 5563333323 缺口 3：统一 cleanupCommittedTransaction
            // #649 评论 5575950895 问题 3：直接用 currentJournal（已包含 manifest committed 状态）
            return finalizePublishCleanup(projectId, context, cleanupJournal)
    }

    private data class PublishContext(
        val txContext: MirrorStorageTransactionContext,
        val storage: ReadableMirrorStorage,
        val snapshot: ProjectWorkspaceSnapshot,
        val oldEntries: Map<ChapterKey, ChapterMirrorEntry>,
        val allKeys: Set<ChapterKey>,
        val writePlan: List<WritePlanEntry>,
        val txId: String,
    )

    private data class StageResult(
        val stagedRefs: Map<ChapterKey, StagedMirrorRef>,
        val desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        val items: Map<ChapterKey, PendingItem>,
    )

    private data class FrozenPlanContext(
        val frozenPlan: FrozenManifestPlan,
        val currentJournal: PendingMirrorPublish,
    )

    private data class PromoteResult(
        val promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
        val items: Map<ChapterKey, PendingItem>,
        val currentJournal: PendingMirrorPublish,
    )

    private data class ItemBackupOutcome(val backupRef: MirrorFileRef, val vacated: Boolean)

    private suspend fun preparePublishContext(projectId: String): PublishContext? {
            // #649 评论 5565862745 问题 4：使用 currentTransactionResult() 获取完整事务上下文
            // 事务开始时拿一次 backend/treeUri/storage，整笔事务复用此 context
            val txContextResult = router.currentTransactionResult()
            if (txContextResult.isFailure) {
                // state.json 损坏或 DOCUMENT_TREE + treeUri=null
                val error = txContextResult.exceptionOrNull()
                DiagnosticsLogger.e(TAG, "Failed to get transaction context: ${error?.message}")
                return null
            }
            val txContext = txContextResult.getOrThrow()
            val storage = txContext.storage
            if (!storage.isSupported()) {
                DiagnosticsLogger.i(TAG, SKIP_NOT_SUPPORTED)
                return null
            }
            val snapshotResult = source.getProjectWorkspaceSnapshot(projectId)
            if (snapshotResult !is BridgeResult.Success) {
                logNotLoaded(snapshotResult, "publishProject")
                return null
            }
            val snapshot = snapshotResult.data
            val oldEntries = stateStore.getProjectEntries(projectId)
            // 1. 准备阶段：读完整快照和所有章节正文到内存，计算 desired state
            val allKeys = mutableSetOf<ChapterKey>()
            for (volumeWithChapters in snapshot.volumes) {
                for (chapter in volumeWithChapters.chapters) {
                    allKeys.add(ChapterKey(projectId, volumeWithChapters.volume.id, chapter.id))
                }
            }
            val usedRelativePaths = mutableSetOf<String>()
            val writePlan = planner.buildWritePlan(projectId, snapshot, oldEntries, usedRelativePaths)
            if (writePlan == null) {
                logPublishAborted(projectId, "failed to build write plan")
                return null
            }
            // 生成事务 ID
            val txId = "${System.currentTimeMillis()}-${projectId.take(8)}"
            return PublishContext(txContext, storage, snapshot, oldEntries, allKeys, writePlan, txId)
    }

    private fun writePublishStageJournal(
        projectId: String,
        context: PublishContext,
    ): Boolean {
            if (!journalWriter.writePendingPublishJournal(PendingJournalParams(
                    projectId = projectId,
                    transactionType = MirrorTransactionType.UPSERT_PROJECT,
                    phase = PendingMirrorPublish.PHASE_STAGE,
                    txId = context.txId,
                    backend = context.txContext.backend,
                    treeUri = context.txContext.treeUri,
                    oldEntries = context.oldEntries,
                    newEntries = emptyMap(),
                    stagedRefs = emptyMap(),
                    items = emptyMap(),
                    removedProjectIds = emptySet(),
                    manifestOldRef = null,
                    manifestStagedRef = null,
                    manifestNewRef = null,
                    manifestBackupRef = null,
                    manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                ))
            ) {
                logPublishAborted(projectId, "PHASE_STAGE journal write failed")
                return false
            }
            return true
    }

    private fun stageAllContent(
        projectId: String,
        context: PublishContext,
    ): StageResult? {
            val stagedRefs = mutableMapOf<ChapterKey, StagedMirrorRef>()
            val desiredEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
            val items = mutableMapOf<ChapterKey, PendingItem>()
            for (planEntry in context.writePlan) {
                val contentHash = computeContentHash(planEntry.content)
                val staged =
                    context.storage.stageText(
                        txId = context.txId,
                        relativePath = planEntry.relativePath,
                        mimeType = MIME_MARKDOWN,
                        text = planEntry.content,
                    )
                if (staged == null) {
                    logPublishAborted(projectId, "stage failed for ${planEntry.key.chapterId}")
                    context.storage.rollback(context.txId)
                    return null
                }
                stagedRefs[planEntry.key] = staged
                // 记录 desired entry（promote 后填 URI）
                desiredEntries[planEntry.key] =
                    ChapterMirrorEntry(
                        uri = "",
                        relativePath = planEntry.relativePath,
                        revision = planEntry.chapter.updatedAt.toEpochMillis(),
                        contentHash = contentHash,
                    )
                val oldRef =
                    planEntry.oldEntry?.let { MirrorFileRef(uri = it.uri, relativePath = it.relativePath) }
                items[planEntry.key] =
                    PendingItem(
                        key = planEntry.key,
                        stagedRef = staged,
                        oldRef = oldRef,
                        backupOldRef = null,
                        promotedRef = null,
                        state = PendingItem.STATE_STAGED,
                        // #649 评论 5566303837 问题 2：记录旧正文 hash 用于崩溃恢复校验
                        oldContentHash = planEntry.oldEntry?.contentHash,
                    )
            }
            return StageResult(stagedRefs, desiredEntries, items)
    }

    private fun buildPublishFrozenPlanAndJournal(
        projectId: String,
        context: PublishContext,
        stageResult: StageResult,
    ): FrozenPlanContext? {
            // buildFrozenManifestPlan 返回 null 时直接停止事务，不带着 null plan 继续
            // backup/vacate/promote（#649 评论 5575950895 问题 3）。
            val committedManifestResolution = resolveCommittedManifestForPublish(context.storage)
            if (committedManifestResolution is CommittedManifestResolution.Stop) {
                logPublishAborted(projectId, "committed manifest corrupted or migration failed")
                return null
            }
            val committedManifest =
                when (committedManifestResolution) {
                    is CommittedManifestResolution.Baseline -> committedManifestResolution.manifest
                    CommittedManifestResolution.FirstPublish -> null
                    CommittedManifestResolution.Stop -> null // 上面已 return，这里不会走到
                }
            val frozenPlan =
                buildFrozenManifestPlan(
                    committedManifest = committedManifest,
                    targetProjectId = projectId,
                    targetSnapshot = context.snapshot,
                    targetDesiredEntries = stageResult.desiredEntries,
                )
            if (frozenPlan == null) {
                logPublishAborted(projectId, "failed to build frozen manifest plan")
                context.storage.rollback(context.txId)
                return null
            }
            val frozenPlanJson = frozenManifestPlanToJson(frozenPlan)
            val frozenPlanHash = computeContentHash(frozenPlanJson)
            // #649 评论 5575950895 问题 3：advancing journal 模式。
            // 第一份 durable pending journal 直接收成 currentJournal，传入 frozenManifestPlan/Hash，
            // 后续步骤只 currentJournal = currentJournal.copy(...) + journalWriter.persistPendingJournal(currentJournal)，
            // 不再让 writePendingPublishJournal 重建整份对象。以后新增字段也不会被旧 builder 擦掉。
            val currentJournal =
                PendingMirrorPublish(
                    txId = context.txId,
                    backend = context.txContext.backend,
                    treeUri = context.txContext.treeUri,
                    projectId = projectId,
                    transactionType = MirrorTransactionType.UPSERT_PROJECT,
                    phase = PendingMirrorPublish.PHASE_PROMOTE,
                    oldEntries = context.oldEntries,
                    newEntries = stageResult.desiredEntries,
                    stagedRefs = stageResult.stagedRefs,
                    items = stageResult.items,
                    removedProjectIds = emptySet(),
                    manifestOldRef = null,
                    manifestStagedRef = null,
                    manifestNewRef = null,
                    manifestBackupRef = null,
                    frozenManifestPlan = frozenPlanJson,
                    frozenManifestPlanHash = frozenPlanHash,
                )
            if (!journalWriter.persistPendingJournal(currentJournal)) {
                logPublishAborted(projectId, "journal write failed after stage")
                context.storage.rollback(context.txId)
                return null
            }
            return FrozenPlanContext(frozenPlan, currentJournal)
    }

    private data class ItemBackupContext(
        val projectId: String,
        val key: ChapterKey,
        val txId: String,
        val oldRef: MirrorFileRef,
        val items: MutableMap<ChapterKey, PendingItem>,
        val stagedRefs: Map<ChapterKey, StagedMirrorRef>,
        val storage: ReadableMirrorStorage,
        val currentJournal: PendingMirrorPublish,
    )

    private suspend fun promoteAllStaged(
        projectId: String,
        context: PublishContext,
        stageResult: StageResult,
        frozenContext: FrozenPlanContext,
    ): PromoteResult? {
            val stagedRefs = stageResult.stagedRefs
            val desiredEntries = stageResult.desiredEntries
            val items = stageResult.items.toMutableMap()
            val storage = context.storage
            val txId = context.txId
            val promotedEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
            var currentJournal = frozenContext.currentJournal
            for ((key, staged) in stagedRefs) {
                val item = items[key]!!
                // 已完成则跳过（恢复场景）
                if (item.state == PendingItem.STATE_PROMOTED && item.promotedRef != null) {
                    promotedEntries[key] =
                        ChapterMirrorEntry(
                            uri = item.promotedRef.uri,
                            relativePath = item.promotedRef.relativePath,
                            revision = desiredEntries[key]!!.revision,
                            contentHash = desiredEntries[key]!!.contentHash,
                        )
                    continue
                }
                val oldRef = item.oldRef
                // 1. 两步备份 old（如果有）
                // #649 评论 5565067997 修复 1：用 STATE_BACKUP_READY / STATE_OLD_VACATED 显式状态
                if (oldRef != null && item.state != PendingItem.STATE_OLD_VACATED) {
                    val backupCtx = ItemBackupContext(projectId, key, txId, oldRef, items, stagedRefs, storage, currentJournal)
                    currentJournal = backupAndVacateItem(backupCtx, item) ?: return null
                }
                // 2. promote staged（不删 old）
                //    #649 评论 5566303837 问题 2：OLD_VACATED 崩溃窗口检查
                val newRef = promoteItemStaged(key, item, staged, desiredEntries, storage)
                if (newRef == null) {
                    logPublishAborted(projectId, "promote failed for ${key.chapterId}")
                    // #649 评论 5564379115 问题 2：统一事务回滚，不逐 item 回滚
                    rollbackExecutor.rollbackWholePublishTransaction(txId, items, stagedRefs, storage, currentJournal)
                    return null
                }
                promotedEntries[key] =
                    ChapterMirrorEntry(
                        uri = newRef.uri,
                        relativePath = newRef.relativePath,
                        revision = desiredEntries[key]!!.revision,
                        contentHash = desiredEntries[key]!!.contentHash,
                    )
                // 逐项更新 journal（记录该 item 已 PROMOTED）
                items[key] = items[key]!!.copy(promotedRef = newRef, state = PendingItem.STATE_PROMOTED)
                currentJournal = currentJournal.copy(items = items)
                if (!journalWriter.persistPendingJournal(currentJournal)) {
                    logPublishAborted(projectId, "journal write failed after promote for ${key.chapterId}")
                    // #649 评论 5564379115 问题 2：统一事务回滚，不逐 item 回滚
                    rollbackExecutor.rollbackWholePublishTransaction(txId, items, stagedRefs, storage, currentJournal)
                    return null
                }
            }
            return PromoteResult(promotedEntries, items, currentJournal)
    }

    private fun backupAndVacateItem(
        ctx: ItemBackupContext,
        item: PendingItem,
    ): PendingMirrorPublish? {
            val backupOutcome = prepareItemBackup(ctx) ?: return null
            // #649 评论 5565067997 修复 1：journal 先写 STATE_BACKUP_READY
            ctx.items[ctx.key] = item.copy(backupOldRef = backupOutcome.backupRef, state = PendingItem.STATE_BACKUP_READY)
            // #649 评论 5575950895 问题 3：advancing journal 模式
            var currentJournal = ctx.currentJournal.copy(items = ctx.items)
            if (!journalWriter.persistPendingJournal(currentJournal)) {
                logPublishAborted(ctx.projectId, "journal write failed after backup prepare for ${ctx.key.chapterId}")
                rollbackExecutor.rollbackWholePublishTransaction(ctx.txId, ctx.items, ctx.stagedRefs, ctx.storage, currentJournal)
                return null
            }
            // 2. vacate old（如果 prepareBackup 还没 move old）
            if (!backupOutcome.vacated && !ctx.storage.vacateCommitted(ctx.oldRef)) {
                logPublishAborted(ctx.projectId, "vacate failed for ${ctx.key.chapterId}")
                rollbackExecutor.rollbackWholePublishTransaction(ctx.txId, ctx.items, ctx.stagedRefs, ctx.storage, currentJournal)
                return null
            }
            // #649 评论 5565067997 修复 1：vacate 成功后写 STATE_OLD_VACATED
            ctx.items[ctx.key] = ctx.items[ctx.key]!!.copy(state = PendingItem.STATE_OLD_VACATED)
            currentJournal = currentJournal.copy(items = ctx.items)
            if (!journalWriter.persistPendingJournal(currentJournal)) {
                logPublishAborted(ctx.projectId, "journal write failed after vacate for ${ctx.key.chapterId}")
                rollbackExecutor.rollbackWholePublishTransaction(ctx.txId, ctx.items, ctx.stagedRefs, ctx.storage, currentJournal)
                return null
            }
            return currentJournal
    }

    private fun prepareItemBackup(ctx: ItemBackupContext): ItemBackupOutcome? {
            // 检查崩溃窗口：backup 已就绪但 vacate 未完成
            // 使用 lookupBackup 三态查询（#649 评论 5565862745 问题 3）
            val backupResult = ctx.storage.lookupBackup(ctx.txId, ctx.oldRef.relativePath)
            return when (backupResult) {
                is MirrorLookupResult.Found -> {
                    // backup 已存在，直接复用
                    // #649 评论 5565067997 修复 5：用 lookup() 三态查询判断 old 是否已 vacate
                    val oldLookup = ctx.storage.lookup(ctx.oldRef.relativePath)
                    when (oldLookup) {
                        is MirrorLookupResult.Missing -> ItemBackupOutcome(backupResult.ref, true)
                        is MirrorLookupResult.Found -> ItemBackupOutcome(backupResult.ref, false)
                        is MirrorLookupResult.Failed -> {
                            logPublishAborted(ctx.projectId, "lookup old failed for ${ctx.key.chapterId}: ${oldLookup.cause?.message}")
                            rollbackExecutor.rollbackWholePublishTransaction(ctx.txId, ctx.items, ctx.stagedRefs, ctx.storage, ctx.currentJournal)
                            null
                        }
                    }
                }
                is MirrorLookupResult.Missing -> {
                    // backup 不存在，需要 prepareBackup
                    val prepared = ctx.storage.prepareBackup(ctx.txId, ctx.oldRef, MIME_MARKDOWN)
                    if (prepared == null) {
                        logPublishAborted(ctx.projectId, "backup prepare failed for ${ctx.key.chapterId}")
                        rollbackExecutor.rollbackWholePublishTransaction(ctx.txId, ctx.items, ctx.stagedRefs, ctx.storage, ctx.currentJournal)
                        null
                    } else {
                        ItemBackupOutcome(prepared.backupRef, prepared.vacated)
                    }
                }
                is MirrorLookupResult.Failed -> {
                    logPublishAborted(ctx.projectId, "lookupBackup failed for ${ctx.key.chapterId}: ${backupResult.cause?.message}")
                    rollbackExecutor.rollbackWholePublishTransaction(ctx.txId, ctx.items, ctx.stagedRefs, ctx.storage, ctx.currentJournal)
                    null
                }
            }
    }

    private fun promoteItemStaged(
        key: ChapterKey,
        item: PendingItem,
        staged: StagedMirrorRef,
        desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        storage: ReadableMirrorStorage,
    ): MirrorFileRef? {
            // promote 成功后 journal 可能还没写 PROMOTED，final 上可能已有新内容
            //    #649 评论 5566303837 问题 2：OLD_VACATED 崩溃窗口检查
            val existingNewRef = findExistingPromotedRef(key, item, staged, desiredEntries, storage)
            return existingNewRef ?: storage.promoteStaged(staged, staged.finalRelativePath)
    }

    private fun findExistingPromotedRef(
        key: ChapterKey,
        item: PendingItem,
        staged: StagedMirrorRef,
        desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        storage: ReadableMirrorStorage,
    ): MirrorFileRef? {
            if (item.state != PendingItem.STATE_OLD_VACATED) return null
            val finalLookup = storage.lookup(staged.finalRelativePath)
            if (finalLookup !is MirrorLookupResult.Found) return null
            // final 已存在，校验是否是本事务的新正文
            val expectedHash = desiredEntries[key]?.contentHash ?: return null
            val hashResult = storage.readTextAndHash(finalLookup.ref) ?: return null
            // final 已是本事务的新正文 → promote 已完成，直接复用
            return if (hashResult.second == expectedHash) finalLookup.ref else null
    }

    private suspend fun commitPublishManifest(
        projectId: String,
        context: PublishContext,
        stageResult: StageResult,
        promoteResult: PromoteResult,
        frozenContext: FrozenPlanContext,
    ): ManifestTransactionResult? {
            val manifestTargetJson =
                frozenPlanToManifestJson(frozenContext.frozenPlan, promoteResult.promotedEntries)
                    ?: run {
                        rollbackExecutor.rollbackWholePublishTransaction(
                            context.txId,
                            promoteResult.items,
                            stageResult.stagedRefs,
                            context.storage,
                            promoteResult.currentJournal,
                        )
                        return null
                    }
            val manifestResult =
                publishManifestWithDesiredTransactional(
                    ManifestTransactionParams(
                        projectId = projectId,
                        snapshot = null,
                        desiredEntries = promoteResult.promotedEntries,
                        txId = context.txId,
                        journalContext = promoteResult.currentJournal,
                        items = promoteResult.items,
                        storage = context.storage,
                        prebuiltTargetJson = manifestTargetJson,
                    ),
                )
            if (manifestResult == null) {
                logPublishAborted(projectId, "manifest write failed")
                // #649 评论 5564379115 问题 2：统一事务回滚
                rollbackExecutor.rollbackWholePublishTransaction(
                    context.txId,
                    promoteResult.items,
                    stageResult.stagedRefs,
                    context.storage,
                    promoteResult.currentJournal,
                )
                return null
            }
            return manifestResult
    }

    private fun persistPublishCommittedBaseline(
        projectId: String,
        promoteResult: PromoteResult,
        manifestResult: ManifestTransactionResult,
    ): PendingMirrorPublish? {
            // 标记所有 item 为 COMMITTED，更新 journal 到 cleanup 阶段
            // #649 评论 5576949398 问题 2：从 manifestResult.committedJournal.copy(phase=PHASE_CLEANUP, ...)
            // 继续推进，不再从 manifest 调用之前的旧 currentJournal 重建 cleanup journal。
            val committedItems = promoteResult.items.mapValues { it.value.copy(state = PendingItem.STATE_COMMITTED) }
            val currentJournal =
                manifestResult.committedJournal.copy(
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    newEntries = promoteResult.promotedEntries,
                    stagedRefs = emptyMap(),
                    items = committedItems,
                )
            if (!journalWriter.persistPendingJournal(currentJournal)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: cleanup journal write failed, keeping journal for retry",
                )
                return null
            }
            // journal 落盘后再更新 stateStore（cleanup 阶段幂等执行）
            // #649 评论 5563333323 缺口 2：putChapterEntries 失败也不清 journal
            if (!stateStore.putChapterEntries(promoteResult.promotedEntries)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: putChapterEntries failed, keeping journal for retry",
                )
                return null
            }
            // #649 评论 5576464076 问题 2：manifest 提交成功后写入 committed manifest。
            // #649 评论 5576949398 问题 2：用 persistCommittedBaselineFromJournal 统一写入，
            // 严格校验 manifestTargetJson/hash 自洽 + 严格解析。
            if (!journalWriter.persistCommittedBaselineFromJournal(manifestResult.committedJournal)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: persistCommittedBaseline failed, keeping journal for retry",
                )
                return null
            }
            // #649 评论 5564820566 问题 5：manifest 提交成功后标记作品已发布，
            // 让零章节作品在 cleanupStaleProjects 中也能被找到。
            if (!stateStore.addPublishedProjectId(projectId)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: addPublishedProjectId failed, keeping journal for retry",
                )
                return null
            }
            return currentJournal
    }

    private fun finalizePublishCleanup(
        projectId: String,
        context: PublishContext,
        cleanupJournal: PendingMirrorPublish,
    ): MirrorPublishResult {
            if (cleanupCommittedTransaction(cleanupJournal, context.storage, allLiveKeys = context.allKeys)) {
                // 全部清理成功，清除 journal
                stateStore.clearPendingPublish()
                return MirrorPublishResult.Committed
            } else {
                // 有失败项，保留 journal，下次 recover 继续清
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: cleanup partial failure, keeping journal for retry",
                )
                return MirrorPublishResult.RetryableFailure
            }
    }

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
            val preparation = prepareDeleteFrozenPlan(projectId, storage)
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
            val manifestTargetJson = buildDeleteManifestTargetJson(projectId, preparation.frozenPlan)
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
        if (!journalWriter.writePendingPublishJournal(PendingJournalParams(
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
            ))
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
    ): ManifestTransactionResult? {
        val manifestRelativePath = "$META_DIR/$MANIFEST_FILE_NAME"
        val ctx = ManifestTransactionContext(
            projectId = params.projectId,
            snapshot = params.snapshot,
            desiredEntries = params.desiredEntries,
            txId = params.txId,
            journalContext = params.journalContext,
            items = params.items,
            storage = params.storage,
            prebuiltTargetJson = params.prebuiltTargetJson,
            manifestRelativePath = manifestRelativePath,
        )
        // 1. 冻结 old 身份（#649 评论 5570613481 问题 3 + 5572554935 问题 3）
        val oldIdentity = resolveManifestOldIdentity(ctx) ?: return null
        // 2. 冻结 manifest 目标 + stage（#649 评论 5571899956 问题 1 + 5573750754 修复 2）
        val stageContext = prepareManifestTargetAndStage(ctx, oldIdentity) ?: return null
        // 3. backup 阶段（#649 评论 5565067997 修复 2 + 5573310799 问题 3）
        when (val backupOutcome = executeManifestBackupPhase(ctx, oldIdentity, stageContext)) {
            is ManifestBackupOutcome.Completed -> return backupOutcome.result
            is ManifestBackupOutcome.Aborted -> return null
            is ManifestBackupOutcome.Proceed -> {
                // 4. promote 阶段（#649 评论 5566303837 问题 3 + 5570613481 问题 2）
                when (val promoteOutcome = promoteManifestStaged(ctx, stageContext, backupOutcome)) {
                    is ManifestPromoteOutcome.Completed -> return promoteOutcome.result
                    is ManifestPromoteOutcome.Aborted -> return null
                    is ManifestPromoteOutcome.Proceed ->
                        // 5. setManifestUri + commit（#649 评论 5563333323 缺口 2 + 5576949398 问题 2）
                        return commitManifestTransaction(ctx, promoteOutcome, backupOutcome.backupRef)
                }
            }
        }
    }


    internal data class ManifestTransactionParams(
        val projectId: String,
        val snapshot: ProjectWorkspaceSnapshot?,
        val desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        val txId: String,
        val journalContext: PendingMirrorPublish,
        val items: Map<ChapterKey, PendingItem>,
        val storage: ReadableMirrorStorage,
        // #649 评论 5575551884 问题 3：分离"预构建目标 JSON"与"manifest 子事务已开始"。
        // manifestTargetJson != null 表示 manifest 子事务已持久化开始（恢复路径）。
        // prebuiltTargetJson 表示冻结的 manifest JSON 目标（尚未开始子事务）。
        val prebuiltTargetJson: String? = null,
    )

    private data class ManifestTransactionContext(
        val projectId: String,
        val snapshot: ProjectWorkspaceSnapshot?,
        val desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        val txId: String,
        val journalContext: PendingMirrorPublish,
        val items: Map<ChapterKey, PendingItem>,
        val storage: ReadableMirrorStorage,
        val prebuiltTargetJson: String?,
        val manifestRelativePath: String,
    )

    private data class ManifestOldIdentity(
        val oldRef: MirrorFileRef?,
        val oldContentHash: String?,
    )

    private data class ManifestStageContext(
        val newContentHash: String,
        val oldContentHash: String?,
        val staged: StagedMirrorRef?,
        val resumeState: ManifestTransactionState,
        val currentJournal: PendingMirrorPublish,
    )

    private sealed interface ManifestBackupOutcome {
        data class Completed(val result: ManifestTransactionResult) : ManifestBackupOutcome
        data class Proceed(
            val backupRef: MirrorFileRef?,
            val currentJournal: PendingMirrorPublish,
        ) : ManifestBackupOutcome
        data object Aborted : ManifestBackupOutcome
    }

    private sealed interface ManifestPromoteOutcome {
        data class Completed(val result: ManifestTransactionResult) : ManifestPromoteOutcome
        data class Proceed(
            val newRef: MirrorFileRef,
            val currentJournal: PendingMirrorPublish,
        ) : ManifestPromoteOutcome
        data object Aborted : ManifestPromoteOutcome
    }

    private fun resolveManifestOldIdentity(
        ctx: ManifestTransactionContext,
    ): ManifestOldIdentity? {
        // #649 评论 5570613481 问题 3：恢复已有 manifest 事务时沿用已有 manifestOldContentHash；
        // #649 评论 5572554935 问题 3：首次 manifest old 身份未冻结。
        // 一旦 manifestTargetJson != null，说明 manifest 事务已经开始；
        // 恢复时只能用 journalContext.manifestOldRef / manifestOldContentHash，
        // 绝对不能再从当前 stateStore.getManifestUri() 反推 old。
        val isResumingManifest = ctx.journalContext.manifestTargetJson != null
        val frozenOldRef: MirrorFileRef? =
            if (isResumingManifest) {
                ctx.journalContext.manifestOldRef
            } else {
                // 首次进入 manifest 事务：从 stateStore 读当前 old 身份，准备冻结进 journal
                val initialOldUri = stateStore.getManifestUri()
                initialOldUri?.let { MirrorFileRef(uri = it, relativePath = ctx.manifestRelativePath) }
            }
        val manifestOldContentHash =
            ctx.journalContext.manifestOldContentHash ?: run {
                frozenOldRef?.let { ctx.storage.readTextAndHash(it)?.second }
            }
        // 旧 manifest 存在但读取失败：在移动旧 manifest 之前就停止事务
        if (frozenOldRef != null && manifestOldContentHash == null) {
            DiagnosticsLogger.w(
                TAG,
                "Manifest transaction: old manifest exists but readTextAndHash failed, stopping before vacate",
            )
            return null
        }
        return ManifestOldIdentity(frozenOldRef, manifestOldContentHash)
    }

    private suspend fun prepareManifestTargetAndStage(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
    ): ManifestStageContext? {
        // #649 评论 5573750754 修复 2：入口先把调用方当前状态合进去，
        // 后续所有 manifest 状态推进只允许 currentJournal = currentJournal.copy(...) + journalWriter.persistPendingJournal(currentJournal)，
        // 不再从原始 journalContext 重建，避免把已 PROMOTED 正文 item 倒退回旧状态。
        val currentJournal = ctx.journalContext.copy(items = ctx.items, newEntries = ctx.desiredEntries)
        if (ctx.journalContext.manifestTargetJson != null) {
            // 恢复路径：manifest 子事务已持久化开始，使用冻结的 manifest JSON，跳过重新 stage
            // 恢复时 staged 可能已存在（MANIFEST_STAGED/MANIFEST_BACKUP_READY/MANIFEST_OLD_VACATED），
            // 也可能已 promote（MANIFEST_PROMOTED/MANIFEST_COMMITTED）
            return ManifestStageContext(
                newContentHash = computeContentHash(ctx.journalContext.manifestTargetJson),
                oldContentHash = oldIdentity.oldContentHash,
                staged = ctx.journalContext.manifestStagedRef,
                resumeState = ctx.journalContext.manifestSwapState,
                currentJournal = currentJournal,
            )
        } else if (ctx.prebuiltTargetJson != null) {
            return stagePrebuiltManifest(ctx, oldIdentity, currentJournal)
        } else {
            return buildAndStageManifest(ctx, oldIdentity, currentJournal)
        }
    }

    private fun stagePrebuiltManifest(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        currentJournal: PendingMirrorPublish,
    ): ManifestStageContext? {
        // #649 评论 5575551884 问题 3：预构建目标 JSON，但 manifest 子事务尚未开始。
        // 使用冻结产物作为目标，正常走 stage → journal 流程，不伪造 isResumingManifest。
        val json = ctx.prebuiltTargetJson!!
        val manifestNewContentHash = computeContentHash(json)
        val staged =
            ctx.storage.stageText(
                txId = ctx.txId,
                relativePath = ctx.manifestRelativePath,
                mimeType = MIME_JSON,
                text = json,
            ) ?: return null
        val updatedJournal =
            currentJournal.copy(
                manifestOldRef = oldIdentity.oldRef,
                manifestStagedRef = staged,
                manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                manifestNewContentHash = manifestNewContentHash,
                manifestOldContentHash = oldIdentity.oldContentHash,
                manifestTargetJson = json,
            )
        if (!journalWriter.persistPendingJournal(updatedJournal)) {
            ctx.storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
            return null
        }
        return ManifestStageContext(manifestNewContentHash, oldIdentity.oldContentHash, staged, ManifestTransactionState.MANIFEST_STAGED, updatedJournal)
    }

    private fun buildAndStageManifest(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        currentJournal: PendingMirrorPublish,
    ): ManifestStageContext? {
        // 首次进入 manifest 事务：构建并 stage，写 journal 保存 manifestTargetJson
        val json = planner.buildManifestJsonForDesired(ctx.projectId, ctx.snapshot, ctx.desiredEntries) ?: return null
        val manifestNewContentHash = computeContentHash(json)
        val staged =
            ctx.storage.stageText(
                txId = ctx.txId,
                relativePath = ctx.manifestRelativePath,
                mimeType = MIME_JSON,
                text = json,
            ) ?: return null
        // 写 journal：保存 manifestTargetJson，状态为 STAGED
        // #649 评论 5572554935 问题 3：首次事务必须把 old 身份冻结进 journal。
        // #649 评论 5573750754 修复 2：从 currentJournal（已合并 items/newEntries）前进，
        // 不再从原始 journalContext 重建，避免丢失调用方传入的最新 items/newEntries。
        val updatedJournal =
            currentJournal.copy(
                manifestOldRef = oldIdentity.oldRef,
                manifestStagedRef = staged,
                manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                manifestNewContentHash = manifestNewContentHash,
                manifestOldContentHash = oldIdentity.oldContentHash,
                manifestTargetJson = json,
            )
        if (!journalWriter.persistPendingJournal(updatedJournal)) {
            ctx.storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
            return null
        }
        return ManifestStageContext(manifestNewContentHash, oldIdentity.oldContentHash, staged, ManifestTransactionState.MANIFEST_STAGED, updatedJournal)
    }

    private fun deleteStagedIfExists(
        storage: ReadableMirrorStorage,
        staged: StagedMirrorRef?,
    ) {
        if (staged != null) {
            storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
        }
    }

    private fun executeManifestBackupPhase(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        stageContext: ManifestStageContext,
    ): ManifestBackupOutcome {
        // #649 评论 5565067997 修复 2：根据 journalContext.manifestSwapState 决定从哪一步继续。
        // 不再用 "backup + final 同时存在" 猜测，而是看 journal 记录的显式状态。
        // 恢复路径严格按当前状态前进，不回退到 STAGED。
        // #649 评论 5573310799 问题 3：lookupBackup 完整三态 when，不把 Missing 和 Failed 揉在一起。
        when (val backupResult = ctx.storage.lookupBackup(ctx.txId, ctx.manifestRelativePath)) {
            is MirrorLookupResult.Found -> {
                // backup 已存在。根据 journal 的 manifestSwapState 决定下一步。
                val manifestBackupRef = backupResult.ref
                return when (stageContext.resumeState) {
                    ManifestTransactionState.MANIFEST_COMMITTED ->
                        handleBackupFoundCommitted(ctx, stageContext, manifestBackupRef)
                    ManifestTransactionState.MANIFEST_PROMOTED ->
                        handleBackupFoundPromoted(ctx, stageContext, manifestBackupRef)
                    ManifestTransactionState.MANIFEST_OLD_VACATED ->
                        handleBackupFoundOldVacated(ctx, stageContext, manifestBackupRef)
                    else ->
                        // MANIFEST_STAGED 或 MANIFEST_BACKUP_READY：backup 就绪，需 vacate old
                        handleBackupFoundNeedsVacate(ctx, oldIdentity, stageContext, manifestBackupRef)
                }
            }
            is MirrorLookupResult.Missing -> {
                return handleBackupMissing(ctx, oldIdentity, stageContext)
            }
            is MirrorLookupResult.Failed -> {
                // SAF 权限异常、MediaStore query 失败时不再重新 prepareBackup（状态不明还继续改文件）。
                DiagnosticsLogger.w(TAG, "manifest backup lookup failed: ${backupResult.cause?.message}")
                deleteStagedIfExists(ctx.storage, stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
        }
    }

    private fun handleBackupFoundCommitted(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        // 已提交，final 就是 new manifest。用 lookup() 三态查询。
        val existingFinal = ctx.storage.lookup(ctx.manifestRelativePath)
        when (existingFinal) {
            is MirrorLookupResult.Found -> {
                // #649 评论 5573750754 修复 5：必须校验 hash == manifestNewContentHash。
                // lookup final = Found + readTextAndHash 成功 + hash == manifestNewContentHash
                // 三者全部成立才能返回 committed。hash 不匹配或读取失败就保留 journal。
                val committedHashResult = ctx.storage.readTextAndHash(existingFinal.ref)
                if (committedHashResult == null) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: COMMITTED but readTextAndHash failed, keeping journal")
                    return ManifestBackupOutcome.Aborted
                }
                if (committedHashResult.second != stageContext.newContentHash) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: COMMITTED but final hash mismatch, keeping journal")
                    return ManifestBackupOutcome.Aborted
                }
                // #649 评论 5576949398 问题 2：返回 committedJournal（currentJournal）
                val committedJournal = stageContext.currentJournal.copy(
                    manifestNewRef = existingFinal.ref,
                    manifestBackupRef = manifestBackupRef,
                    isManifestCommitted = true,
                    manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                    manifestNewContentHash = stageContext.newContentHash,
                    manifestOldContentHash = stageContext.oldContentHash,
                )
                return ManifestBackupOutcome.Completed(
                    ManifestTransactionResult(committedJournal, existingFinal.ref),
                )
            }
            is MirrorLookupResult.Missing -> {
                DiagnosticsLogger.w(TAG, "Manifest transaction: COMMITTED but final missing, rolling back")
                deleteStagedIfExists(ctx.storage, stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Manifest transaction: lookup final failed (COMMITTED): ${existingFinal.cause?.message}",
                )
                deleteStagedIfExists(ctx.storage, stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
        }
    }

    private fun handleBackupFoundPromoted(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        // 已 promote，继续 setManifestUri。用 lookup() 三态查询。
        val existingFinal = ctx.storage.lookup(ctx.manifestRelativePath)
        when (existingFinal) {
            is MirrorLookupResult.Found -> {
                // #649 评论 5573750754 修复 5：必须校验 hash == manifestNewContentHash。
                val promotedHashResult = ctx.storage.readTextAndHash(existingFinal.ref)
                if (promotedHashResult == null) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: PROMOTED but readTextAndHash failed, keeping journal")
                    return ManifestBackupOutcome.Aborted
                }
                if (promotedHashResult.second != stageContext.newContentHash) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: PROMOTED but final hash mismatch, keeping journal")
                    return ManifestBackupOutcome.Aborted
                }
                if (!stateStore.setManifestUri(existingFinal.ref.uri)) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed (resume PROMOTED)")
                    return ManifestBackupOutcome.Aborted
                }
                val updatedJournal = stageContext.currentJournal.copy(
                    manifestNewRef = existingFinal.ref,
                    manifestBackupRef = manifestBackupRef,
                    isManifestCommitted = true,
                    manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                )
                if (!journalWriter.persistPendingJournal(updatedJournal)) {
                    return ManifestBackupOutcome.Aborted
                }
                // #649 评论 5576949398 问题 2：返回 committedJournal（currentJournal）
                return ManifestBackupOutcome.Completed(
                    ManifestTransactionResult(updatedJournal, existingFinal.ref),
                )
            }
            is MirrorLookupResult.Missing -> {
                DiagnosticsLogger.w(TAG, "Manifest transaction: PROMOTED but final missing")
                deleteStagedIfExists(ctx.storage, stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Manifest transaction: lookup final failed (PROMOTED): ${existingFinal.cause?.message}",
                )
                deleteStagedIfExists(ctx.storage, stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
        }
    }

    private fun handleBackupFoundOldVacated(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        // old 已腾空，继续 promote
        val updatedJournal = stageContext.currentJournal.copy(
            manifestBackupRef = manifestBackupRef,
            manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED,
        )
        if (!journalWriter.persistPendingJournal(updatedJournal)) {
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Proceed(manifestBackupRef, updatedJournal)
    }

    private fun handleBackupFoundNeedsVacate(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        // MANIFEST_STAGED 或 MANIFEST_BACKUP_READY：backup 就绪，需 vacate old
        // #649 评论 5565067997 修复 5：用 lookup() 三态查询判断 old 是否已 vacate
        val oldRef = oldIdentity.oldRef
        if (oldRef != null) {
            val oldLookup = ctx.storage.lookup(oldRef.relativePath)
            when (oldLookup) {
                is MirrorLookupResult.Found -> {
                    // old 还在 final，需 vacate
                    if (!ctx.storage.vacateCommitted(oldRef)) {
                        DiagnosticsLogger.w(TAG, "Manifest transaction: vacate failed (resume BACKUP_READY)")
                        deleteStagedIfExists(ctx.storage, stageContext.staged)
                        return ManifestBackupOutcome.Aborted
                    }
                }
                is MirrorLookupResult.Missing -> {
                    // old 已腾空，无需再 vacate
                }
                is MirrorLookupResult.Failed -> {
                    // 查询失败，不能继续
                    DiagnosticsLogger.w(
                        TAG,
                        "Manifest transaction: lookup old failed (resume BACKUP_READY): ${oldLookup.cause?.message}",
                    )
                    deleteStagedIfExists(ctx.storage, stageContext.staged)
                    return ManifestBackupOutcome.Aborted
                }
            }
        }
        // 写 journal：MANIFEST_OLD_VACATED
        val updatedJournal = stageContext.currentJournal.copy(
            manifestBackupRef = manifestBackupRef,
            manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED,
        )
        if (!journalWriter.persistPendingJournal(updatedJournal)) {
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Proceed(manifestBackupRef, updatedJournal)
    }

    private fun handleBackupMissing(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        stageContext: ManifestStageContext,
    ): ManifestBackupOutcome {
        val oldRef = oldIdentity.oldRef
        if (oldRef == null) {
            // oldRef == null 时什么都不做（首次发布无旧 manifest），直接进入后面的 promote
            return ManifestBackupOutcome.Proceed(null, stageContext.currentJournal)
        }
        // #649 评论 5564820566 问题 3：manifest 也用两步 prepareBackup + vacateCommitted
        val prepared = ctx.storage.prepareBackup(ctx.txId, oldRef, MIME_JSON)
        if (prepared == null) {
            // backup 失败：删 manifest staging，不动旧 manifest
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        val manifestBackupRef = prepared.backupRef
        // 写 journal：MANIFEST_BACKUP_READY
        var currentJournal = stageContext.currentJournal.copy(
            manifestBackupRef = manifestBackupRef,
            manifestSwapState = ManifestTransactionState.MANIFEST_BACKUP_READY,
        )
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        // vacate old（如果 prepareBackup 还没 move old）
        if (!prepared.vacated && !ctx.storage.vacateCommitted(oldRef)) {
            // vacate 失败：删 staging 和 backup
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        // 写 journal：MANIFEST_OLD_VACATED
        currentJournal = currentJournal.copy(manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED)
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            // journal 失败：恢复 backup 到最终位置，删 staging
            ctx.storage.restoreBackup(prepared.backupRef, ctx.manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Proceed(manifestBackupRef, currentJournal)
    }

    private fun promoteManifestStaged(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        backupOutcome: ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        val finalLookup = ctx.storage.lookup(ctx.manifestRelativePath)
        if (finalLookup is MirrorLookupResult.Found) {
            return handlePromoteFinalFound(ctx, backupOutcome, finalLookup.ref)
        }
        if (finalLookup is MirrorLookupResult.Failed) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: lookup final failed: ${finalLookup.cause?.message}, keeping journal")
            return ManifestPromoteOutcome.Aborted
        }
        return handlePromoteFinalMissing(ctx, stageContext.staged, backupOutcome)
    }

    private fun handlePromoteFinalFound(
        ctx: ManifestTransactionContext,
        backupOutcome: ManifestBackupOutcome.Proceed,
        finalRef: MirrorFileRef,
    ): ManifestPromoteOutcome {
        // final 已存在，需要校验身份
        var currentJournal = backupOutcome.currentJournal
        val desiredNewHash = currentJournal.manifestNewContentHash
        if (desiredNewHash == null) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: final exists but no expected hash, keeping journal")
            return ManifestPromoteOutcome.Aborted
        }
        val hashResult = ctx.storage.readTextAndHash(finalRef)
        if (hashResult == null) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: readTextAndHash failed, cannot verify final identity, keeping journal")
            return ManifestPromoteOutcome.Aborted
        }
        if (hashResult.second != desiredNewHash) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: final hash mismatch, state unknown, keeping journal")
            return ManifestPromoteOutcome.Aborted
        }
        // final 已是新 manifest → promote 已完成，只需 setManifestUri
        if (!stateStore.setManifestUri(finalRef.uri)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed (final already new manifest)")
            return ManifestPromoteOutcome.Aborted
        }
        currentJournal = currentJournal.copy(
            manifestNewRef = finalRef,
            isManifestCommitted = true,
            manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
        )
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            return ManifestPromoteOutcome.Aborted
        }
        // #649 评论 5576949398 问题 2：返回 committedJournal（currentJournal）
        return ManifestPromoteOutcome.Completed(ManifestTransactionResult(currentJournal, finalRef))
    }

    private fun handlePromoteFinalMissing(
        ctx: ManifestTransactionContext,
        staged: StagedMirrorRef?,
        backupOutcome: ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        // final 不存在，正常 promote
        val storage = ctx.storage
        var currentJournal = backupOutcome.currentJournal
        val newRef = if (staged != null) storage.promoteStaged(staged, ctx.manifestRelativePath) else null
        if (newRef == null) {
            // promote 失败：恢复 backup（如果有），删 staging
            backupOutcome.backupRef?.let {
                storage.restoreBackup(it, ctx.manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            }
            deleteStagedIfExists(storage, staged)
            return ManifestPromoteOutcome.Aborted
        }
        // 写 journal：MANIFEST_PROMOTED
        currentJournal = currentJournal.copy(manifestNewRef = newRef, manifestSwapState = ManifestTransactionState.MANIFEST_PROMOTED)
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            // journal 写失败：删新 manifest，恢复 backup（如果有）
            storage.delete(newRef)
            backupOutcome.backupRef?.let {
                storage.restoreBackup(it, ctx.manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            }
            return ManifestPromoteOutcome.Aborted
        }
        return ManifestPromoteOutcome.Proceed(newRef, currentJournal)
    }

    private fun commitManifestTransaction(
        ctx: ManifestTransactionContext,
        promoteOutcome: ManifestPromoteOutcome.Proceed,
        manifestBackupRef: MirrorFileRef?,
    ): ManifestTransactionResult? {
        val newRef = promoteOutcome.newRef
        var currentJournal = promoteOutcome.currentJournal
        // 4. setManifestUri
        //    #649 评论 5563333323 缺口 2：setManifestUri 返回 Boolean
        if (!stateStore.setManifestUri(newRef.uri)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed")
            ctx.storage.delete(newRef)
            manifestBackupRef?.let {
                ctx.storage.restoreBackup(it, ctx.manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            }
            return null
        }
        // 写 journal：MANIFEST_COMMITTED
        currentJournal = currentJournal.copy(isManifestCommitted = true, manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED)
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: MANIFEST_COMMITTED journal write failed")
        }
        // #649 评论 5576949398 问题 2：返回 committedJournal（currentJournal）
        return ManifestTransactionResult(currentJournal, newRef)
    }

    private fun projectIdKeepingJournal(projectId: String): String = "$projectId, keeping journal"

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val META_DIR = "_meta"
        private const val MANIFEST_FILE_NAME = "manifest.json"
        private const val MIME_MARKDOWN = "text/markdown"
        private const val MIME_JSON = "application/json"
        private const val MIN_ID_LENGTH = 8
        private const val SKIP_NOT_SUPPORTED = "Mirror publish skipped: storage not supported"
        private const val KEEPING_JOURNAL_NOT_PROMOTING = "keeping journal, not promoting"
        private const val ROLLBACK_MANIFEST_JOURNAL_FAILED = "rollback manifest: journal write failed after "
        private const val OLD_RESTORED = "OLD_RESTORED"
    }
}
