package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * 从 MirrorRecoveryExecutor 提取，只负责 cleanup 阶段恢复逻辑。
 */
internal class MirrorCleanupRecoveryExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val publishExecutor: MirrorPublishExecutor,
) {
    internal suspend fun recoverCleanupPhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        when (journal.transactionType) {
            MirrorTransactionType.UPSERT_PROJECT -> recoverCleanupUpsertProject(journal, storage)
            MirrorTransactionType.DELETE_PROJECT -> recoverCleanupDeleteProject(journal, storage)
        }
    }

    private suspend fun recoverCleanupUpsertProject(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        if (!recoverCleanupBaseline(journal, "UPSERT_PROJECT")) return
        if (!stateStore.putChapterEntries(journal.newEntries)) {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: putChapterEntries failed for UPSERT_PROJECT ${journal.projectId}, keeping journal",
            )
            return
        }
        if (!stateStore.addPublishedProjectId(journal.projectId)) {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: addPublishedProjectId failed for UPSERT_PROJECT" +
                    " ${journal.projectId}, keeping journal",
            )
            return
        }
        if (publishExecutor.cleanupCommittedTransaction(journal, storage, allLiveKeys = journal.newEntries.keys)) {
            stateStore.clearPendingPublish()
        } else {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: partial failure for UPSERT_PROJECT ${journal.projectId}, keeping journal",
            )
        }
    }

    private suspend fun recoverCleanupDeleteProject(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        if (!recoverCleanupBaseline(journal, "DELETE_PROJECT")) return
        if (!journal.isManifestCommitted && !recoverCleanupDeleteManifest(journal, storage)) return
        val removeResult = stateStore.removeAllProjectEntries(journal.projectId)
        if (removeResult.isFailure) {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: removeAllProjectEntries failed for ${journal.projectId}, keeping journal",
            )
            return
        }
        if (publishExecutor.cleanupCommittedTransaction(journal, storage, allLiveKeys = null)) {
            if (!stateStore.removePublishedProjectId(journal.projectId)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover cleanup: removePublishedProjectId failed for ${journal.projectId}, keeping journal",
                )
                return
            }
            stateStore.clearPendingPublish()
        } else {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: partial failure for DELETE_PROJECT ${journal.projectId}, keeping journal",
            )
        }
    }

    private fun recoverCleanupBaseline(
        journal: PendingMirrorPublish,
        type: String,
    ): Boolean {
        if (journal.isManifestCommitted && !journalWriter.persistCommittedBaselineFromJournal(journal)) {
            DiagnosticsLogger.w(
                TAG,
                RECOVER_CLEANUP_BASELINE_FAILED + "$type " + projectIdKeepingJournal(journal.projectId),
            )
            return false
        }
        return true
    }

    private suspend fun recoverCleanupDeleteManifest(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ): Boolean {
        val targetJsonOutcome = resolveDeleteRecoveryManifestTargetJson(journal)
        if (targetJsonOutcome == DeleteManifestTargetJsonOutcome.Failed) return false
        val recoveryManifestTargetJson = (targetJsonOutcome as DeleteManifestTargetJsonOutcome.Resolved).json
        val desiredWithoutDeleted = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        for ((key, entry) in journal.oldEntries) {
            if (key.projectId != journal.projectId) {
                desiredWithoutDeleted[key] = entry
            }
        }
        val manifestParams =
            ManifestTransactionParams(
                projectId = journal.projectId,
                snapshot = null,
                desiredEntries = if (recoveryManifestTargetJson != null) emptyMap() else desiredWithoutDeleted,
                txId = journal.txId,
                journalContext = journal,
                items = journal.items,
                storage = storage,
                prebuiltTargetJson = recoveryManifestTargetJson,
            )
        val manifestResult = publishExecutor.publishManifestWithDesiredTransactional(manifestParams)
        if (manifestResult == null) {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: manifest rewrite failed for DELETE_PROJECT ${journal.projectId}",
            )
            return false
        }
        if (!journalWriter.persistCommittedBaselineFromJournal(manifestResult.committedJournal)) {
            DiagnosticsLogger.w(
                TAG,
                RECOVER_CLEANUP_BASELINE_FAILED + "DELETE_PROJECT " + projectIdKeepingJournal(journal.projectId),
            )
            return false
        }
        return true
    }

    private sealed interface DeleteManifestTargetJsonOutcome {
        data class Resolved(val json: String?) : DeleteManifestTargetJsonOutcome

        data object Failed : DeleteManifestTargetJsonOutcome
    }

    private fun resolveDeleteRecoveryManifestTargetJson(
        journal: PendingMirrorPublish,
    ): DeleteManifestTargetJsonOutcome {
        if (journal.manifestTargetJson != null) {
            return DeleteManifestTargetJsonOutcome.Resolved(journal.manifestTargetJson)
        }
        if (journal.frozenManifestPlan != null && journal.frozenManifestPlanHash != null) {
            if (computeContentHash(journal.frozenManifestPlan) != journal.frozenManifestPlanHash) {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover cleanup: frozenManifestPlan hash mismatch for DELETE, keeping journal",
                )
                return DeleteManifestTargetJsonOutcome.Failed
            }
            val plan =
                frozenManifestPlanFromJson(journal.frozenManifestPlan) ?: run {
                    DiagnosticsLogger.w(
                        TAG,
                        "Recover cleanup: failed to parse frozenManifestPlan for DELETE, keeping journal",
                    )
                    return DeleteManifestTargetJsonOutcome.Failed
                }
            return DeleteManifestTargetJsonOutcome.Resolved(frozenPlanToManifestJson(plan, emptyMap()))
        }
        return DeleteManifestTargetJsonOutcome.Resolved(null)
    }

    private fun projectIdKeepingJournal(projectId: String): String = "$projectId, keeping journal"

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val RECOVER_CLEANUP_BASELINE_FAILED = "Recover cleanup: committed baseline failed for "
    }
}
