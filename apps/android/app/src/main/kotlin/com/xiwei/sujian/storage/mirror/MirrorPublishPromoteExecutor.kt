package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * 从 [MirrorPublishProjectExecutor] 提取，只负责发布流程的 promote 阶段。
 *
 * Issue #667：事务中间文件（staging、backup）移到 [MirrorTransactionWorkspace]（私有目录），
 * [ReadableMirrorStorage] 只做最终用户可见文件的读写。
 *
 * 包括：promoteAllStaged、backupAndVacateItem、prepareItemBackup、
 * promoteItemStaged、findExistingPromotedRef，以及只属于 promote 阶段的类型。
 */
internal class MirrorPublishPromoteExecutor(
    private val journalWriter: MirrorJournalWriter,
    private val rollbackExecutor: MirrorRollbackExecutor,
    private val workspace: MirrorTransactionWorkspace,
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

    /**
     * 备份旧文件并推进 journal 状态。
     *
     * Issue #667：不再调用 `storage.vacateCommitted()`。
     * 私有 backup 是独立文件，不影响 Download 中的旧文件。
     * 旧文件的删除在 [promoteItemStaged] 中通过 `storage.delete(oldRef)` 完成。
     */
    private suspend fun backupAndVacateItem(
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
        // Issue #667：不再需要 vacateCommitted，私有 backup 不影响 Download 中的旧文件。
        // 直接推进到 STATE_OLD_VACATED，旧文件的删除在 promoteItemStaged 中完成。
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

    /**
     * 准备旧正文备份。
     *
     * Issue #667：使用 [MirrorTransactionWorkspace] 进行备份操作。
     * 1. 先用 `workspace.lookupBackup()` 检查是否已有备份（恢复时可能已存在）
     * 2. 若没有备份，先用 `storage.readTextAndHash()` 读旧内容，再用 `workspace.prepareBackup()` 写备份
     */
    private suspend fun prepareItemBackup(ctx: ItemBackupContext): ItemBackupOutcome? {
        val backupResult = workspace.lookupBackup(ctx.txId, ctx.oldRef.relativePath)
        return when (backupResult) {
            is MirrorLookupResult.Found -> {
                // 备份已存在（恢复场景），直接使用
                ItemBackupOutcome(backupResult.ref, true)
            }
            is MirrorLookupResult.Missing -> {
                // 读取旧正文内容，然后写入 workspace backup
                val oldContentResult = ctx.storage.readTextAndHash(ctx.oldRef)
                if (oldContentResult == null) {
                    logPublishAborted(
                        ctx.projectId,
                        "read old content failed for backup ${ctx.key.chapterId}",
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
                    val (oldContent, _) = oldContentResult
                    val prepared = workspace.prepareBackup(ctx.txId, ctx.oldRef, oldContent)
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
                        ItemBackupOutcome(prepared, true)
                    }
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

    /**
     * 将暂存内容提升到 Download 最终位置。
     *
     * Issue #667：新流程：
     * 1. 从 workspace 读取暂存内容
     * 2. 删除 Download 中的旧文件（如果存在）
     * 3. 在 Download 中创建新文件
     */
    private fun promoteItemStaged(
        key: ChapterKey,
        item: PendingItem,
        staged: StagedMirrorRef,
        desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        storage: ReadableMirrorStorage,
    ): MirrorFileRef? {
        val existingNewRef = findExistingPromotedRef(key, item, staged, desiredEntries, storage)
        if (existingNewRef != null) return existingNewRef

        // 从 workspace 读取暂存内容
        val content = workspace.readStaged(staged) ?: return null

        // 删除 Download 中的旧文件（如果存在），以便创建新文件
        val oldRef = item.oldRef
        if (oldRef != null) {
            when (val oldLookup = storage.lookup(oldRef.relativePath)) {
                is MirrorLookupResult.Found -> {
                    if (!storage.delete(oldLookup.ref)) {
                        DiagnosticsLogger.w(TAG, "promote: delete old file failed for ${key.chapterId}")
                    }
                }
                is MirrorLookupResult.Missing -> {
                    // 旧文件已不存在，无需删除
                }
                is MirrorLookupResult.Failed -> {
                    DiagnosticsLogger.w(
                        TAG,
                        "promote: lookup old failed for ${key.chapterId}: ${oldLookup.cause?.message}",
                    )
                }
            }
        }

        // 在 Download 中创建新文件
        val relativeDir = staged.finalRelativePath.substringBeforeLast('/', "")
        val displayName = staged.finalRelativePath.substringAfterLast('/')
        return storage.createText(relativeDir, displayName, staged.mimeType, content)
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
    }
}
