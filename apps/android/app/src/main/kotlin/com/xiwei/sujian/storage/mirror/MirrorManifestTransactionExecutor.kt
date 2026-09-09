package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot

/**
 * 从 MirrorPublishExecutor 提取，只负责 manifest 事务性写入逻辑。
 *
 * 包括：manifest 目标构建、stage、backup、promote、setManifestUri 和 commit。
 */
internal class MirrorManifestTransactionExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val planner: MirrorPublishPlanner,
) {

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

    private data class ManifestTransactionContext(
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

    private data class ManifestOldIdentity(
        val oldRef: MirrorFileRef?,
        val oldContentHash: String?,
    )

    private data class ManifestStageContext(
        val newContentHash: String,
        val oldContentHash: String?,
        val staged: StagedMirrorRef?,
        val resumeState: ManifestTransactionState,
        val currentJournal: PendingMirrorPublish,
    )

    private sealed interface ManifestBackupOutcome {
        data class Completed(val result: ManifestTransactionResult) : ManifestBackupOutcome
        data class Proceed(
            val backupRef: MirrorFileRef?,
            val currentJournal: PendingMirrorPublish,
        ) : ManifestBackupOutcome
        data object Aborted : ManifestBackupOutcome
    }

    private sealed interface ManifestPromoteOutcome {
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
        val ctx = ManifestTransactionContext(
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
        val oldIdentity = resolveManifestOldIdentity(ctx) ?: return null
        val stageContext = prepareManifestTargetAndStage(ctx, oldIdentity) ?: return null
        when (val backupOutcome = executeManifestBackupPhase(ctx, oldIdentity, stageContext)) {
            is ManifestBackupOutcome.Completed -> return backupOutcome.result
            is ManifestBackupOutcome.Aborted -> return null
            is ManifestBackupOutcome.Proceed -> {
                when (val promoteOutcome = promoteManifestStaged(ctx, stageContext, backupOutcome)) {
                    is ManifestPromoteOutcome.Completed -> return promoteOutcome.result
                    is ManifestPromoteOutcome.Aborted -> return null
                    is ManifestPromoteOutcome.Proceed ->
                        return commitManifestTransaction(ctx, promoteOutcome, backupOutcome.backupRef)
                }
            }
        }
    }

    private fun resolveManifestOldIdentity(
        ctx: ManifestTransactionContext,
    ): ManifestOldIdentity? {
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

    private suspend fun prepareManifestTargetAndStage(
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
        return ManifestStageContext(manifestNewContentHash, oldIdentity.oldContentHash, staged, ManifestTransactionState.MANIFEST_STAGED, updatedJournal)
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
        return ManifestStageContext(manifestNewContentHash, oldIdentity.oldContentHash, staged, ManifestTransactionState.MANIFEST_STAGED, updatedJournal)
    }

    private fun deleteStagedIfExists(
        storage: ReadableMirrorStorage,
        staged: StagedMirrorRef?,
    ) {
        if (staged != null) {
            storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
        }
    }

    private fun executeManifestBackupPhase(
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
                    DiagnosticsLogger.w(TAG, "Manifest transaction: COMMITTED but readTextAndHash failed, keeping journal")
                    return ManifestBackupOutcome.Aborted
                }
                if (committedHashResult.second != stageContext.newContentHash) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: COMMITTED but final hash mismatch, keeping journal")
                    return ManifestBackupOutcome.Aborted
                }
                val committedJournal = stageContext.currentJournal.copy(
                    manifestNewRef = existingFinal.ref,
                    manifestBackupRef = manifestBackupRef,
                    isManifestCommitted = true,
                    manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                    manifestNewContentHash = stageContext.newContentHash,
                    manifestOldContentHash = stageContext.oldContentHash,
                )
                return ManifestBackupOutcome.Completed(
                    ManifestTransactionResult(committedJournal, existingFinal.ref),
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
                    DiagnosticsLogger.w(TAG, "Manifest transaction: PROMOTED but readTextAndHash failed, keeping journal")
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
                val updatedJournal = stageContext.currentJournal.copy(
                    manifestNewRef = existingFinal.ref,
                    manifestBackupRef = manifestBackupRef,
                    isManifestCommitted = true,
                    manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                )
                if (!journalWriter.persistPendingJournal(updatedJournal)) {
                    return ManifestBackupOutcome.Aborted
                }
                return ManifestBackupOutcome.Completed(
                    ManifestTransactionResult(updatedJournal, existingFinal.ref),
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
        val updatedJournal = stageContext.currentJournal.copy(
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
        val updatedJournal = stageContext.currentJournal.copy(
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
        var currentJournal = stageContext.currentJournal.copy(
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
            ctx.storage.restoreBackup(prepared.backupRef, ctx.manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            deleteStagedIfExists(ctx.storage, stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Proceed(manifestBackupRef, currentJournal)
    }

    private fun promoteManifestStaged(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        backupOutcome: ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        val finalLookup = ctx.storage.lookup(ctx.manifestRelativePath)
        if (finalLookup is MirrorLookupResult.Found) {
            return handlePromoteFinalFound(ctx, backupOutcome, finalLookup.ref)
        }
        if (finalLookup is MirrorLookupResult.Failed) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: lookup final failed: ${finalLookup.cause?.message}, keeping journal")
            return ManifestPromoteOutcome.Aborted
        }
        return handlePromoteFinalMissing(ctx, stageContext.staged, backupOutcome)
    }

    private fun handlePromoteFinalFound(
        ctx: ManifestTransactionContext,
        backupOutcome: ManifestBackupOutcome.Proceed,
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
            DiagnosticsLogger.w(TAG, "Manifest transaction: readTextAndHash failed, cannot verify final identity, keeping journal")
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
        currentJournal = currentJournal.copy(
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
        ctx: ManifestTransactionContext,
        staged: StagedMirrorRef?,
        backupOutcome: ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        val storage = ctx.storage
        var currentJournal = backupOutcome.currentJournal
        val newRef = if (staged != null) storage.promoteStaged(staged, ctx.manifestRelativePath) else null
        if (newRef == null) {
            backupOutcome.backupRef?.let {
                storage.restoreBackup(it, ctx.manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            }
            deleteStagedIfExists(storage, staged)
            return ManifestPromoteOutcome.Aborted
        }
        currentJournal = currentJournal.copy(manifestNewRef = newRef, manifestSwapState = ManifestTransactionState.MANIFEST_PROMOTED)
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
        ctx: ManifestTransactionContext,
        promoteOutcome: ManifestPromoteOutcome.Proceed,
        manifestBackupRef: MirrorFileRef?,
    ): ManifestTransactionResult? {
        val newRef = promoteOutcome.newRef
        var currentJournal = promoteOutcome.currentJournal
        if (!stateStore.setManifestUri(newRef.uri)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed")
            ctx.storage.delete(newRef)
            manifestBackupRef?.let {
                ctx.storage.restoreBackup(it, ctx.manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            }
            return null
        }
        currentJournal = currentJournal.copy(isManifestCommitted = true, manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED)
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
