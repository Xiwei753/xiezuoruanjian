package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.interop.diagnostics.DiagnosticsInterop

/**
 * 从 MirrorRecoveryExecutor 提取，只负责 promote 阶段恢复逻辑。
 *
 * Issue #667：事务中间文件（staging、backup）在 [MirrorTransactionWorkspace]（私有目录）中，
 * 恢复时用 workspace 方法操作事务文件，用 [ReadableMirrorStorage] 只做最终文件操作。
 */
internal class MirrorPromoteRecoveryExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val rollbackExecutor: MirrorRollbackExecutor,
    private val publishExecutor: MirrorPublishExecutor,
    private val workspace: MirrorTransactionWorkspace,
) {
    internal sealed interface PromoteItemResult {
        data class Promoted(val entry: ChapterMirrorEntry) : PromoteItemResult

        data object Reused : PromoteItemResult

        data object RollbackDone : PromoteItemResult
    }

    private data class RecoverPromoteItemContext(
        val journal: PendingMirrorPublish,
        val key: ChapterKey,
        val item: PendingItem,
        val storage: ReadableMirrorStorage,
        val currentItems: MutableMap<ChapterKey, PendingItem>,
        val promotedEntries: MutableMap<ChapterKey, ChapterMirrorEntry>,
    )

    internal suspend fun recoverPromotePhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        onCleanupReady: suspend (PendingMirrorPublish, ReadableMirrorStorage) -> Unit = { _, _ -> },
    ) {
        val promotedEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        val currentItems = journal.items.toMutableMap()
        for ((key, item) in currentItems.toMap()) {
            val outcome = processItemForRecoverPromote(journal, key, item, storage, currentItems, promotedEntries)
            when (outcome) {
                is PromoteItemResult.Promoted -> promotedEntries[key] = outcome.entry
                PromoteItemResult.Reused -> Unit
                PromoteItemResult.RollbackDone -> return
            }
        }
        finalizeRecoveryPromoteWithManifest(journal, storage, currentItems, promotedEntries, onCleanupReady)
    }

    private suspend fun processItemForRecoverPromote(
        journal: PendingMirrorPublish,
        key: ChapterKey,
        item: PendingItem,
        storage: ReadableMirrorStorage,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        promotedEntries: MutableMap<ChapterKey, ChapterMirrorEntry>,
    ): PromoteItemResult {
        val ctx = RecoverPromoteItemContext(journal, key, item, storage, currentItems, promotedEntries)
        if ((item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
            item.promotedRef != null
        ) {
            val reuseOutcome = reusePromotedItemForRecover(ctx)
            if (reuseOutcome != null) return reuseOutcome
        }
        val staged = item.stagedRef ?: journal.stagedRefs[key]
        if (staged == null) {
            DiagnosticsInterop.w(
                TAG,
                "Recover promote: missing stagedRef for ${key.chapterId}, " +
                    "transaction state incomplete, keeping journal and stopping",
            )
            rollbackRecoveryWithCleanup(journal, currentItems, promotedEntries, storage)
            return PromoteItemResult.RollbackDone
        }
        val oldRef = item.oldRef
        if (oldRef != null && item.state != PendingItem.STATE_OLD_VACATED &&
            !prepareBackupForRecoverItem(ctx, oldRef)
        ) {
            return PromoteItemResult.RollbackDone
        }
        return promoteStagedForRecoverItem(ctx, staged)
    }

    private suspend fun reusePromotedItemForRecover(ctx: RecoverPromoteItemContext): PromoteItemResult? {
        val journal = ctx.journal
        val key = ctx.key
        val promotedLookup = ctx.storage.lookup(ctx.item.promotedRef!!.relativePath)
        return when (promotedLookup) {
            is MirrorLookupResult.Found -> {
                val expectedHash = journal.newEntries[key]?.contentHash
                if (expectedHash != null) {
                    val hashResult = ctx.storage.readTextAndHash(promotedLookup.ref)
                    if (hashResult != null && hashResult.second == expectedHash) {
                        return PromoteItemResult.Promoted(
                            ChapterMirrorEntry(
                                uri = promotedLookup.ref.uri,
                                relativePath = promotedLookup.ref.relativePath,
                                revision = journal.newEntries[key]?.revision ?: 0L,
                                contentHash = expectedHash,
                            ),
                        )
                    }
                }
                DiagnosticsInterop.w(
                    TAG,
                    "Recover promote: promotedRef hash mismatch for ${key.chapterId}, keeping journal",
                )
                rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
                PromoteItemResult.RollbackDone
            }
            is MirrorLookupResult.Missing -> {
                DiagnosticsInterop.w(
                    TAG,
                    "Recover promote: promotedRef missing for ${key.chapterId}, keeping journal",
                )
                rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
                PromoteItemResult.RollbackDone
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsInterop.w(
                    TAG,
                    "Recover promote: lookup promotedRef failed for ${key.chapterId}: " +
                        "${promotedLookup.cause?.message}, keeping journal",
                )
                rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
                PromoteItemResult.RollbackDone
            }
        }
    }

    /**
     * Issue #667：使用 [MirrorTransactionWorkspace] 进行 backup 操作。
     * 不再需要 vacateCommitted。
     */
    private suspend fun prepareBackupForRecoverItem(
        ctx: RecoverPromoteItemContext,
        oldRef: MirrorFileRef,
    ): Boolean {
        val journal = ctx.journal
        val key = ctx.key
        val backupResult = workspace.lookupBackup(journal.txId, oldRef.relativePath)
        val backupRef: MirrorFileRef? =
            when (backupResult) {
                is MirrorLookupResult.Found -> backupResult.ref
                is MirrorLookupResult.Missing -> {
                    // Issue #667：读取旧内容并写入 workspace backup
                    val oldContentResult = ctx.storage.readTextAndHash(oldRef)
                    if (oldContentResult == null) {
                        DiagnosticsInterop.w(TAG, "Recover backup: read old content failed for ${key.chapterId}")
                        rollbackRecoveryWithCleanup(journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
                        return false
                    }
                    val (oldContent, _) = oldContentResult
                    val prepared = workspace.prepareBackup(journal.txId, oldRef, oldContent)
                    if (prepared == null) {
                        DiagnosticsInterop.w(TAG, "Recover backup prepare failed for ${key.chapterId}")
                        rollbackRecoveryWithCleanup(journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
                        return false
                    }
                    prepared
                }
                is MirrorLookupResult.Failed -> {
                    DiagnosticsInterop.w(
                        TAG,
                        "Recover backup: lookupBackup failed for ${key.chapterId}: ${backupResult.cause?.message}",
                    )
                    rollbackRecoveryWithCleanup(journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
                    return false
                }
            }
        ctx.currentItems[key] =
            ctx.item.copy(
                backupOldRef = backupRef,
                state = PendingItem.STATE_BACKUP_READY,
            )
        if (!writeRecoveryPromoteJournal(journal, ctx.currentItems)) {
            DiagnosticsInterop.w(
                TAG,
                "Recover backup: journal write failed (BACKUP_READY) for ${key.chapterId}, keeping journal",
            )
            rollbackRecoveryWithCleanup(journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
            return false
        }
        // Issue #667：不再需要 vacateCommitted，直接推进到 STATE_OLD_VACATED
        ctx.currentItems[key] = ctx.currentItems[key]!!.copy(state = PendingItem.STATE_OLD_VACATED)
        if (!writeRecoveryPromoteJournal(journal, ctx.currentItems)) {
            DiagnosticsInterop.w(
                TAG,
                "Recover backup: journal write failed (OLD_VACATED) for ${key.chapterId}, keeping journal",
            )
            rollbackRecoveryWithCleanup(journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
            return false
        }
        return true
    }

    /**
     * Issue #667：从 workspace 读取暂存内容，在 Download 中创建最终文件。
     *
     * 拆分为 [resolveRecoverPromotedRef] / [createFinalFromStaged] / [commitRecoveredPromote] /
     * [removeOldFinalIfPresent] 四个步骤，避免单方法同时承担检查/读取/删除/创建/写日志/回滚职责。
     */
    private suspend fun promoteStagedForRecoverItem(
        ctx: RecoverPromoteItemContext,
        staged: StagedMirrorRef,
    ): PromoteItemResult {
        val refOutcome = resolveRecoverPromotedRef(ctx, staged)
        val newRef: MirrorFileRef? =
            when (refOutcome) {
                is RecoverPromotedRefOutcome.Reuse -> refOutcome.ref
                RecoverPromotedRefOutcome.Proceed -> createFinalFromStaged(ctx, staged)
                RecoverPromotedRefOutcome.RollbackDone -> return PromoteItemResult.RollbackDone
            }
        if (newRef == null) {
            // createFinalFromStaged 失败已回滚
            return PromoteItemResult.RollbackDone
        }
        return commitRecoveredPromote(ctx, newRef)
    }

    private sealed interface RecoverPromotedRefOutcome {
        data class Reuse(val ref: MirrorFileRef) : RecoverPromotedRefOutcome

        data object Proceed : RecoverPromotedRefOutcome

        data object RollbackDone : RecoverPromotedRefOutcome
    }

    /**
     * 检查最终文件是否已存在并可复用。只有当 [PendingItem.state] 为
     * [PendingItem.STATE_OLD_VACATED] 时才需要检查 final，否则直接 [RecoverPromotedRefOutcome.Proceed]。
     */
    private suspend fun resolveRecoverPromotedRef(
        ctx: RecoverPromoteItemContext,
        staged: StagedMirrorRef,
    ): RecoverPromotedRefOutcome {
        if (ctx.item.state != PendingItem.STATE_OLD_VACATED) {
            return RecoverPromotedRefOutcome.Proceed
        }
        return when (val finalOutcome = checkFinalForPromoteStaged(ctx, staged)) {
            is FinalCheckOutcome.Reuse -> RecoverPromotedRefOutcome.Reuse(finalOutcome.ref)
            FinalCheckOutcome.Proceed -> RecoverPromotedRefOutcome.Proceed
            FinalCheckOutcome.RollbackDone -> RecoverPromotedRefOutcome.RollbackDone
        }
    }

    /**
     * 从 workspace 读取暂存内容、删除旧文件、在 Download 中创建新文件。
     * 返回 null 表示失败已回滚。
     */
    private suspend fun createFinalFromStaged(
        ctx: RecoverPromoteItemContext,
        staged: StagedMirrorRef,
    ): MirrorFileRef? {
        val journal = ctx.journal
        val key = ctx.key
        // Issue #667：从 workspace 读取暂存内容，在 Download 中创建最终文件
        val content = workspace.readStaged(staged)
        if (content == null) {
            DiagnosticsInterop.w(TAG, "Recover promote: read staged content failed for ${key.chapterId}")
            rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
            return null
        }
        // 删除旧文件（如果存在）
        val oldRef = ctx.item.oldRef
        if (oldRef != null) {
            removeOldFinalIfPresent(ctx, oldRef)
        }
        // 在 Download 中创建新文件
        val relativeDir = staged.finalRelativePath.substringBeforeLast('/', "")
        val displayName = staged.finalRelativePath.substringAfterLast('/')
        val newRef = ctx.storage.createText(relativeDir, displayName, staged.mimeType, content)
        if (newRef == null) {
            DiagnosticsInterop.w(TAG, "Recover promote failed for ${key.chapterId}")
            rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
            return null
        }
        return newRef
    }

    /**
     * 删除旧文件（如果存在）。lookup 的三种结果分别处理，
     * 删除失败只记日志不阻断后续创建。
     */
    private suspend fun removeOldFinalIfPresent(
        ctx: RecoverPromoteItemContext,
        oldRef: MirrorFileRef,
    ) {
        val key = ctx.key
        when (val oldLookup = ctx.storage.lookup(oldRef.relativePath)) {
            is MirrorLookupResult.Found -> {
                if (!ctx.storage.delete(oldLookup.ref)) {
                    DiagnosticsInterop.w(TAG, "Recover promote: delete old file failed for ${key.chapterId}")
                }
            }
            is MirrorLookupResult.Missing -> Unit
            is MirrorLookupResult.Failed -> {
                DiagnosticsInterop.w(
                    TAG,
                    "Recover promote: lookup old failed for ${key.chapterId}: ${oldLookup.cause?.message}",
                )
            }
        }
    }

    /**
     * 生成 [ChapterMirrorEntry]、更新 [RecoverPromoteItemContext.currentItems] 状态为
     * [PendingItem.STATE_PROMOTED]、写 journal。返回 [PromoteItemResult.Promoted] 或
     * [PromoteItemResult.RollbackDone]。
     */
    private suspend fun commitRecoveredPromote(
        ctx: RecoverPromoteItemContext,
        newRef: MirrorFileRef,
    ): PromoteItemResult {
        val journal = ctx.journal
        val key = ctx.key
        val entry =
            ChapterMirrorEntry(
                uri = newRef.uri,
                relativePath = newRef.relativePath,
                revision = journal.newEntries[key]?.revision ?: 0L,
                contentHash = journal.newEntries[key]?.contentHash ?: "",
            )
        ctx.currentItems[key] = ctx.currentItems[key]!!.copy(promotedRef = newRef, state = PendingItem.STATE_PROMOTED)
        if (!writeRecoveryPromoteJournal(journal, ctx.currentItems)) {
            DiagnosticsInterop.w(TAG, "Recover promote: journal write failed for ${key.chapterId}")
            rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
            return PromoteItemResult.RollbackDone
        }
        return PromoteItemResult.Promoted(entry)
    }

    private sealed interface FinalCheckOutcome {
        data class Reuse(val ref: MirrorFileRef) : FinalCheckOutcome

        data object Proceed : FinalCheckOutcome

        data object RollbackDone : FinalCheckOutcome
    }

    private suspend fun checkFinalForPromoteStaged(
        ctx: RecoverPromoteItemContext,
        staged: StagedMirrorRef,
    ): FinalCheckOutcome {
        val journal = ctx.journal
        val key = ctx.key
        val finalLookup = ctx.storage.lookup(staged.finalRelativePath)
        return when (finalLookup) {
            is MirrorLookupResult.Found -> checkFinalFoundForPromoteStaged(ctx, key, journal, finalLookup)
            is MirrorLookupResult.Missing -> FinalCheckOutcome.Proceed
            is MirrorLookupResult.Failed -> {
                DiagnosticsInterop.w(
                    TAG,
                    "Recover promote: lookup final failed for ${key.chapterId}: " +
                        "${finalLookup.cause?.message}, $KEEPING_JOURNAL_NOT_PROMOTING",
                )
                rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
                FinalCheckOutcome.RollbackDone
            }
        }
    }

    private suspend fun checkFinalFoundForPromoteStaged(
        ctx: RecoverPromoteItemContext,
        key: ChapterKey,
        journal: PendingMirrorPublish,
        finalLookup: MirrorLookupResult.Found,
    ): FinalCheckOutcome {
        val expectedHash = journal.newEntries[key]?.contentHash
        if (expectedHash != null) {
            val hashResult = ctx.storage.readTextAndHash(finalLookup.ref)
            if (hashResult != null) {
                val (_, hash) = hashResult
                if (hash == expectedHash) {
                    return FinalCheckOutcome.Reuse(finalLookup.ref)
                }
                DiagnosticsInterop.w(
                    TAG,
                    "Recover promote: final hash mismatch for ${key.chapterId}, " +
                        KEEPING_JOURNAL_NOT_PROMOTING,
                )
            } else {
                DiagnosticsInterop.w(
                    TAG,
                    "Recover promote: readTextAndHash failed for ${key.chapterId}, " +
                        KEEPING_JOURNAL_NOT_PROMOTING,
                )
            }
        } else {
            DiagnosticsInterop.w(
                TAG,
                "Recover promote: no expectedHash in journal.newEntries for ${key.chapterId}, " +
                    KEEPING_JOURNAL_NOT_PROMOTING,
            )
        }
        rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
        return FinalCheckOutcome.RollbackDone
    }

    private suspend fun finalizeRecoveryPromoteWithManifest(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
        onCleanupReady: suspend (PendingMirrorPublish, ReadableMirrorStorage) -> Unit,
    ) {
        val manifestJson =
            resolveRecoveryManifestJson(journal, storage, currentItems, promotedEntries)
                ?: return
        val manifestParams =
            ManifestTransactionParams(
                projectId = journal.projectId,
                snapshot = null,
                desiredEntries = promotedEntries,
                txId = journal.txId,
                journalContext = journal,
                items = currentItems,
                storage = storage,
                prebuiltTargetJson = manifestJson,
                // Issue #717 评论 5741567193 A 部分：recovery 用 journal 冻结的
                // manifestOldRef/manifestOldContentHash，不重新从 state 猜。
                committedManifestHash = journal.manifestOldContentHash,
                oldBaselineExists = journal.manifestOldRef != null,
            )
        val manifestResult = publishExecutor.publishManifestWithDesiredTransactional(manifestParams)
        if (manifestResult == null) {
            DiagnosticsInterop.w(TAG, "Failed to write manifest during recovery")
            rollbackRecoveryOnly(journal, currentItems, storage)
            return
        }
        writeCleanupJournalAndRecover(
            journal,
            storage,
            currentItems,
            promotedEntries,
            manifestResult.committedJournal,
            onCleanupReady,
        )
    }

    private suspend fun resolveRecoveryManifestJson(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
    ): String? {
        if (journal.manifestTargetJson != null) {
            return journal.manifestTargetJson
        }
        if (journal.frozenManifestPlan != null && journal.frozenManifestPlanHash != null) {
            return resolveManifestJsonFromFrozenPlan(journal, storage, currentItems, promotedEntries)
        }
        DiagnosticsInterop.w(TAG, "Recover promote: old journal without frozen plan, rolling back")
        rollbackRecoveryOnly(journal, currentItems, storage)
        return null
    }

    private suspend fun resolveManifestJsonFromFrozenPlan(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
    ): String? {
        val frozenPlanJson = journal.frozenManifestPlan ?: return null
        val frozenPlanHash = journal.frozenManifestPlanHash ?: return null
        if (computeContentHash(frozenPlanJson) != frozenPlanHash) {
            DiagnosticsInterop.w(TAG, "Recover promote: frozenManifestPlan hash mismatch, rolling back")
            rollbackRecoveryOnly(journal, currentItems, storage)
            return null
        }
        val plan =
            frozenManifestPlanFromJson(frozenPlanJson) ?: run {
                DiagnosticsInterop.w(TAG, "Recover promote: failed to parse frozenManifestPlan, rolling back")
                rollbackRecoveryOnly(journal, currentItems, storage)
                return null
            }
        return frozenPlanToManifestJson(plan, promotedEntries) ?: run {
            DiagnosticsInterop.w(TAG, "Recover promote: frozenPlanToManifestJson failed, rolling back")
            rollbackRecoveryOnly(journal, currentItems, storage)
            return null
        }
    }

    /**
     * 循环后：写 cleanup journal 并通过回调进入 cleanup 阶段。
     */
    internal suspend fun writeCleanupJournalAndRecover(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
        committedJournal: PendingMirrorPublish,
        onCleanupReady: suspend (PendingMirrorPublish, ReadableMirrorStorage) -> Unit = { _, _ -> },
    ) {
        val committedItems = currentItems.mapValues { it.value.copy(state = PendingItem.STATE_COMMITTED) }
        val cleanupJournal =
            committedJournal.copy(
                phase = PendingMirrorPublish.PHASE_CLEANUP,
                newEntries = promotedEntries,
                stagedRefs = emptyMap(),
                items = committedItems,
            )
        if (!journalWriter.persistPendingJournal(cleanupJournal)) {
            DiagnosticsInterop.w(TAG, "Recover promote: cleanup journal write failed, keeping journal for retry")
            return
        }
        onCleanupReady(cleanupJournal, storage)
    }

    private suspend fun rollbackRecoveryOnly(
        journal: PendingMirrorPublish,
        currentItems: Map<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ) {
        rollbackExecutor.rollbackWholePublishTransaction(
            journal.txId,
            currentItems,
            journal.stagedRefs,
            storage,
            journal,
        )
    }

    private suspend fun rollbackRecoveryWithCleanup(
        journal: PendingMirrorPublish,
        currentItems: Map<ChapterKey, PendingItem>,
        promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
        storage: ReadableMirrorStorage,
    ) {
        for ((_, entry) in promotedEntries) {
            storage.delete(MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath))
        }
        rollbackExecutor.rollbackWholePublishTransaction(
            journal.txId,
            currentItems,
            journal.stagedRefs,
            storage,
            journal,
        )
    }

    private fun writeRecoveryPromoteJournal(
        journal: PendingMirrorPublish,
        currentItems: Map<ChapterKey, PendingItem>,
    ): Boolean =
        journalWriter.writePendingPublishJournal(
            PendingJournalParams(
                projectId = journal.projectId,
                transactionType = journal.transactionType,
                phase = PendingMirrorPublish.PHASE_PROMOTE,
                txId = journal.txId,
                backend = journal.backend,
                treeUri = journal.treeUri,
                oldEntries = journal.oldEntries,
                newEntries = journal.newEntries,
                stagedRefs = journal.stagedRefs,
                items = currentItems,
                removedProjectIds = journal.removedProjectIds,
                manifestOldRef = journal.manifestOldRef,
                manifestStagedRef = journal.manifestStagedRef,
                manifestNewRef = journal.manifestNewRef,
                manifestBackupRef = journal.manifestBackupRef,
                manifestSwapState = journal.manifestSwapState,
                journalContext = journal,
            ),
        )

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val KEEPING_JOURNAL_NOT_PROMOTING = "keeping journal, not promoting"
    }
}
