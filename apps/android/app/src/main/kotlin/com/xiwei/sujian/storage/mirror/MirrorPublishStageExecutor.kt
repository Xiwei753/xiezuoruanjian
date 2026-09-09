package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot

/**
 * 从 [MirrorPublishProjectExecutor] 提取，只负责发布流程的准备与 stage 阶段。
 *
 * 包括：preparePublishContext、writePublishStageJournal、stageAllContent、
 * buildPublishFrozenPlanAndJournal，以及只属于准备阶段的类型。
 */
internal class MirrorPublishStageExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val planner: MirrorPublishPlanner,
    private val journalWriter: MirrorJournalWriter,
    private val source: MirrorSnapshotSource,
    private val router: MirrorStorageRouter,
) {
    internal data class PublishContext(
        val txContext: MirrorStorageTransactionContext,
        val storage: ReadableMirrorStorage,
        val snapshot: ProjectWorkspaceSnapshot,
        val oldEntries: Map<ChapterKey, ChapterMirrorEntry>,
        val allKeys: Set<ChapterKey>,
        val writePlan: List<WritePlanEntry>,
        val txId: String,
    )

    internal data class StageResult(
        val stagedRefs: Map<ChapterKey, StagedMirrorRef>,
        val desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        val items: Map<ChapterKey, PendingItem>,
    )

    internal data class FrozenPlanContext(
        val frozenPlan: FrozenManifestPlan,
        val currentJournal: PendingMirrorPublish,
    )

    internal suspend fun preparePublishContext(
        projectId: String,
        logNotLoaded: (BridgeResult<*>, String) -> Unit,
        logPublishAborted: (String, String) -> Unit,
    ): PublishContext? {
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

    internal fun writePublishStageJournal(
        projectId: String,
        context: PublishContext,
        logPublishAborted: (String, String) -> Unit,
    ): Boolean {
        if (!journalWriter.writePendingPublishJournal(
                PendingJournalParams(
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
                ),
            )
        ) {
            logPublishAborted(projectId, "PHASE_STAGE journal write failed")
            return false
        }
        return true
    }

    internal fun stageAllContent(
        projectId: String,
        context: PublishContext,
        logPublishAborted: (String, String) -> Unit,
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

    internal fun buildPublishFrozenPlanAndJournal(
        projectId: String,
        context: PublishContext,
        stageResult: StageResult,
        logPublishAborted: (String, String) -> Unit,
    ): FrozenPlanContext? {
        val committedManifestResolution = resolveCommittedManifestForPublish(context.storage)
        if (committedManifestResolution is MirrorPublishExecutor.CommittedManifestResolution.Stop) {
            logPublishAborted(projectId, "committed manifest corrupted or migration failed")
            return null
        }
        val committedManifest =
            when (committedManifestResolution) {
                is MirrorPublishExecutor.CommittedManifestResolution.Baseline -> committedManifestResolution.manifest
                MirrorPublishExecutor.CommittedManifestResolution.FirstPublish -> null
                MirrorPublishExecutor.CommittedManifestResolution.Stop -> null
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

    private fun resolveCommittedManifestForPublish(
        storage: ReadableMirrorStorage,
    ): MirrorPublishExecutor.CommittedManifestResolution {
        return when (val result = stateStore.getCommittedManifestStrict()) {
            is CommittedManifestReadResult.NotExists ->
                MirrorPublishExecutor.CommittedManifestResolution.FirstPublish
            is CommittedManifestReadResult.Found ->
                MirrorPublishExecutor.CommittedManifestResolution.Baseline(result.manifest)
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

    private companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val MIME_MARKDOWN = "text/markdown"
        private const val SKIP_NOT_SUPPORTED = "Mirror publish skipped: storage not supported"
    }
}
