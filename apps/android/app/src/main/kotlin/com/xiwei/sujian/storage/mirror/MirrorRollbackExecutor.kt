package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * MirrorRollbackExecutor — 发布事务回滚编排器。
 *
 * 从 ReadableMirrorPublisher 提取，只负责整事务回滚编排、journal 推进和 manifest rollback。
 * 单章节回滚逻辑在 [MirrorChapterRollbackExecutor] 中。
 */
internal class MirrorRollbackExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
) {
    private val manifestHelper = MirrorManifestRollbackHelper(stateStore, journalWriter)
    internal val chapterRollbackExecutor = MirrorChapterRollbackExecutor()

    internal fun rollbackChapterToOldState(
        journal: PendingMirrorPublish,
        key: ChapterKey,
        item: PendingItem,
        storage: ReadableMirrorStorage,
    ): RollbackItemResult = chapterRollbackExecutor.rollbackChapterToOldState(journal, key, item, storage)

    private fun readLatestPendingForTxStrict(txId: String): LatestPending {
        val result = stateStore.readPendingPublish()
        return when (result) {
            is PendingPublishResult.NotExists -> LatestPending.NotExists
            is PendingPublishResult.Corrupted -> LatestPending.CorruptedOrMismatch
            is PendingPublishResult.Success -> {
                val journal = PendingMirrorPublish.fromJson(result.json)
                if (journal != null && journal.txId == txId) {
                    LatestPending.Found(journal)
                } else {
                    LatestPending.CorruptedOrMismatch
                }
            }
        }
    }

    /**
     * 回滚整个发布事务：恢复所有 item 的 old backup，再删除 tx staging。
     *
     * #649 评论 5564379115 问题 1/2：统一事务回滚，替代逐 item 回滚。
     *
     * #649 评论 5564624383 问题 2：rollback 本身做成 journal 状态。
     * 统一顺序：
     * 0. 从磁盘读取最新 journal
     * 1. 删除所有 promotedRef
     * 2. 逐个恢复 backupOldRef / lookupBackup()
     * 3. manifest rollback → rollback(txId) → clearPendingPublish
     */
    internal suspend fun rollbackWholePublishTransaction(
        txId: String,
        items: Map<ChapterKey, PendingItem>,
        stagedRefs: Map<ChapterKey, StagedMirrorRef>,
        storage: ReadableMirrorStorage,
        journalContext: PendingMirrorPublish? = null,
    ): Boolean {
        val latestJournal = resolveLatestJournalForRollback(txId, journalContext) ?: return false

        val mergedItems = mergeRollbackItems(latestJournal, items)
        if (!writeRollbackJournalForItems(latestJournal, mergedItems)) {
            DiagnosticsLogger.w(TAG, "rollback: journal write failed at start")
            return false
        }

        val currentItems = mergedItems.toMutableMap()

        if (!rollbackDeletePromotedRefs(latestJournal, currentItems, storage)) return false
        if (!rollbackRestoreOldBackups(latestJournal, currentItems, storage)) return false
        if (!rollbackManifest(latestJournal, storage, currentItems)) {
            return false
        }

        if (!storage.rollback(txId)) {
            DiagnosticsLogger.w(TAG, "rollback: staging cleanup failed for tx $txId, keeping journal")
            return false
        }
        stateStore.clearPendingPublish()
        return true
    }

    private fun resolveLatestJournalForRollback(
        txId: String,
        journalContext: PendingMirrorPublish?,
    ): PendingMirrorPublish? {
        return when (val latest = readLatestPendingForTxStrict(txId)) {
            is LatestPending.Found -> latest.journal
            LatestPending.NotExists -> {
                journalContext
            }
            is LatestPending.CorruptedOrMismatch -> {
                DiagnosticsLogger.w(TAG, "rollback: cannot read latest journal for tx $txId (corrupted/mismatch)")
                null
            }
        }
    }

    private fun mergeRollbackItems(
        latestJournal: PendingMirrorPublish,
        items: Map<ChapterKey, PendingItem>,
    ): MutableMap<ChapterKey, PendingItem> {
        val mergedItems = latestJournal.items.toMutableMap()
        for ((key, callerItem) in items) {
            mergedItems[key] = callerItem
        }
        return mergedItems
    }

    private fun writeRollbackJournalForItems(
        journal: PendingMirrorPublish,
        items: Map<ChapterKey, PendingItem>,
    ): Boolean =
        journalWriter.writePendingPublishJournal(
            PendingJournalParams(
                projectId = journal.projectId,
                transactionType = journal.transactionType,
                phase = PendingMirrorPublish.PHASE_ROLLBACK,
                txId = journal.txId,
                backend = journal.backend,
                treeUri = journal.treeUri,
                oldEntries = journal.oldEntries,
                newEntries = journal.newEntries,
                stagedRefs = journal.stagedRefs,
                items = items,
                removedProjectIds = journal.removedProjectIds,
                manifestOldRef = journal.manifestOldRef,
                manifestStagedRef = journal.manifestStagedRef,
                manifestNewRef = journal.manifestNewRef,
                manifestBackupRef = journal.manifestBackupRef,
                manifestSwapState = journal.manifestSwapState,
                journalContext = journal,
            ),
        )

    /**
     * 阶段 1：幂等删除 promotedRef（跳过已处理的 item）。
     * #649 评论 5565067997 修复 4：检查 delete() 返回值，失败时停止推进状态。
     */
    private fun rollbackDeletePromotedRefs(
        latestJournal: PendingMirrorPublish,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ): Boolean {
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue
            if (item.state == PendingItem.STATE_ROLLBACK_NEW_REMOVED) {
                // 新内容已删，跳过删除步骤，直接进入恢复旧内容
            } else {
                // #649 评论 5565067997 修复 4：检查 delete() 返回值
                val removed = item.promotedRef?.let { storage.delete(it) } ?: true
                if (!removed) {
                    DiagnosticsLogger.w(
                        TAG,
                        "rollback: delete promotedRef failed for ${key.chapterId}, keeping journal",
                    )
                    return false
                }
                currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_NEW_REMOVED)
            }
            // 更新 journal（记录 NEW_REMOVED）
            if (!writeRollbackJournalForItems(latestJournal, currentItems)) {
                DiagnosticsLogger.w(TAG, "rollback: journal write failed after NEW_REMOVED for ${key.chapterId}")
                return false
            }
        }
        return true
    }

    /**
     * 阶段 2：逐个恢复 backupOldRef 到最终路径。
     * #649 评论 5570613481 问题 1：使用统一 helper。
     * #649 评论 5573310799 问题 1：每章恢复成功后先写 stateStore 再标记 journal。
     */
    private fun rollbackRestoreOldBackups(
        latestJournal: PendingMirrorPublish,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ): Boolean {
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue

            val rollbackResult = chapterRollbackExecutor.rollbackChapterToOldState(latestJournal, key, item, storage)
            if (!applyRollbackItemResult(rollbackResult, latestJournal, key, item, currentItems)) {
                return false
            }

            // 更新 journal（记录 OLD_RESTORED）
            if (!writeRollbackJournalForItems(latestJournal, currentItems)) {
                DiagnosticsLogger.w(TAG, "rollback: journal write failed after OLD_RESTORED for ${key.chapterId}")
                return false
            }
        }
        return true
    }

    private fun applyRollbackItemResult(
        rollbackResult: RollbackItemResult,
        latestJournal: PendingMirrorPublish,
        key: ChapterKey,
        item: PendingItem,
        currentItems: MutableMap<ChapterKey, PendingItem>,
    ): Boolean {
        when (rollbackResult) {
            is RollbackItemResult.Restored -> {
                // #649 评论 5573310799 问题 1：先把真实 URI 写进 stateStore，再标记 journal
                val oldEntry =
                    latestJournal.oldEntries[key] ?: run {
                        DiagnosticsLogger.w(TAG, "rollback: missing oldEntry for ${key.chapterId}, keeping journal")
                        return false
                    }
                if (!stateStore.putChapterEntry(
                        key.projectId,
                        key.volumeId,
                        key.chapterId,
                        oldEntry.copy(
                            uri = rollbackResult.ref.uri,
                            relativePath = rollbackResult.ref.relativePath,
                        ),
                    )
                ) {
                    DiagnosticsLogger.w(
                        TAG,
                        "rollback: putChapterEntry failed for ${key.chapterId}, keeping journal",
                    )
                    return false
                }
                currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_OLD_RESTORED)
            }
            RollbackItemResult.NewFileRemoved -> {
                currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_OLD_RESTORED)
            }
            RollbackItemResult.StateUnknown -> {
                DiagnosticsLogger.w(TAG, "rollback: state unknown for ${key.chapterId}, keeping journal")
                return false
            }
            RollbackItemResult.Failed -> {
                DiagnosticsLogger.w(TAG, "rollback: failed for ${key.chapterId}, keeping journal")
                return false
            }
        }
        return true
    }

    /**
     * Manifest rollback 入口（委托给 [MirrorManifestRollbackHelper]）。
     *
     * [MirrorRecoveryExecutor.recoverRollbackPhase] 和 [rollbackWholePublishTransaction] 共用此方法。
     *
     * @return true 表示 manifest rollback 成功（或不需要 rollback）；false 表示失败
     */
    internal fun rollbackManifest(
        journalContext: PendingMirrorPublish?,
        storage: ReadableMirrorStorage,
        items: Map<ChapterKey, PendingItem>,
    ): Boolean = manifestHelper.rollbackManifest(journalContext, storage, items)

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
    }
}
