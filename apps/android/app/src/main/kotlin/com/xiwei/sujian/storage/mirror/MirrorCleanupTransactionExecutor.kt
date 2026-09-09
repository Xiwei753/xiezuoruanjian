package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * 从 MirrorPublishExecutor 提取，只负责 committed 事务的清理逻辑。
 *
 * 包括：删除 backup old refs、删除已删除章节的旧 ref、删除 manifest 备份和清理 tx staging。
 */
internal class MirrorCleanupTransactionExecutor(
    private val stateStore: ReadableMirrorStateStore,
) {

    internal fun cleanupCommittedTransaction(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        allLiveKeys: Set<ChapterKey>? = null,
    ): Boolean {
        val backupOk = cleanupBackupOldRefs(journal, storage)
        val oldEntriesOk = cleanupOldEntries(journal, storage, allLiveKeys)
        val manifestBackupOk = cleanupManifestBackupRef(journal, storage)
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
        return when (val lookupResult = storage.lookup(relativePath)) {
            is MirrorLookupResult.Found -> storage.delete(lookupResult.ref)
            is MirrorLookupResult.Missing -> true
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

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
    }
}
