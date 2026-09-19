package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.interop.diagnostics.DiagnosticsInterop
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot

/**
 * 从 [MirrorManifestTransactionExecutor] 提取，只负责 manifest 事务的准备与备份阶段。
 *
 * Issue #667：manifest 的 staging 和 backup 操作移到 [MirrorTransactionWorkspace]（私有目录）。
 * manifest 最终位置也在私有目录（通过 [MirrorTransactionWorkspace.writeManifest]）。
 *
 * 包括：resolveManifestOldIdentity、prepareManifestTargetAndStage、
 * stagePrebuiltManifest、buildAndStageManifest、executeManifestBackupPhase
 * 及所有 handleBackup* 分支。
 */
internal class MirrorManifestPrepareExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val planner: MirrorPublishPlanner,
    private val workspace: MirrorTransactionWorkspace,
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
        // Issue #717 评论 5741567193 A 部分：本次冻结的 committed manifest hash。
        val committedManifestHash: String? = null,
        // Issue #717 评论 5741567193 A 部分：old-baseline 存在状态。
        val oldBaselineExists: Boolean = false,
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

    /**
     * 解析旧 manifest 身份。
     *
     * Issue #717 评论 5741567193 A 部分：正常事务的旧 manifest 身份来源统一收口到
     * committed baseline + 私有 workspace manifest，不再用
     * `stateStore.getManifestUri() != null` 推断，也不再 fallback 到公开
     * `Download/Sujian/_meta/manifest.json`。
     *
     * 规则：
     * - 恢复场景（`journalContext.manifestTargetJson != null`）：继续使用 journal 已冻结的
     *   `manifestOldRef` / `manifestOldContentHash`，不重新从 state 猜。
     * - 正常事务 + 无 committed baseline（首次发布）：`oldRef=null, oldContentHash=null`。
     * - 正常事务 + 有 committed baseline：
     *   `oldRef=workspace.manifestRef(), oldContentHash=committedManifestHash`。
     *
     * 公开 Download 下的旧 manifest 退出正常状态机。最终只保留三份事实：
     * committed baseline、私有 workspace manifest、pending journal。
     */
    internal fun resolveManifestOldIdentity(ctx: ManifestTransactionContext): ManifestOldIdentity? {
        // Issue #717 评论 5741567193 A 部分：recovery 用 journal。
        // 只要 journal 冻结了 manifestTargetJson / manifestOldRef / manifestOldContentHash 中的任一个，
        // 就走恢复分支，继续使用 journal 的值，不重新从 state 猜。
        val isResumingManifest =
            ctx.journalContext.manifestTargetJson != null ||
                ctx.journalContext.manifestOldRef != null ||
                ctx.journalContext.manifestOldContentHash != null
        if (isResumingManifest) {
            // 恢复场景：继续使用 journal 已冻结的 manifestOldRef/manifestOldContentHash。
            val frozenOldRef = ctx.journalContext.manifestOldRef
            val frozenOldContentHash = ctx.journalContext.manifestOldContentHash
            if (frozenOldRef != null && frozenOldContentHash == null) {
                DiagnosticsInterop.w(
                    TAG,
                    "Manifest transaction: journal has manifestOldRef but no manifestOldContentHash, stopping",
                )
                return null
            }
            return ManifestOldIdentity(frozenOldRef, frozenOldContentHash)
        }
        // 正常事务：用 committed baseline 推断旧 manifest 身份。
        if (!ctx.oldBaselineExists) {
            // 首次发布：无旧 manifest。
            return ManifestOldIdentity(oldRef = null, oldContentHash = null)
        }
        // 有 committed baseline：旧 manifest 身份来自私有 workspace manifest。
        val oldRef = workspace.manifestRef()
        val oldContentHash = ctx.committedManifestHash
        if (oldContentHash == null) {
            DiagnosticsInterop.w(
                TAG,
                "Manifest transaction: oldBaselineExists=true but committedManifestHash is null, stopping",
            )
            return null
        }
        return ManifestOldIdentity(oldRef, oldContentHash)
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

    /**
     * Issue #667：manifest staging 用 [MirrorTransactionWorkspace.stageText] 写到私有目录。
     */
    private fun stagePrebuiltManifest(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        currentJournal: PendingMirrorPublish,
    ): ManifestStageContext? {
        val json = ctx.prebuiltTargetJson!!
        val manifestNewContentHash = computeContentHash(json)
        val staged =
            workspace.stageText(
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
            workspace.deleteStaged(staged)
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

    /**
     * Issue #667：manifest staging 用 [MirrorTransactionWorkspace.stageText] 写到私有目录。
     */
    private suspend fun buildAndStageManifest(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        currentJournal: PendingMirrorPublish,
    ): ManifestStageContext? {
        val json = planner.buildManifestJsonForDesired(ctx.projectId, ctx.snapshot, ctx.desiredEntries) ?: return null
        val manifestNewContentHash = computeContentHash(json)
        val staged =
            workspace.stageText(
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
            workspace.deleteStaged(staged)
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

    /**
     * Issue #667：删除 workspace 中的 staged manifest 文件。
     */
    internal fun deleteStagedIfExists(staged: StagedMirrorRef?) {
        if (staged != null) {
            workspace.deleteStaged(staged)
        }
    }

    /**
     * Issue #667：manifest backup 用 [MirrorTransactionWorkspace] 操作。
     */
    internal fun executeManifestBackupPhase(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        stageContext: ManifestStageContext,
    ): ManifestBackupOutcome {
        when (val backupResult = workspace.lookupBackup(ctx.txId, ctx.manifestRelativePath)) {
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
                DiagnosticsInterop.w(TAG, "manifest backup lookup failed: ${backupResult.cause?.message}")
                deleteStagedIfExists(stageContext.staged)
                return ManifestBackupOutcome.Aborted
            }
        }
    }

    /**
     * Issue #667：manifest 已提交到私有目录。
     * 检查私有目录中的 manifest 内容是否匹配新 manifest hash。
     */
    private fun handleBackupFoundCommitted(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        // Issue #667：manifest 在私有目录中，用 workspace.readManifest() 读取
        val currentContent = workspace.readManifest()
        if (currentContent == null) {
            DiagnosticsInterop.w(TAG, "Manifest transaction: COMMITTED but manifest missing in workspace, rolling back")
            deleteStagedIfExists(stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        val currentHash = computeContentHash(currentContent)
        if (currentHash != stageContext.newContentHash) {
            DiagnosticsInterop.w(TAG, "Manifest transaction: COMMITTED but manifest hash mismatch, keeping journal")
            return ManifestBackupOutcome.Aborted
        }
        // manifest 已提交，构造 committed journal
        val manifestPath = workspace.manifestFile().absolutePath
        val committedRef = MirrorFileRef(uri = manifestPath, relativePath = ctx.manifestRelativePath)
        val committedJournal =
            stageContext.currentJournal.copy(
                manifestNewRef = committedRef,
                manifestBackupRef = manifestBackupRef,
                isManifestCommitted = true,
                manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                manifestNewContentHash = stageContext.newContentHash,
                manifestOldContentHash = stageContext.oldContentHash,
            )
        return ManifestBackupOutcome.Completed(
            MirrorManifestTransactionExecutor.ManifestTransactionResult(committedJournal, committedRef),
        )
    }

    /**
     * Issue #667：manifest 已 promote 到私有目录但尚未 commit。
     */
    private fun handleBackupFoundPromoted(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        // Issue #667：manifest 在私有目录中，用 workspace.readManifest() 读取
        val currentContent = workspace.readManifest()
        if (currentContent == null) {
            DiagnosticsInterop.w(TAG, "Manifest transaction: PROMOTED but manifest missing in workspace")
            deleteStagedIfExists(stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        val currentHash = computeContentHash(currentContent)
        if (currentHash != stageContext.newContentHash) {
            DiagnosticsInterop.w(TAG, "Manifest transaction: PROMOTED but manifest hash mismatch, keeping journal")
            return ManifestBackupOutcome.Aborted
        }
        // manifest 已 promote，setManifestUri 指向私有文件路径
        val manifestPath = workspace.manifestFile().absolutePath
        if (!stateStore.setManifestUri(manifestPath)) {
            DiagnosticsInterop.w(TAG, "Manifest transaction: setManifestUri failed (resume PROMOTED)")
            return ManifestBackupOutcome.Aborted
        }
        val committedRef = MirrorFileRef(uri = manifestPath, relativePath = ctx.manifestRelativePath)
        val updatedJournal =
            stageContext.currentJournal.copy(
                manifestNewRef = committedRef,
                manifestBackupRef = manifestBackupRef,
                isManifestCommitted = true,
                manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
            )
        if (!journalWriter.persistPendingJournal(updatedJournal)) {
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Completed(
            MirrorManifestTransactionExecutor.ManifestTransactionResult(updatedJournal, committedRef),
        )
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
            deleteStagedIfExists(stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Proceed(manifestBackupRef, updatedJournal)
    }

    /**
     * Issue #667：不再需要 vacateCommitted。
     * 私有目录中的 manifest 不影响 Download 中的文件。
     * 直接推进到 MANIFEST_OLD_VACATED 状态。
     */
    private fun handleBackupFoundNeedsVacate(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        stageContext: ManifestStageContext,
        manifestBackupRef: MirrorFileRef,
    ): ManifestBackupOutcome {
        // Issue #667：不再需要 vacateCommitted，私有 backup 不影响 Download 中的旧文件。
        val updatedJournal =
            stageContext.currentJournal.copy(
                manifestBackupRef = manifestBackupRef,
                manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED,
            )
        if (!journalWriter.persistPendingJournal(updatedJournal)) {
            deleteStagedIfExists(stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Proceed(manifestBackupRef, updatedJournal)
    }

    /**
     * Issue #667：manifest backup 缺失时，先读取旧 manifest 内容再写入 workspace backup。
     *
     * Issue #717 评论 5741567193 A 部分：正常事务只从私有 workspace manifest 读取旧内容，
     * 不再 fallback 到公开 `Download/Sujian/_meta/manifest.json`。
     */
    private fun handleBackupMissing(
        ctx: ManifestTransactionContext,
        oldIdentity: ManifestOldIdentity,
        stageContext: ManifestStageContext,
    ): ManifestBackupOutcome {
        val oldRef = oldIdentity.oldRef
        if (oldRef == null) {
            // 没有旧 manifest（首次发布），不需要 backup
            return ManifestBackupOutcome.Proceed(null, stageContext.currentJournal)
        }
        // Issue #717 评论 5741567193 A 部分：只从私有 workspace manifest 读取旧内容。
        // 公开 Download 下的旧 manifest 退出正常状态机。
        val oldContent = workspace.readManifest()
        if (oldContent == null) {
            DiagnosticsInterop.w(
                TAG,
                "Manifest transaction: cannot read old manifest from private workspace for backup",
            )
            deleteStagedIfExists(stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        val prepared = workspace.prepareBackup(ctx.txId, oldRef, oldContent)
        if (prepared == null) {
            deleteStagedIfExists(stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return finalizeBackupMissing(ctx, stageContext, prepared)
    }

    /**
     * Issue #667：不再需要 vacateCommitted。
     * 直接推进到 MANIFEST_OLD_VACATED 状态。
     */
    private fun finalizeBackupMissing(
        ctx: ManifestTransactionContext,
        stageContext: ManifestStageContext,
        prepared: MirrorFileRef,
    ): ManifestBackupOutcome {
        val manifestBackupRef = prepared
        var currentJournal =
            stageContext.currentJournal.copy(
                manifestBackupRef = manifestBackupRef,
                manifestSwapState = ManifestTransactionState.MANIFEST_BACKUP_READY,
            )
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            deleteStagedIfExists(stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        // Issue #667：不再需要 vacateCommitted，直接推进到 MANIFEST_OLD_VACATED
        currentJournal = currentJournal.copy(manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED)
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            deleteStagedIfExists(stageContext.staged)
            return ManifestBackupOutcome.Aborted
        }
        return ManifestBackupOutcome.Proceed(manifestBackupRef, currentJournal)
    }

    private companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val MIME_JSON = "application/json"
    }
}
