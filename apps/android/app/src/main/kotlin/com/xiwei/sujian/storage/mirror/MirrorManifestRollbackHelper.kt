package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * MirrorManifestRollbackHelper — manifest rollback 的 phase 方法集合。
 *
 * Issue #667：manifest 在 [MirrorTransactionWorkspace]（私有目录）中，
 * rollback 时用 workspace 方法操作 manifest；Download 中不再有 manifest 文件。
 *
 * 从 MirrorRollbackExecutor 拆出，负责 manifest 回滚的多分支状态机：
 * 删除新 manifest → 恢复旧 manifest backup → setManifestUri → 写 journal。
 * 拆类避免 MirrorRollbackExecutor 触发 LargeClass / TooManyFunctions。
 */
internal class MirrorManifestRollbackHelper(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val workspace: MirrorTransactionWorkspace,
) {
    /**
     * Manifest rollback 入口（#649 评论 5565067997 修复 3）。
     *
     * Issue #667：manifest 在私有目录中，回滚操作：
     * 1. 如果新 manifest 已写入私有目录（workspace.writeManifest），用 workspace.deleteManifest() 删除
     * 2. 从 workspace backup 恢复旧 manifest 内容
     * 3. setManifestUri 指向私有目录中的 manifest 文件路径
     *
     * @return true 表示 manifest rollback 成功（或不需要 rollback）；false 表示失败
     */
    internal fun rollbackManifest(
        journalContext: PendingMirrorPublish?,
        storage: ReadableMirrorStorage,
        items: Map<ChapterKey, PendingItem>,
    ): Boolean {
        if (journalContext == null) return true
        if (journalContext.manifestTargetJson == null) return true
        if (journalContext.manifestOldRef != null && journalContext.manifestOldContentHash == null) {
            DiagnosticsLogger.w(
                TAG,
                "rollbackManifest: manifestOldRef exists but manifestOldContentHash is null, " +
                    "unknown state, keeping journal",
            )
            return false
        }
        val manifestRelativePath = "$META_DIR/$MANIFEST_FILE_NAME"
        val backup =
            when (val r = workspace.lookupBackup(journalContext.txId, manifestRelativePath)) {
                is MirrorLookupResult.Found -> r.ref
                is MirrorLookupResult.Missing -> null
                is MirrorLookupResult.Failed -> {
                    DiagnosticsLogger.w(
                        TAG,
                        "rollback manifest: lookupBackup failed: ${r.cause?.message}, keeping journal",
                    )
                    return false
                }
            }
        val ctx =
            ManifestRollbackContext(
                journalContext = journalContext,
                storage = storage,
                items = items,
                manifestRelativePath = manifestRelativePath,
            )

        // 步骤 1：处理私有目录中的 manifest（新 manifest 已写入或旧 manifest 仍在）
        var currentJournal: PendingMirrorPublish = journalContext
        when (val step1 = handleManifestForRollback(ctx, currentJournal)) {
            is RollbackManifestStep.Continue -> currentJournal = step1.journal
            RollbackManifestStep.Done -> return true
            RollbackManifestStep.Failed -> return false
        }

        // 步骤 2：恢复旧 manifest
        return rollbackOldManifest(ctx, currentJournal, backup, journalContext.manifestOldRef)
    }

    /**
     * Manifest rollback 各 phase 共享的只读上下文。
     */
    private data class ManifestRollbackContext(
        val journalContext: PendingMirrorPublish,
        val storage: ReadableMirrorStorage,
        val items: Map<ChapterKey, PendingItem>,
        val manifestRelativePath: String,
    )

    /**
     * 步骤 1：处理私有目录中的 manifest 文件。
     *
     * Issue #667：manifest 在私有目录中，用 workspace.readManifest() 读取当前内容，
     * 判断是旧 manifest 还是新 manifest。
     */
    private fun handleManifestForRollback(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
    ): RollbackManifestStep {
        val currentManifestContent = workspace.readManifest()
        if (currentManifestContent == null) {
            // 私有目录中没有 manifest 文件
            return handleManifestMissing(ctx, currentJournal)
        }
        // 读取 manifest 内容并计算 hash，判断是旧还是新
        val currentHash = computeContentHash(currentManifestContent)
        return when {
            currentHash == ctx.journalContext.manifestNewContentHash ->
                handleManifestMatchesNew(ctx, currentJournal)
            currentHash == ctx.journalContext.manifestOldContentHash ->
                handleManifestMatchesOld(ctx, currentJournal)
            else -> {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback manifest: current manifest hash matches neither old nor new, state unknown",
                )
                RollbackManifestStep.Failed
            }
        }
    }

    /**
     * 当前 manifest 是本事务新 manifest → 删除它。
     */
    private fun handleManifestMatchesNew(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
    ): RollbackManifestStep {
        if (!workspace.deleteManifest()) {
            DiagnosticsLogger.w(TAG, "rollback manifest: delete new manifest failed")
            return RollbackManifestStep.Failed
        }
        // 写 MANIFEST_ROLLBACK_NEW_REMOVED
        val nextJournal =
            currentJournal.copy(
                manifestNewRef = null,
                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_NEW_REMOVED,
            )
        if (!writeRollbackJournal(
                nextJournal,
                ctx.items,
                ManifestTransactionState.MANIFEST_ROLLBACK_NEW_REMOVED,
                manifestNewRef = null,
            )
        ) {
            DiagnosticsLogger.w(TAG, ROLLBACK_MANIFEST_JOURNAL_FAILED + "NEW_REMOVED")
            return RollbackManifestStep.Failed
        }
        return RollbackManifestStep.Continue(nextJournal)
    }

    /**
     * 当前 manifest 已经是 old manifest → 直接 setManifestUri，跳过删除+restore。
     */
    private fun handleManifestMatchesOld(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
    ): RollbackManifestStep {
        // manifest 已在私有目录中且是旧 manifest，setManifestUri 指向私有文件路径
        val manifestPath = workspace.manifestFile().absolutePath
        if (!stateStore.setManifestUri(manifestPath)) {
            DiagnosticsLogger.w(TAG, "rollback manifest: setManifestUri failed (manifest already old)")
            return RollbackManifestStep.Failed
        }
        val nextJournal =
            currentJournal.copy(
                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
            )
        if (!writeRollbackJournal(nextJournal, ctx.items, ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED)) {
            DiagnosticsLogger.w(
                TAG,
                ROLLBACK_MANIFEST_JOURNAL_FAILED + "OLD_RESTORED (manifest already old)",
            )
            return RollbackManifestStep.Failed
        }
        return RollbackManifestStep.Done
    }

    /**
     * 私有目录中没有 manifest 文件。
     */
    private fun handleManifestMissing(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
    ): RollbackManifestStep {
        // manifest 不存在，无需删除新 manifest
        if (ctx.journalContext.manifestNewRef != null) {
            val nextJournal =
                currentJournal.copy(
                    manifestNewRef = null,
                    manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_NEW_REMOVED,
                )
            if (!writeRollbackJournal(
                    nextJournal,
                    ctx.items,
                    ManifestTransactionState.MANIFEST_ROLLBACK_NEW_REMOVED,
                    manifestNewRef = null,
                )
            ) {
                DiagnosticsLogger.w(
                    TAG,
                    ROLLBACK_MANIFEST_JOURNAL_FAILED + "NEW_REMOVED (manifest missing)",
                )
                return RollbackManifestStep.Failed
            }
            return RollbackManifestStep.Continue(nextJournal)
        }
        return RollbackManifestStep.Continue(currentJournal)
    }

    private fun rollbackOldManifest(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
        backup: MirrorFileRef?,
        manifestOldRef: MirrorFileRef?,
    ): Boolean {
        // 2. 按是否有旧 manifest 分两条路径
        if (manifestOldRef == null) {
            return rollbackOldManifestWithoutOld(ctx, currentJournal)
        }

        // 有旧 manifest 的回滚：需要从 workspace backup 恢复
        if (backup == null) {
            return rollbackWithMissingBackup(ctx, currentJournal)
        }
        return rollbackWithBackup(ctx, currentJournal, backup)
    }

    private fun rollbackOldManifestWithoutOld(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
    ): Boolean {
        // #649 评论 5572554935 问题 4：首次发布 manifest（无旧 manifest）的回滚。
        if (!stateStore.clearManifestUri()) {
            DiagnosticsLogger.w(TAG, "rollback manifest: clearManifestUri failed (no-old manifest rollback)")
            return false
        }
        // 删除私有目录中可能残留的新 manifest
        workspace.deleteManifest()
        val nextJournal =
            currentJournal.copy(
                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
            )
        if (!writeRollbackJournal(nextJournal, ctx.items, ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED)) {
            DiagnosticsLogger.w(TAG, ROLLBACK_MANIFEST_JOURNAL_FAILED + "OLD_RESTORED (no-old)")
            return false
        }
        return true
    }

    private fun rollbackWithMissingBackup(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
    ): Boolean {
        // Issue #667：manifest 在私有目录中。backup 缺失时检查私有目录中是否有旧 manifest。
        val currentContent = workspace.readManifest()
        if (currentContent == null) {
            // 私有目录中没有 manifest，backup 也缺失：old manifest 丢失，无法恢复
            DiagnosticsLogger.w(
                TAG,
                "rollback manifest: backup missing and manifest missing, old manifest lost, keeping journal",
            )
            return false
        }
        // 校验私有目录中的 manifest 是否是 old manifest
        val oldHash = ctx.journalContext.manifestOldContentHash
        if (oldHash != null) {
            val currentHash = computeContentHash(currentContent)
            if (currentHash == oldHash) {
                // 私有目录中确实是 old manifest → setManifestUri 并推进状态
                val manifestPath = workspace.manifestFile().absolutePath
                if (!stateStore.setManifestUri(manifestPath)) {
                    DiagnosticsLogger.w(
                        TAG,
                        "rollback manifest: setManifestUri failed (backup null, old verified in workspace)",
                    )
                    return false
                }
                val nextJournal =
                    currentJournal.copy(
                        manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                    )
                if (!writeRollbackJournal(
                        nextJournal,
                        ctx.items,
                        ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                    )
                ) {
                    DiagnosticsLogger.w(
                        TAG,
                        ROLLBACK_MANIFEST_JOURNAL_FAILED + "OLD_RESTORED (backup null, old verified)",
                    )
                    return false
                }
                return true
            }
        }
        // hash 不匹配/读取失败/无 hash：私有目录中的不是 old manifest，无法恢复
        DiagnosticsLogger.w(
            TAG,
            "rollback manifest: backup missing, workspace manifest exists but not old manifest" +
                " (hash mismatch), keeping journal",
        )
        return false
    }

    /**
     * 从 workspace backup 恢复旧 manifest。
     *
     * Issue #667：新流程：
     * 1. `workspace.readBackup()` 读取备份内容
     * 2. `workspace.writeManifest()` 原子写入旧 manifest 到私有目录
     * 3. `stateStore.setManifestUri()` 指向私有文件路径
     */
    private fun rollbackWithBackup(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
        backup: MirrorFileRef,
    ): Boolean {
        // 检查私有目录中是否已恢复（带身份校验）
        val manifestOldHash = ctx.journalContext.manifestOldContentHash
        val currentContent = workspace.readManifest()
        if (currentContent != null) {
            val currentHash = computeContentHash(currentContent)
            if (manifestOldHash != null && currentHash == manifestOldHash) {
                // 私有目录中已是旧 manifest → setManifestUri 并推进状态
                val manifestPath = workspace.manifestFile().absolutePath
                if (!stateStore.setManifestUri(manifestPath)) {
                    DiagnosticsLogger.w(
                        TAG,
                        "rollback manifest: setManifestUri failed (already restored)",
                    )
                    return false
                }
                val nextJournal =
                    currentJournal.copy(
                        manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                    )
                if (!writeRollbackJournal(
                        nextJournal,
                        ctx.items,
                        ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                    )
                ) {
                    DiagnosticsLogger.w(
                        TAG,
                        ROLLBACK_MANIFEST_JOURNAL_FAILED + "OLD_RESTORED (already restored)",
                    )
                    return false
                }
                return true
            }
            // hash 不匹配 → 私有目录中是新 manifest，需要从 backup 恢复
        }

        // 从 workspace backup 读取旧 manifest 内容
        val backupContent = workspace.readBackup(backup)
        if (backupContent == null) {
            DiagnosticsLogger.w(TAG, "rollback manifest: read backup content failed")
            return false
        }

        // 原子写入旧 manifest 到私有目录
        if (!workspace.writeManifest(backupContent)) {
            DiagnosticsLogger.w(TAG, "rollback manifest: writeManifest failed (restoring old manifest)")
            return false
        }

        // setManifestUri 指向私有文件路径
        val manifestPath = workspace.manifestFile().absolutePath
        if (!stateStore.setManifestUri(manifestPath)) {
            DiagnosticsLogger.w(TAG, "rollback manifest: setManifestUri failed (after restoring manifest backup)")
            return false
        }
        val nextJournal =
            currentJournal.copy(
                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
            )
        if (!writeRollbackJournal(nextJournal, ctx.items, ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED)) {
            DiagnosticsLogger.w(TAG, ROLLBACK_MANIFEST_JOURNAL_FAILED + OLD_RESTORED)
            return false
        }
        return true
    }

    private fun writeRollbackJournal(
        journal: PendingMirrorPublish,
        items: Map<ChapterKey, PendingItem>,
        manifestSwapState: ManifestTransactionState,
        manifestNewRef: MirrorFileRef? = journal.manifestNewRef,
    ): Boolean =
        journalWriter.writePendingPublishJournal(
            PendingJournalParams(
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
                manifestNewRef = manifestNewRef,
                manifestBackupRef = journal.manifestBackupRef,
                isManifestCommitted = journal.isManifestCommitted,
                manifestSwapState = manifestSwapState,
                manifestNewContentHash = journal.manifestNewContentHash,
                manifestOldContentHash = journal.manifestOldContentHash,
                journalContext = journal,
            ),
        )

    private sealed interface RollbackManifestStep {
        data class Continue(val journal: PendingMirrorPublish) : RollbackManifestStep

        data object Done : RollbackManifestStep

        data object Failed : RollbackManifestStep
    }

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val META_DIR = "_meta"
        private const val MANIFEST_FILE_NAME = "manifest.json"
        private const val ROLLBACK_MANIFEST_JOURNAL_FAILED = "rollback manifest: journal write failed after "
        private const val OLD_RESTORED = "OLD_RESTORED"
    }
}
