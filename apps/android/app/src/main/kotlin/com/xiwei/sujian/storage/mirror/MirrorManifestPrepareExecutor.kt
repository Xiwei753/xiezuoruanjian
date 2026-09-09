package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot

/**
 * 从 [MirrorManifestTransactionExecutor] 提取，只负责 manifest 事务的准备与备份阶段。
 *
 * 包括：resolveManifestOldIdentity、prepareManifestTargetAndStage、
 * stagePrebuiltManifest、buildAndStageManifest、executeManifestBackupPhase
 * 及所有 handleBackup* 分支。
 */
internal class MirrorManifestPrepareExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val planner: MirrorPublishPlanner,
) {
    internal data class ManifestTransactionContext(
        val projectId: String,
        val snapshot: ProjectWorkspaceSnapshot?,
        val desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        val txId: String,
        val journalContext: PendingMirrorPublish,
        val items: Map<ChapterKey, PendingItem>,
        val storage: ReadableMirrorStorage,
        val prebuiltTargetJson: String?,
        val manifestRelativePath: String,
    )

    internal data class ManifestOldIdentity(
        val oldRef: MirrorFileRef?,
        val oldContentHash: String?,
    )

    internal data class ManifestStageContext(
        val newContentHash: String,
        val oldContentHash: String?,
        val staged: StagedMirrorRef?,
        val resumeState: ManifestTransactionState,
        val currentJournal: PendingMirrorPublish,
    )

    internal sealed interface ManifestBackupOutcome {
        data class Completed(
            val result: MirrorManifestTransactionExecutor.ManifestTransactionResult,
        ) : ManifestBackupOutcome

        data class Proceed(
            val backupRef: MirrorFileRef?,
            val currentJournal: PendingMirrorPublish,
        ) : ManifestBackupOutcome

        data object Aborted : ManifestBackupOutcome
    }

    internal fun resolveManifestOldIdentity(ctx: ManifestTransactionContext): ManifestOldIdentity? {
        val isResumingManifest = ctx.journalContext.manifestTargetJson != null
        val frozenOldRef: MirrorFileRef? =
            if (isResumingManifest) {
                ctx.journalContext.manifestOldRef
            } else {
                val initialOldUri = stateStore.getManifestUri()
                initialOldUri?.let { MirrorFileRef(uri = it, relativePath = ctx.manifestRelativePath) }
            }
        val manifestOldContentHash =
            ctx.journalContext.manifestOldContentHash ?: run {
                frozenOldRef?.let { ctx.storage.readTextAndHash(it)?.second }
            }
        if (frozenOldRef != null && manifestOldContentHash == null) {
            DiagnosticsLogger.w(
                TAG,
                "Manifest transaction: old manifest exists but readTextAndHash failed, stopping before vacate",
            )
            return null
        }
        return ManifestOldIdentity(frozenOldRef, manifestOldContentHash)
    }

    internal suspend fun prepareManifestTargetAndStage(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
    ): ManifestStageContext? {
        val currentJournal = ctx.journalContext.copy(items = ctx.items, newEntries = ctx.desiredEntries)
        if (ctx.journalContext.manifestTargetJson != null) {
            return ManifestStageContext(
                newContentHash = computeContentHash(ctx.journalContext.manifestTargetJson),
                oldContentHash = oldIdentity.oldContentHash,
                staged = ctx.journalContext.manifestStagedRef,
                resumeState = ctx.journalContext.manifestSwapState,
                currentJournal = currentJournal,
            )
        } else if (ctx.prebuiltTargetJson != null) {
            return stagePrebuiltManifest(ctx, oldIdentity, currentJournal)
        } else {
            return buildAndStageManifest(ctx, oldIdentity, currentJournal)
        }
    }

    private fun stagePrebuiltManifest(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        currentJournal: PendingMirrorPublish,
    ): ManifestStageContext? {
        val json = ctx.prebuiltTargetJson!!
        val manifestNewContentHash = computeContentHash(json)
        val staged =
            ctx.storage.stageText(
                txId = ctx.txId,
                relativePath = ctx.manifestRelativePath,
                mimeType = MIME_JSON,
                text = json,
            ) ?: return null
        val updatedJournal =
            currentJournal.copy(
                manifestOldRef = oldIdentity.oldRef,
                manifestStagedRef = staged,
                manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                manifestNewContentHash = manifestNewContentHash,
                manifestOldContentHash = oldIdentity.oldContentHash,
                manifestTargetJson = json,
            )
        if (!journalWriter.persistPendingJournal(updatedJournal)) {
            ctx.storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
            return null
        }
        return ManifestStageContext(
            manifestNewContentHash,
            oldIdentity.oldContentHash,
            staged,
            ManifestTransactionState.MANIFEST_STAGED,
            updatedJournal,
        )
    }

    private fun buildAndStageManifest(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        currentJournal: PendingMirrorPublish,
    ): ManifestStageContext? {
        val json = planner.buildManifestJsonForDesired(ctx.projectId, ctx.snapshot, ctx.desiredEntries) ?: return null
        val manifestNewContentHash = computeContentHash(json)
        val staged =
            ctx.storage.stageText(
                txId = ctx.txId,
                relativePath = ctx.manifestRelativePath,
                mimeType = MIME_JSON,
                text = json,
            ) ?: return null
        val updatedJournal =
            currentJournal.copy(
                manifestOldRef = oldIdentity.oldRef,
                manifestStagedRef = staged,
                manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                manifestNewContentHash = manifestNewContentHash,
                manifestOldContentHash = oldIdentity.oldContentHash,
                manifestTargetJson = json,
            )
        if (!journalWriter.persistPendingJournal(updatedJournal)) {
            ctx.storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
            return null
        }
        return ManifestStageContext(
            manifestNewContentHash,
            oldIdentity.oldContentHash,
            staged,
            ManifestTransactionState.MANIFEST_STAGED,
            updatedJournal,
        )
    }

    internal fun deleteStagedIfExists(
        storage: ReadableMirrorStorage,
        staged: StagedMirrorRef?,
    ) {
        if (staged != null) {
            storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
        }
    }

    internal fun executeManifestBackupPhase(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        stageContext: ManifestStageContext,
    ): ManifestBackupOutcome {
        when (val backupResult = ctx.storage.lookupBackup(ctx.txId, ctx.manifestRelativePath)) {
            is MirrorLookupResult.Found -> {
                val manifestBackupRef = backupResult.ref
                return when (stageContext.resumeState) {
                    ManifestTransactionState.MANIFEST_COMMITTED ->
                        handleBackupFoundCommitted(ctx, stageContext, manifestBackupRef)
                    ManifestTransactionState.MANIFEST_PROMOTED ->
                        handleBackupFoundPromoted(ctx, stageContext, manifestBackupRef)
                    ManifestTransactionState.MANIFEST_OLD_VACATED ->
                        handleBackupFoundOldVacated(ctx, stageContext, manifestBackupRef)
                    else ->
                        handleBackupFoundNeedsVacate(ctx, oldIdentity, stageContext, manifestBackupRef)
                }
            }
            is MirrorLookupResult.Missing -> {
                return handleBackupMissing(ctx, oldIdentity, stageContext)
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(TAG, "manifest backup lookup failed: ${backupResult.cause?.message}")
                deleteStagedIfExists(ctx.storage, stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
        }
    }

    private fun handleBackupFoundCommitted(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        val existingFinal = ctx.storage.lookup(ctx.manifestRelativePath)
        when (existingFinal) {
            is MirrorLookupResult.Found -> {
                val committedHashResult = ctx.storage.readTextAndHash(existingFinal.ref)
                if (committedHashResult == null) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Manifest transaction: COMMITTED but readTextAndHash failed, keeping journal",
                    )
                    return ManifestBackupOutcome.Aborted
                }
                if (committedHashResult.second != stageContext.newContentHash) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: COMMITTED but final hash mismatch, keeping journal")
                    return ManifestBackupOutcome.Aborted
                }
                val committedJournal =
                    stageContext.currentJournal.copy(
                        manifestNewRef = existingFinal.ref,
                        manifestBackupRef = manifestBackupRef,
                        isManifestCommitted = true,
                        manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                        manifestNewContentHash = stageContext.newContentHash,
                        manifestOldContentHash = stageContext.oldContentHash,
                    )
                return ManifestBackupOutcome.Completed(
                    MirrorManifestTransactionExecutor.ManifestTransactionResult(committedJournal, existingFinal.ref),
                )
            }
            is MirrorLookupResult.Missing -> {
                DiagnosticsLogger.w(TAG, "Manifest transaction: COMMITTED but final missing, rolling back")
                deleteStagedIfExists(ctx.storage, stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Manifest transaction: lookup final failed (COMMITTED): ${existingFinal.cause?.message}",
                )
                deleteStagedIfExists(ctx.storage, stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
        }
    }

    private fun handleBackupFoundPromoted(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        val existingFinal = ctx.storage.lookup(ctx.manifestRelativePath)
        when (existingFinal) {
            is MirrorLookupResult.Found -> {
                val promotedHashResult = ctx.storage.readTextAndHash(existingFinal.ref)
                if (promotedHashResult == null) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Manifest transaction: PROMOTED but readTextAndHash failed, keeping journal",
                    )
                    return ManifestBackupOutcome.Aborted
                }
                if (promotedHashResult.second != stageContext.newContentHash) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: PROMOTED but final hash mismatch, keeping journal")
                    return ManifestBackupOutcome.Aborted
                }
                if (!stateStore.setManifestUri(existingFinal.ref.uri)) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed (resume PROMOTED)")
                    return ManifestBackupOutcome.Aborted
                }
                val updatedJournal =
                    stageContext.currentJournal.copy(
                        manifestNewRef = existingFinal.ref,
                        manifestBackupRef = manifestBackupRef,
                        isManifestCommitted = true,
                        manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                    )
                if (!journalWriter.persistPendingJournal(updatedJournal)) {
                    return ManifestBackupOutcome.Aborted
                }
                return ManifestBackupOutcome.Completed(
                    MirrorManifestTransactionExecutor.ManifestTransactionResult(updatedJournal, existingFinal.ref),
                )
            }
            is MirrorLookupResult.Missing -> {
                DiagnosticsLogger.w(TAG, "Manifest transaction: PROMOTED but final missing")
                deleteStagedIfExists(ctx.storage, stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Manifest transaction: lookup final failed (PROMOTED): ${existingFinal.cause?.message}",
                )
                deleteStagedIfExists(ctx.storage, stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
        }
    }

    private fun handleBackupFoundOldVacated(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        val updatedJournal =
            stageContext.currentJournal.copy(
                manifestBackupRef = manifestBackupRef,
                manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED,
            )
        if (!journalWriter.persistPendingJournal(updatedJournal)) {
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Proceed(manifestBackupRef, updatedJournal)
    }

    private fun handleBackupFoundNeedsVacate(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        val oldRef = oldIdentity.oldRef
        if (oldRef != null) {
            val oldLookup = ctx.storage.lookup(oldRef.relativePath)
            when (oldLookup) {
                is MirrorLookupResult.Found -> {
                    if (!ctx.storage.vacateCommitted(oldRef)) {
                        DiagnosticsLogger.w(TAG, "Manifest transaction: vacate failed (resume BACKUP_READY)")
                        deleteStagedIfExists(ctx.storage, stageContext.staged)
                        return ManifestBackupOutcome.Aborted
                    }
                }
                is MirrorLookupResult.Missing -> {
                    // old 已腾空，无需再 vacate
                }
                is MirrorLookupResult.Failed -> {
                    DiagnosticsLogger.w(
                        TAG,
                        "Manifest transaction: lookup old failed (resume BACKUP_READY): ${oldLookup.cause?.message}",
                    )
                    deleteStagedIfExists(ctx.storage, stageContext.staged)
                    return ManifestBackupOutcome.Aborted
                }
            }
        }
        val updatedJournal =
            stageContext.currentJournal.copy(
                manifestBackupRef = manifestBackupRef,
                manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED,
            )
        if (!journalWriter.persistPendingJournal(updatedJournal)) {
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Proceed(manifestBackupRef, updatedJournal)
    }

    private fun handleBackupMissing(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        stageContext: ManifestStageContext,
    ): ManifestBackupOutcome {
        val oldRef = oldIdentity.oldRef
        if (oldRef == null) {
            return ManifestBackupOutcome.Proceed(null, stageContext.currentJournal)
        }
        val prepared = ctx.storage.prepareBackup(ctx.txId, oldRef, MIME_JSON)
        if (prepared == null) {
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        val manifestBackupRef = prepared.backupRef
        var currentJournal =
            stageContext.currentJournal.copy(
                manifestBackupRef = manifestBackupRef,
                manifestSwapState = ManifestTransactionState.MANIFEST_BACKUP_READY,
            )
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        if (!prepared.vacated && !ctx.storage.vacateCommitted(oldRef)) {
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        currentJournal = currentJournal.copy(manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED)
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            ctx.storage.restoreBackup(
                prepared.backupRef,
                ctx.manifestRelativePath,
                MIME_JSON,
                currentJournal.manifestOldContentHash,
            )
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Proceed(manifestBackupRef, currentJournal)
    }

    private companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val MIME_JSON = "application/json"
    }
}
