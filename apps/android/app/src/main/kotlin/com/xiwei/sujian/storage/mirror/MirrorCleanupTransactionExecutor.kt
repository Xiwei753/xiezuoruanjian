package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * 从 MirrorPublishExecutor 提取，只负责 committed 事务的清理逻辑。
 *
 * Issue #667：backup 和 staging 文件在 [MirrorTransactionWorkspace]（私有目录）中，
 * 清理时用 workspace 方法删除；Download 中的旧最终文件仍用 [ReadableMirrorStorage] 删除。
 */
internal class MirrorCleanupTransactionExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val workspace: MirrorTransactionWorkspace,
) {
    internal fun cleanupCommittedTransaction(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        allLiveKeys: Set<ChapterKey>? = null,
    ): Boolean {
        val backupOk = cleanupBackupOldRefs(journal)
        val oldEntriesOk = cleanupOldEntries(journal, storage, allLiveKeys)
        val manifestBackupOk = cleanupManifestBackupRef(journal)
        val txOk = rollbackTxStaging(journal)
        return backupOk && oldEntriesOk && manifestBackupOk && txOk
    }

    /**
     * 清理 workspace 中的 backup 文件。
     *
     * Issue #667：backup 文件在私有目录，用 [MirrorTransactionWorkspace.deleteBackup] 删除。
     */
    private fun cleanupBackupOldRefs(
        journal: PendingMirrorPublish,
    ): Boolean {
        var allSuccess = true
        for ((_, item) in journal.items) {
            if (!deleteBackupOldRef(item)) allSuccess = false
        }
        return allSuccess
    }

    private fun deleteBackupOldRef(
        item: PendingItem,
    ): Boolean {
        val ref = item.backupOldRef ?: return true
        return try {
            workspace.deleteBackup(ref)
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to delete backupOldRef ${item.backupOldRef?.uri}", e)
            false
        }
    }

    /**
     * 清理 Download 中的旧最终文件（不再被引用的章节）。
     *
     * 这些文件在用户可见的 Download/Sujian/ 目录中，仍用 [ReadableMirrorStorage] 删除。
     */
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
                if (!removeOldChapterEntry(key)) allSuccess = false
            }
        }
        return allSuccess
    }

    private fun deleteOldEntry(
        entry: ChapterMirrorEntry,
        storage: ReadableMirrorStorage,
    ): Boolean {
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

    /**
     * 清理 workspace 中的 manifest backup 文件。
     *
     * Issue #667：manifest backup 在私有目录，用 [MirrorTransactionWorkspace.deleteBackup] 删除。
     */
    private fun cleanupManifestBackupRef(
        journal: PendingMirrorPublish,
    ): Boolean {
        val ref = journal.manifestBackupRef ?: return true
        return try {
            workspace.deleteBackup(ref)
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to delete manifestBackupRef ${journal.manifestBackupRef?.uri}", e)
            false
        }
    }

    /**
     * 清理 workspace 中的 staging 和 backup 目录。
     *
     * Issue #667：用 [MirrorTransactionWorkspace.rollback] 清理私有目录中的事务中间文件。
     */
    private fun rollbackTxStaging(
        journal: PendingMirrorPublish,
    ): Boolean {
        var allSuccess = true
        try {
            if (!workspace.rollback(journal.txId)) {
                DiagnosticsLogger.w(TAG, "cleanup: tx staging cleanup failed for ${journal.txId}")
                allSuccess = false
            }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to rollback tx ${journal.txId}", e)
            allSuccess = false
        }
        return allSuccess
    }

    private fun deleteRefByLookup(
        storage: ReadableMirrorStorage,
        relativePath: String,
        failMessage: (cause: Throwable?) -> String,
    ): Boolean {
        return when (val lookupResult = storage.lookup(relativePath)) {
            is MirrorLookupResult.Found -> storage.delete(lookupResult.ref)
            is MirrorLookupResult.Missing -> true
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(TAG, failMessage(lookupResult.cause))
                false
            }
        }
    }

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
    }
}
