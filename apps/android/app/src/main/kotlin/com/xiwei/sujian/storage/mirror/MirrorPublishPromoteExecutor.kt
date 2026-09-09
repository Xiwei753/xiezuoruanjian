package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * 从 [MirrorPublishProjectExecutor] 提取，只负责发布流程的 promote 阶段。
 *
 * 包括：promoteAllStaged、backupAndVacateItem、prepareItemBackup、
 * promoteItemStaged、findExistingPromotedRef，以及只属于 promote 阶段的类型。
 */
internal class MirrorPublishPromoteExecutor(
    private val journalWriter: MirrorJournalWriter,
    private val rollbackExecutor: MirrorRollbackExecutor,
) {
    internal data class ItemBackupContext(
        val projectId: String,
        val key: ChapterKey,
        val txId: String,
        val oldRef: MirrorFileRef,
        val items: MutableMap<ChapterKey, PendingItem>,
        val stagedRefs: Map<ChapterKey, StagedMirrorRef>,
        val storage: ReadableMirrorStorage,
        val currentJournal: PendingMirrorPublish,
    )

    internal data class ItemBackupOutcome(val backupRef: MirrorFileRef, val vacated: Boolean)

    internal data class PromoteResult(
        val promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
        val items: Map<ChapterKey, PendingItem>,
        val currentJournal: PendingMirrorPublish,
    )

    internal suspend fun promoteAllStaged(
        projectId: String,
        stageResult: MirrorPublishStageExecutor.StageResult,
        storage: ReadableMirrorStorage,
        frozenContext: MirrorPublishStageExecutor.FrozenPlanContext,
        logPublishAborted: (String, String) -> Unit,
    ): PromoteResult? {
        val stagedRefs = stageResult.stagedRefs
        val desiredEntries = stageResult.desiredEntries
        val txId = frozenContext.currentJournal.txId
        val items = frozenContext.currentJournal.items.toMutableMap()
        var currentJournal = frozenContext.currentJournal
        val promotedEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
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
                val backupCtx =
                    ItemBackupContext(
                        projectId,
                        key,
                        txId,
                        oldRef,
                        items,
                        stagedRefs,
                        storage,
                        currentJournal,
                    )
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
        ctx.items[ctx.key] =
            item.copy(
                backupOldRef = backupOutcome.backupRef,
                state = PendingItem.STATE_BACKUP_READY,
            )
        var currentJournal = ctx.currentJournal.copy(items = ctx.items)
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            logPublishAborted(ctx.projectId, "journal write failed after backup prepare for ${ctx.key.chapterId}")
            rollbackExecutor.rollbackWholePublishTransaction(
                ctx.txId,
                ctx.items,
                ctx.stagedRefs,
                ctx.storage,
                currentJournal,
            )
            return null
        }
        if (!backupOutcome.vacated && !ctx.storage.vacateCommitted(ctx.oldRef)) {
            logPublishAborted(ctx.projectId, "vacate failed for ${ctx.key.chapterId}")
            rollbackExecutor.rollbackWholePublishTransaction(
                ctx.txId,
                ctx.items,
                ctx.stagedRefs,
                ctx.storage,
                currentJournal,
            )
            return null
        }
        ctx.items[ctx.key] = ctx.items[ctx.key]!!.copy(state = PendingItem.STATE_OLD_VACATED)
        currentJournal = currentJournal.copy(items = ctx.items)
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            logPublishAborted(ctx.projectId, "journal write failed after vacate for ${ctx.key.chapterId}")
            rollbackExecutor.rollbackWholePublishTransaction(
                ctx.txId,
                ctx.items,
                ctx.stagedRefs,
                ctx.storage,
                currentJournal,
            )
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
                        logPublishAborted(
                            ctx.projectId,
                            "lookup old failed for ${ctx.key.chapterId}: ${oldLookup.cause?.message}",
                        )
                        rollbackExecutor.rollbackWholePublishTransaction(
                            ctx.txId,
                            ctx.items,
                            ctx.stagedRefs,
                            ctx.storage,
                            ctx.currentJournal,
                        )
                        null
                    }
                }
            }
            is MirrorLookupResult.Missing -> {
                val prepared = ctx.storage.prepareBackup(ctx.txId, ctx.oldRef, MIME_MARKDOWN)
                if (prepared == null) {
                    logPublishAborted(
                        ctx.projectId,
                        "backup prepare failed for ${ctx.key.chapterId}",
                    )
                    rollbackExecutor.rollbackWholePublishTransaction(
                        ctx.txId,
                        ctx.items,
                        ctx.stagedRefs,
                        ctx.storage,
                        ctx.currentJournal,
                    )
                    null
                } else {
                    ItemBackupOutcome(prepared.backupRef, prepared.vacated)
                }
            }
            is MirrorLookupResult.Failed -> {
                logPublishAborted(
                    ctx.projectId,
                    "lookupBackup failed for ${ctx.key.chapterId}: ${backupResult.cause?.message}",
                )
                rollbackExecutor.rollbackWholePublishTransaction(
                    ctx.txId,
                    ctx.items,
                    ctx.stagedRefs,
                    ctx.storage,
                    ctx.currentJournal,
                )
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

    private fun logPublishAborted(
        projectId: String,
        message: String,
    ) {
        DiagnosticsLogger.w(TAG, "Publish project $projectId: $message")
    }

    private companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val MIME_MARKDOWN = "text/markdown"
    }
}
