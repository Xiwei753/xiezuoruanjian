package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.interop.diagnostics.DiagnosticsInterop
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
        // Issue #717 评论 5741567193 A 部分：本次冻结的 committed manifest hash，
        // 供 resolveManifestOldIdentity 不再 fallback 到公开 _meta/manifest.json。
        val committedManifestHash: String? = null,
        // Issue #717 评论 5741567193 A 部分：old-baseline 存在状态。
        // false 表示首次发布（无旧 manifest）；true 表示有 committed baseline。
        val oldBaselineExists: Boolean = false,
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
                // Issue #717 评论 5741567193 A 部分：传递冻结的 committed baseline 身份。
                committedManifestHash = params.committedManifestHash,
                oldBaselineExists = params.oldBaselineExists,
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
     * Issue #726 评论 5750735839：promote 时私有目录中 manifest 存在且 `hash == oldHash`
     * 是**正常更新中间状态**（prepare 阶段 `handleBackupFoundNeedsVacate` 不 vacate
     * private manifest），不是异常。`handlePromoteManifestFound` 据此做三态判断。
     */
    private fun promoteManifestStaged(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        stageContext: MirrorManifestPrepareExecutor.ManifestStageContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        // Issue #667：检查私有目录中是否已有 manifest（恢复场景）
        val currentContent = workspace.readManifest()
        if (currentContent != null) {
            return handlePromoteManifestFound(ctx, stageContext, backupOutcome, currentContent)
        }
        return handlePromoteManifestMissing(ctx, stageContext, backupOutcome)
    }

    /**
     * 私有目录中已有 manifest — 三态判断（Issue #726 评论 5750735839）。
     *
     * 1. `currentHash == newContentHash`：已 promote，幂等恢复（setManifestUri / commit），
     *    返回 [ManifestPromoteOutcome.Completed]。
     * 2. `currentHash == oldContentHash`：正常更新中间状态（prepare 不 vacate private
     *    manifest），走 [atomicPromoteManifest] 原子覆盖，返回
     *    [ManifestPromoteOutcome.Proceed]。
     * 3. `currentHash` 同时不等于 `oldContentHash` 和 `newContentHash`：真正的 unknown
     *    state，保留 journal 并 [ManifestPromoteOutcome.Aborted]。
     */
    private fun handlePromoteManifestFound(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        stageContext: MirrorManifestPrepareExecutor.ManifestStageContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
        currentContent: String,
    ): ManifestPromoteOutcome {
        val newHash = stageContext.newContentHash
        val oldHash = stageContext.oldContentHash
        val currentHash = computeContentHash(currentContent)
        // 状态 1：currentHash == newHash → 幂等恢复
        if (currentHash == newHash) {
            return completeIdempotentRecovery(ctx, backupOutcome)
        }
        // 状态 2：currentHash == oldHash → 正常更新中间状态，atomic promote
        if (oldHash != null && currentHash == oldHash) {
            DiagnosticsInterop.i(
                TAG,
                "Manifest transaction: currentHash == oldHash (normal mid-state), performing atomic promote",
            )
            return atomicPromoteManifest(ctx, stageContext, backupOutcome)
        }
        // 状态 3：currentHash 既非 oldHash 也非 newHash → 真正 unknown state
        DiagnosticsInterop.w(
            TAG,
            "Manifest transaction: manifest hash is neither oldHash nor newHash, state unknown, keeping journal",
        )
        return ManifestPromoteOutcome.Aborted
    }

    /**
     * 幂等恢复路径：`currentHash == newHash`，setManifestUri + commit，返回
     * [ManifestPromoteOutcome.Completed]。
     *
     * 从原 `handlePromoteManifestFound` 的 `currentHash == desiredNewHash` 分支提取，
     * 行为不变：setManifestUri 指向私有文件路径，写 `MANIFEST_COMMITTED` journal。
     */
    private fun completeIdempotentRecovery(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        var currentJournal = backupOutcome.currentJournal
        // 私有目录中已是新 manifest → setManifestUri 指向私有文件路径
        val manifestPath = workspace.manifestFile().absolutePath
        if (!stateStore.setManifestUri(manifestPath)) {
            DiagnosticsInterop.w(TAG, "Manifest transaction: setManifestUri failed (manifest already new)")
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
     * 私有目录中没有 manifest — 走共享的 [atomicPromoteManifest] 路径。
     *
     * Issue #726 评论 5750735839：与 `handlePromoteManifestFound` 的
     * `currentHash == oldHash` 分支共用 [atomicPromoteManifest]，避免两套 promote 语义。
     */
    private fun handlePromoteManifestMissing(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        stageContext: MirrorManifestPrepareExecutor.ManifestStageContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome = atomicPromoteManifest(ctx, stageContext, backupOutcome)

    /**
     * Issue #726 评论 5750735839：共享的原子 promote manifest 函数。
     *
     * 从 staged ref（没有时用 journal 的 `manifestTargetJson`）读取目标 JSON，
     * 用 [MirrorTransactionWorkspace.writeManifest] 原子覆盖私有 `manifest.json`，
     * 重新读取并校验 `hash == newContentHash`，写 `MANIFEST_PROMOTED` journal
     * （设置 `manifestNewRef`），返回 [ManifestPromoteOutcome.Proceed]。
     *
     * 失败时从 backup 恢复并返回 [ManifestPromoteOutcome.Aborted]。
     *
     * 被 [handlePromoteManifestMissing]（manifest 缺失）和
     * [handlePromoteManifestFound] 的 `currentHash == oldHash` 分支共用，
     * 避免两套 promote 语义。
     */
    private fun atomicPromoteManifest(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        stageContext: MirrorManifestPrepareExecutor.ManifestStageContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): ManifestPromoteOutcome {
        var currentJournal = backupOutcome.currentJournal
        val staged = stageContext.staged
        // 1. 读取目标 manifest JSON：优先 staged ref，其次 journal 的 manifestTargetJson
        val manifestContent =
            if (staged != null) {
                workspace.readStaged(staged)
            } else {
                // 没有 staged ref，尝试从 journal 的 manifestTargetJson 获取
                currentJournal.manifestTargetJson
            }
        if (manifestContent == null) {
            // 无法获取 manifest 内容，尝试从 backup 恢复
            restoreManifestFromBackup(backupOutcome.backupRef)
            prepareExecutor.deleteStagedIfExists(staged)
            return ManifestPromoteOutcome.Aborted
        }
        // 2. 原子写入 manifest 到私有目录
        if (!workspace.writeManifest(manifestContent)) {
            // 写入失败，尝试从 backup 恢复
            restoreManifestFromBackup(backupOutcome.backupRef)
            prepareExecutor.deleteStagedIfExists(staged)
            return ManifestPromoteOutcome.Aborted
        }
        // 3. Issue #726 评论 5750735839：原子覆盖后重新读取并校验 hash == newContentHash
        val writtenContent = workspace.readManifest()
        if (writtenContent == null) {
            DiagnosticsInterop.w(TAG, "Manifest transaction: atomic write succeeded but re-read returned null")
            restoreManifestFromBackup(backupOutcome.backupRef)
            prepareExecutor.deleteStagedIfExists(staged)
            return ManifestPromoteOutcome.Aborted
        }
        val writtenHash = computeContentHash(writtenContent)
        if (writtenHash != stageContext.newContentHash) {
            DiagnosticsInterop.w(
                TAG,
                "Manifest transaction: atomic write hash verification failed, restoring from backup",
            )
            restoreManifestFromBackup(backupOutcome.backupRef)
            prepareExecutor.deleteStagedIfExists(staged)
            return ManifestPromoteOutcome.Aborted
        }
        // 4. 写 MANIFEST_PROMOTED journal（设置 manifestNewRef）
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
            restoreManifestFromBackup(backupOutcome.backupRef)
            return ManifestPromoteOutcome.Aborted
        }
        return ManifestPromoteOutcome.Proceed(newRef, currentJournal)
    }

    /**
     * 从 backup 恢复 manifest 内容到私有目录（promote 失败时的回滚辅助）。
     */
    private fun restoreManifestFromBackup(backupRef: MirrorFileRef?) {
        backupRef?.let { ref ->
            val backupContent = workspace.readBackup(ref)
            if (backupContent != null) {
                workspace.writeManifest(backupContent)
            }
        }
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
            DiagnosticsInterop.w(TAG, "Manifest transaction: setManifestUri failed")
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
            DiagnosticsInterop.w(TAG, "Manifest transaction: MANIFEST_COMMITTED journal write failed")
        }
        return ManifestTransactionResult(currentJournal, newRef)
    }

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val META_DIR = "_meta"
        private const val MANIFEST_FILE_NAME = "manifest.json"
    }
}
