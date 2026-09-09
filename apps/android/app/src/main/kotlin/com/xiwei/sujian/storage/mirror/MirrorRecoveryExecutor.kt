package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * MirrorRecoveryExecutor — pending publish 恢复路由器。
 *
 * 按 journal phase 路由到对应的子执行器：
 * - promote → [MirrorPromoteRecoveryExecutor]
 * - cleanup → [MirrorCleanupRecoveryExecutor]
 * - rollback → [MirrorRollbackExecutor.recoverRollbackPhase]
 *
 * promote 完成后通过回调进入 cleanup，不再由本类实现具体阶段逻辑。
 */
internal class MirrorRecoveryExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val rollbackExecutor: MirrorRollbackExecutor,
    private val publishExecutor: MirrorPublishExecutor,
) {
    private val promoteExecutor = MirrorPromoteRecoveryExecutor(
        stateStore = stateStore,
        journalWriter = journalWriter,
        rollbackExecutor = rollbackExecutor,
        publishExecutor = publishExecutor,
    )

    private val cleanupExecutor = MirrorCleanupRecoveryExecutor(
        stateStore = stateStore,
        journalWriter = journalWriter,
        publishExecutor = publishExecutor,
    )

    internal suspend fun recoverPromotePhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        promoteExecutor.recoverPromotePhase(journal, storage) { cleanupJournal, cleanupStorage ->
            cleanupExecutor.recoverCleanupPhase(cleanupJournal, cleanupStorage)
        }
    }

    internal suspend fun recoverCleanupPhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        cleanupExecutor.recoverCleanupPhase(journal, storage)
    }

    /**
     * 恢复 rollback 阶段。
     *
     * #649 评论 5564624383 问题 2：进程死在回滚中间时继续回滚。
     */
    internal suspend fun recoverRollbackPhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        val currentItems = journal.items.toMutableMap()

        if (!rollbackDeletePromotedItems(journal, currentItems, storage)) return

        val allSuccess = rollbackRestoreOldItems(journal, currentItems, storage)
        if (!allSuccess) {
            DiagnosticsLogger.w(TAG, "Recover rollback: partial failure, keeping journal for retry")
            return
        }

        if (!rollbackExecutor.rollbackManifest(journal, storage, currentItems)) {
            DiagnosticsLogger.w(TAG, "Recover rollback: manifest rollback failed, keeping journal")
            return
        }
        if (storage.rollback(journal.txId)) {
            stateStore.clearPendingPublish()
        } else {
            DiagnosticsLogger.w(TAG, "Recover rollback: staging cleanup failed, keeping journal for retry")
        }
    }

    /** 步骤 1：删除 promotedRef（幂等，跳过已处理的 item）。返回 false 表示应停止恢复。 */
    private fun rollbackDeletePromotedItems(
        journal: PendingMirrorPublish,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ): Boolean {
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue
            if (item.state != PendingItem.STATE_ROLLBACK_NEW_REMOVED) {
                val removed = item.promotedRef?.let { storage.delete(it) } ?: true
                if (!removed) {
                    DiagnosticsLogger.w(TAG, "Recover rollback: delete promotedRef failed for ${key.chapterId}, keeping journal")
                    return false
                }
                currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_NEW_REMOVED)
            }
            if (!writeRollbackJournal(journal, currentItems)) {
                DiagnosticsLogger.w(TAG, "Recover rollback: journal write failed after NEW_REMOVED for ${key.chapterId}")
                return false
            }
        }
        return true
    }

    /** 步骤 2：恢复 backupOldRef 到最终路径。返回 false 表示应停止恢复。 */
    private fun rollbackRestoreOldItems(
        journal: PendingMirrorPublish,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ): Boolean {
        var allSuccess = true
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue
            when (val result = rollbackExecutor.rollbackChapterToOldState(journal, key, item, storage)) {
                is RollbackItemResult.Restored -> {
                    val oldEntry = journal.oldEntries[key] ?: run {
                        DiagnosticsLogger.w(TAG, "Recover rollback: missing oldEntry for ${key.chapterId}, keeping journal")
                        return false
                    }
                    if (!stateStore.putChapterEntry(key.projectId, key.volumeId, key.chapterId, oldEntry.copy(uri = result.ref.uri, relativePath = result.ref.relativePath))) {
                        DiagnosticsLogger.w(TAG, "Recover rollback: putChapterEntry failed for ${key.chapterId}, keeping journal")
                        return false
                    }
                    currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_OLD_RESTORED)
                }
                RollbackItemResult.NewFileRemoved -> {
                    currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_OLD_RESTORED)
                }
                RollbackItemResult.StateUnknown -> {
                    DiagnosticsLogger.w(TAG, "Recover rollback: state unknown for ${key.chapterId}, keeping journal")
                    return false
                }
                RollbackItemResult.Failed -> {
                    DiagnosticsLogger.w(TAG, "Recover rollback: failed for ${key.chapterId}, keeping journal")
                    allSuccess = false
                    continue
                }
            }
            if (!writeRollbackJournal(journal, currentItems)) {
                DiagnosticsLogger.w(TAG, "Recover rollback: journal write failed after OLD_RESTORED for ${key.chapterId}")
                return false
            }
        }
        return allSuccess
    }

    /** 写 rollback 阶段 journal。返回 false 表示写入失败。 */
    private fun writeRollbackJournal(
        journal: PendingMirrorPublish,
        items: Map<ChapterKey, PendingItem>,
    ): Boolean = journalWriter.writePendingPublishJournal(PendingJournalParams(
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
    ))

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
    }
}
