package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot

/**
 * 从 MirrorPublishExecutor 提取，只负责 manifest 事务性写入逻辑。
 *
 * 包括：manifest 目标构建、stage、backup、promote、setManifestUri 和 commit。
 * 准备与备份阶段委托给 [MirrorManifestPrepareExecutor]。
 */
internal class MirrorManifestTransactionExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val planner: MirrorPublishPlanner,
) {
    private val prepareExecutor = MirrorManifestPrepareExecutor(stateStore, journalWriter, planner)

    internal data class ManifestTransactionParams(
        val projectId: String,
        val snapshot: ProjectWorkspaceSnapshot?,
        val desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        val txId: String,
        val journalContext: PendingMirrorPublish,
        val items: Map<ChapterKey, PendingItem>,
        val storage: ReadableMirrorStorage,
        val prebuiltTargetJson: String? = null,
    )

    internal data class ManifestTransactionResult(
        val committedJournal: PendingMirrorPublish,
        val newRef: MirrorFileRef,
    )

    internal sealed interface ManifestPromoteOutcome {
        data class Completed(val result: ManifestTransactionResult) : ManifestPromoteOutcome

        data class Proceed(
            val newRef: MirrorFileRef,
            val currentJournal: PendingMirrorPublish,
        ) : ManifestPromoteOutcome

        data object Aborted : ManifestPromoteOutcome
    }

    internal suspend fun publishManifestWithDesiredTransactional(
        params: ManifestTransactionParams,
    ): ManifestTransactionResult? {
        val manifestRelativePath = "$META_DIR/$MANIFEST_FILE_NAME"
        val ctx =
            MirrorManifestPrepareExecutor.ManifestTransactionContext(
                projectId = params.projectId,
                snapshot = params.snapshot,
                desiredEntries = params.desiredEntries,
                txId = params.txId,
                journalContext = params.journalContext,
                items = params.items,
                storage = params.storage,
                prebuiltTargetJson = params.prebuiltTargetJson,
                manifestRelativePath = manifestRelativePath,
            )
        val oldIdentity = prepareExecutor.resolveManifestOldIdentity(ctx) ?: return null
        val stageContext = prepareExecutor.prepareManifestTargetAndStage(ctx, oldIdentity) ?: return null
        when (val backupOutcome = prepareExecutor.executeManifestBackupPhase(ctx, oldIdentity, stageContext)) {
            is MirrorManifestPrepareExecutor.ManifestBackupOutcome.Completed -> return backupOutcome.result
            is MirrorManifestPrepareExecutor.ManifestBackupOutcome.Aborted -> return null
            is MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed -> {
                when (val promoteOutcome = promoteManifestStaged(ctx, stageContext, backupOutcome)) {
                    is ManifestPromoteOutcome.Completed -> return promoteOutcome.result
                    is ManifestPromoteOutcome.Aborted -> return null
                    is ManifestPromoteOutcome.Proceed ->
                        return commitManifestTransaction(ctx, promoteOutcome, backupOutcome.backupRef)
                }
            }
        }
    }

    private fun promoteManifestStaged(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        stageContext: MirrorManifestPrepareExecutor.ManifestStageContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        val finalLookup = ctx.storage.lookup(ctx.manifestRelativePath)
        if (finalLookup is MirrorLookupResult.Found) {
            return handlePromoteFinalFound(ctx, backupOutcome, finalLookup.ref)
        }
        if (finalLookup is MirrorLookupResult.Failed) {
            DiagnosticsLogger.w(
                TAG,
                "Manifest transaction: lookup final failed: ${finalLookup.cause?.message}, keeping journal",
            )
            return ManifestPromoteOutcome.Aborted
        }
        return handlePromoteFinalMissing(ctx, stageContext.staged, backupOutcome)
    }

    private fun handlePromoteFinalFound(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
        finalRef: MirrorFileRef,
    ): ManifestPromoteOutcome {
        var currentJournal = backupOutcome.currentJournal
        val desiredNewHash = currentJournal.manifestNewContentHash
        if (desiredNewHash == null) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: final exists but no expected hash, keeping journal")
            return ManifestPromoteOutcome.Aborted
        }
        val hashResult = ctx.storage.readTextAndHash(finalRef)
        if (hashResult == null) {
            DiagnosticsLogger.w(
                TAG,
                "Manifest transaction: readTextAndHash failed, cannot verify final identity, keeping journal",
            )
            return ManifestPromoteOutcome.Aborted
        }
        if (hashResult.second != desiredNewHash) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: final hash mismatch, state unknown, keeping journal")
            return ManifestPromoteOutcome.Aborted
        }
        if (!stateStore.setManifestUri(finalRef.uri)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed (final already new manifest)")
            return ManifestPromoteOutcome.Aborted
        }
        currentJournal =
            currentJournal.copy(
                manifestNewRef = finalRef,
                isManifestCommitted = true,
                manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
            )
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            return ManifestPromoteOutcome.Aborted
        }
        return ManifestPromoteOutcome.Completed(ManifestTransactionResult(currentJournal, finalRef))
    }

    private fun handlePromoteFinalMissing(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        staged: StagedMirrorRef?,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        val storage = ctx.storage
        var currentJournal = backupOutcome.currentJournal
        val newRef = if (staged != null) storage.promoteStaged(staged, ctx.manifestRelativePath) else null
        if (newRef == null) {
            backupOutcome.backupRef?.let {
                storage.restoreBackup(it, ctx.manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            }
            prepareExecutor.deleteStagedIfExists(storage, staged)
            return ManifestPromoteOutcome.Aborted
        }
        currentJournal =
            currentJournal.copy(
                manifestNewRef = newRef,
                manifestSwapState = ManifestTransactionState.MANIFEST_PROMOTED,
            )
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            storage.delete(newRef)
            backupOutcome.backupRef?.let {
                storage.restoreBackup(it, ctx.manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            }
            return ManifestPromoteOutcome.Aborted
        }
        return ManifestPromoteOutcome.Proceed(newRef, currentJournal)
    }

    private fun commitManifestTransaction(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        promoteOutcome: ManifestPromoteOutcome.Proceed,
        manifestBackupRef: MirrorFileRef?,
    ): ManifestTransactionResult? {
        val newRef = promoteOutcome.newRef
        var currentJournal = promoteOutcome.currentJournal
        if (!stateStore.setManifestUri(newRef.uri)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed")
            ctx.storage.delete(newRef)
            manifestBackupRef?.let {
                ctx.storage.restoreBackup(
                    it,
                    ctx.manifestRelativePath,
                    MIME_JSON,
                    currentJournal.manifestOldContentHash,
                )
            }
            return null
        }
        currentJournal =
            currentJournal.copy(
                isManifestCommitted = true,
                manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
            )
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: MANIFEST_COMMITTED journal write failed")
        }
        return ManifestTransactionResult(currentJournal, newRef)
    }

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val META_DIR = "_meta"
        private const val MANIFEST_FILE_NAME = "manifest.json"
        private const val MIME_JSON = "application/json"
    }
}
