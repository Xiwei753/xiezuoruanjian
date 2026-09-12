package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot

/**
 * 从 MirrorPublishExecutor 提取，只负责 manifest 事务性写入逻辑。
 *
 * Issue #667：manifest 最终位置改为私有目录（通过 [MirrorTransactionWorkspace.writeManifest]），
 * 不再出现在 Download/Sujian/_meta/ 中。stateStore 中的 manifestUri 指向私有文件路径。
 *
 * 包括：manifest 目标构建、stage、backup、promote、setManifestUri 和 commit。
 * 准备与备份阶段委托给 [MirrorManifestPrepareExecutor]。
 */
internal class MirrorManifestTransactionExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val planner: MirrorPublishPlanner,
    private val workspace: MirrorTransactionWorkspace,
) {
    private val prepareExecutor = MirrorManifestPrepareExecutor(stateStore, journalWriter, planner, workspace)

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

    /**
     * Issue #667：manifest promote 到私有目录。
     *
     * 从 workspace staging 读取 manifest 内容，用 [MirrorTransactionWorkspace.writeManifest] 原子写入。
     */
    private fun promoteManifestStaged(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        stageContext: MirrorManifestPrepareExecutor.ManifestStageContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        // Issue #667：检查私有目录中是否已有 manifest（恢复场景）
        val currentContent = workspace.readManifest()
        if (currentContent != null) {
            return handlePromoteManifestFound(ctx, backupOutcome, currentContent)
        }
        return handlePromoteManifestMissing(ctx, stageContext.staged, backupOutcome)
    }

    /**
     * 私有目录中已有 manifest — 校验是否是目标新 manifest。
     */
    private fun handlePromoteManifestFound(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
        currentContent: String,
    ): ManifestPromoteOutcome {
        var currentJournal = backupOutcome.currentJournal
        val desiredNewHash = currentJournal.manifestNewContentHash
        if (desiredNewHash == null) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: manifest exists but no expected hash, keeping journal")
            return ManifestPromoteOutcome.Aborted
        }
        val currentHash = computeContentHash(currentContent)
        if (currentHash != desiredNewHash) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: manifest hash mismatch, state unknown, keeping journal")
            return ManifestPromoteOutcome.Aborted
        }
        // 私有目录中已是新 manifest → setManifestUri 指向私有文件路径
        val manifestPath = workspace.manifestFile().absolutePath
        if (!stateStore.setManifestUri(manifestPath)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed (manifest already new)")
            return ManifestPromoteOutcome.Aborted
        }
        val newRef = MirrorFileRef(uri = manifestPath, relativePath = ctx.manifestRelativePath)
        currentJournal =
            currentJournal.copy(
                manifestNewRef = newRef,
                isManifestCommitted = true,
                manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
            )
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            return ManifestPromoteOutcome.Aborted
        }
        return ManifestPromoteOutcome.Completed(ManifestTransactionResult(currentJournal, newRef))
    }

    /**
     * 私有目录中没有 manifest — 从 workspace staging 读取内容并写入。
     */
    private fun handlePromoteManifestMissing(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        staged: StagedMirrorRef?,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        var currentJournal = backupOutcome.currentJournal
        // Issue #667：从 workspace staging 读取 manifest 内容
        val manifestContent =
            if (staged != null) {
                workspace.readStaged(staged)
            } else {
                // 没有 staged ref，尝试从 journal 的 manifestTargetJson 获取
                currentJournal.manifestTargetJson
            }
        if (manifestContent == null) {
            // 无法获取 manifest 内容，尝试从 backup 恢复
            backupOutcome.backupRef?.let { backupRef ->
                val backupContent = workspace.readBackup(backupRef)
                if (backupContent != null) {
                    workspace.writeManifest(backupContent)
                }
            }
            prepareExecutor.deleteStagedIfExists(staged)
            return ManifestPromoteOutcome.Aborted
        }
        // Issue #667：原子写入 manifest 到私有目录
        if (!workspace.writeManifest(manifestContent)) {
            // 写入失败，尝试从 backup 恢复
            backupOutcome.backupRef?.let { backupRef ->
                val backupContent = workspace.readBackup(backupRef)
                if (backupContent != null) {
                    workspace.writeManifest(backupContent)
                }
            }
            prepareExecutor.deleteStagedIfExists(staged)
            return ManifestPromoteOutcome.Aborted
        }
        val manifestPath = workspace.manifestFile().absolutePath
        val newRef = MirrorFileRef(uri = manifestPath, relativePath = ctx.manifestRelativePath)
        currentJournal =
            currentJournal.copy(
                manifestNewRef = newRef,
                manifestSwapState = ManifestTransactionState.MANIFEST_PROMOTED,
            )
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            // journal 写入失败，回滚 manifest
            workspace.deleteManifest()
            backupOutcome.backupRef?.let { backupRef ->
                val backupContent = workspace.readBackup(backupRef)
                if (backupContent != null) {
                    workspace.writeManifest(backupContent)
                }
            }
            return ManifestPromoteOutcome.Aborted
        }
        return ManifestPromoteOutcome.Proceed(newRef, currentJournal)
    }

    /**
     * Issue #667：manifest commit — setManifestUri 指向私有文件路径。
     */
    private fun commitManifestTransaction(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        promoteOutcome: ManifestPromoteOutcome.Proceed,
        manifestBackupRef: MirrorFileRef?,
    ): ManifestTransactionResult? {
        val newRef = promoteOutcome.newRef
        var currentJournal = promoteOutcome.currentJournal
        // Issue #667：setManifestUri 指向私有文件路径
        if (!stateStore.setManifestUri(newRef.uri)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed")
            // 回滚：删除新 manifest，从 backup 恢复旧 manifest
            workspace.deleteManifest()
            manifestBackupRef?.let { backupRef ->
                val backupContent = workspace.readBackup(backupRef)
                if (backupContent != null) {
                    workspace.writeManifest(backupContent)
                }
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
    }
}
