package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot

/**
 * 从 MirrorPublishExecutor 提取，只负责单项目发布流程。
 *
 * 包括：准备、stage、frozen plan、promote、manifest commit 和 cleanup 阶段。
 */
internal class MirrorPublishProjectExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val planner: MirrorPublishPlanner,
    private val rollbackExecutor: MirrorRollbackExecutor,
    private val manifestExecutor: MirrorManifestTransactionExecutor,
    private val source: MirrorSnapshotSource,
    private val router: MirrorStorageRouter,
    private val callbacks: MirrorPublishExecutorCallbacks,
) {
    private val cleanupTransactionExecutor = MirrorCleanupTransactionExecutor(stateStore)
    private val ensurePendingRecovered get() = callbacks.ensurePendingRecovered
    private val logNotLoaded get() = callbacks.logNotLoaded
    private val logPublishAborted get() = callbacks.logPublishAborted

    internal suspend fun publishProject(projectId: String): MirrorPublishResult {
        try {
            return executePublishProject(projectId)
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish project: ${e.message}", e)
            return MirrorPublishResult.RetryableFailure
        }
    }

    private suspend fun executePublishProject(projectId: String): MirrorPublishResult {
            if (!ensurePendingRecovered()) {
                return MirrorPublishResult.PendingRecovery
            }
            val context = preparePublishContext(projectId)
                ?: return MirrorPublishResult.RetryableFailure
            if (!writePublishStageJournal(projectId, context)) {
                return MirrorPublishResult.RetryableFailure
            }
            val stageResult = stageAllContent(projectId, context)
                ?: return MirrorPublishResult.RetryableFailure
            val frozenContext = buildPublishFrozenPlanAndJournal(projectId, context, stageResult)
                ?: return MirrorPublishResult.RetryableFailure
            val promoteResult = promoteAllStaged(projectId, context, stageResult, frozenContext)
                ?: return MirrorPublishResult.RetryableFailure
            val manifestResult = commitPublishManifest(projectId, context, stageResult, promoteResult, frozenContext)
                ?: return MirrorPublishResult.RetryableFailure
            val cleanupJournal = persistPublishCommittedBaseline(projectId, promoteResult, manifestResult)
                ?: return MirrorPublishResult.RetryableFailure
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
            val txContextResult = router.currentTransactionResult()
            if (txContextResult.isFailure) {
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
            val committedManifestResolution = resolveCommittedManifestForPublish(context.storage)
            if (committedManifestResolution is CommittedManifestResolution.Stop) {
                logPublishAborted(projectId, "committed manifest corrupted or migration failed")
                return null
            }
            val committedManifest =
                when (committedManifestResolution) {
                    is CommittedManifestResolution.Baseline -> committedManifestResolution.manifest
                    CommittedManifestResolution.FirstPublish -> null
                    CommittedManifestResolution.Stop -> null
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
                if (oldRef != null && item.state != PendingItem.STATE_OLD_VACATED) {
                    val backupCtx = ItemBackupContext(projectId, key, txId, oldRef, items, stagedRefs, storage, currentJournal)
                    currentJournal = backupAndVacateItem(backupCtx, item) ?: return null
                }
                val newRef = promoteItemStaged(key, item, staged, desiredEntries, storage)
                if (newRef == null) {
                    logPublishAborted(projectId, "promote failed for ${key.chapterId}")
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
                items[key] = items[key]!!.copy(promotedRef = newRef, state = PendingItem.STATE_PROMOTED)
                currentJournal = currentJournal.copy(items = items)
                if (!journalWriter.persistPendingJournal(currentJournal)) {
                    logPublishAborted(projectId, "journal write failed after promote for ${key.chapterId}")
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
            ctx.items[ctx.key] = item.copy(backupOldRef = backupOutcome.backupRef, state = PendingItem.STATE_BACKUP_READY)
            var currentJournal = ctx.currentJournal.copy(items = ctx.items)
            if (!journalWriter.persistPendingJournal(currentJournal)) {
                logPublishAborted(ctx.projectId, "journal write failed after backup prepare for ${ctx.key.chapterId}")
                rollbackExecutor.rollbackWholePublishTransaction(ctx.txId, ctx.items, ctx.stagedRefs, ctx.storage, currentJournal)
                return null
            }
            if (!backupOutcome.vacated && !ctx.storage.vacateCommitted(ctx.oldRef)) {
                logPublishAborted(ctx.projectId, "vacate failed for ${ctx.key.chapterId}")
                rollbackExecutor.rollbackWholePublishTransaction(ctx.txId, ctx.items, ctx.stagedRefs, ctx.storage, currentJournal)
                return null
            }
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
            val backupResult = ctx.storage.lookupBackup(ctx.txId, ctx.oldRef.relativePath)
            return when (backupResult) {
                is MirrorLookupResult.Found -> {
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
            val expectedHash = desiredEntries[key]?.contentHash ?: return null
            val hashResult = storage.readTextAndHash(finalLookup.ref) ?: return null
            return if (hashResult.second == expectedHash) finalLookup.ref else null
    }

    private suspend fun commitPublishManifest(
        projectId: String,
        context: PublishContext,
        stageResult: StageResult,
        promoteResult: PromoteResult,
        frozenContext: FrozenPlanContext,
    ): MirrorManifestTransactionExecutor.ManifestTransactionResult? {
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
                manifestExecutor.publishManifestWithDesiredTransactional(
                    MirrorManifestTransactionExecutor.ManifestTransactionParams(
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
        manifestResult: MirrorManifestTransactionExecutor.ManifestTransactionResult,
    ): PendingMirrorPublish? {
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
            if (!stateStore.putChapterEntries(promoteResult.promotedEntries)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: putChapterEntries failed, keeping journal for retry",
                )
                return null
            }
            if (!journalWriter.persistCommittedBaselineFromJournal(manifestResult.committedJournal)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: persistCommittedBaseline failed, keeping journal for retry",
                )
                return null
            }
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
            if (cleanupTransactionExecutor.cleanupCommittedTransaction(cleanupJournal, context.storage, allLiveKeys = context.allKeys)) {
                stateStore.clearPendingPublish()
                return MirrorPublishResult.Committed
            } else {
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: cleanup partial failure, keeping journal for retry",
                )
                return MirrorPublishResult.RetryableFailure
            }
    }

    private fun resolveCommittedManifestForPublish(storage: ReadableMirrorStorage): MirrorPublishExecutor.CommittedManifestResolution {
        return when (val result = stateStore.getCommittedManifestStrict()) {
            is CommittedManifestReadResult.NotExists -> MirrorPublishExecutor.CommittedManifestResolution.FirstPublish
            is CommittedManifestReadResult.Found -> MirrorPublishExecutor.CommittedManifestResolution.Baseline(result.manifest)
            is CommittedManifestReadResult.Corrupted -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Committed manifest corrupted, stopping publish: ${result.cause.message}",
                )
                MirrorPublishExecutor.CommittedManifestResolution.Stop
            }
            is CommittedManifestReadResult.NeedsMigration -> {
                val migration = ReadableMirrorStateMigration(stateStore, storage)
                when (migration.migrate()) {
                    ReadableMirrorStateMigration.Result.SUCCESS -> {
                        when (val reread = stateStore.getCommittedManifestStrict()) {
                            is CommittedManifestReadResult.Found ->
                                MirrorPublishExecutor.CommittedManifestResolution.Baseline(reread.manifest)
                            is CommittedManifestReadResult.NotExists ->
                                MirrorPublishExecutor.CommittedManifestResolution.FirstPublish
                            is CommittedManifestReadResult.Corrupted -> {
                                DiagnosticsLogger.w(
                                    TAG,
                                    "Committed manifest still corrupted after migration: ${reread.cause.message}",
                                )
                                MirrorPublishExecutor.CommittedManifestResolution.Stop
                            }
                            is CommittedManifestReadResult.NeedsMigration -> {
                                DiagnosticsLogger.w(
                                    TAG,
                                    "State still needs migration after migration attempt, stopping",
                                )
                                MirrorPublishExecutor.CommittedManifestResolution.Stop
                            }
                        }
                    }
                    ReadableMirrorStateMigration.Result.FAILURE -> {
                        DiagnosticsLogger.w(TAG, "State migration failed, stopping publish")
                        MirrorPublishExecutor.CommittedManifestResolution.Stop
                    }
                }
            }
        }
    }

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val MIME_MARKDOWN = "text/markdown"
        private const val SKIP_NOT_SUPPORTED = "Mirror publish skipped: storage not supported"
    }
}
