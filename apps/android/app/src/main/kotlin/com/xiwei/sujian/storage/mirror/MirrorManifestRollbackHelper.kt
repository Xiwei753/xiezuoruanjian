package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * MirrorManifestRollbackHelper — manifest rollback 的 phase 方法集合。
 *
 * 从 MirrorRollbackExecutor 拆出，负责 manifest 回滚的多分支状态机：
 * 删除新 manifest → 恢复旧 manifest backup → setManifestUri → 写 journal。
 * 拆类避免 MirrorRollbackExecutor 触发 LargeClass / TooManyFunctions。
 */
internal class MirrorManifestRollbackHelper(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
) {
    /**
     * Manifest rollback 入口（#649 评论 5565067997 修复 3）。
     *
     * 正确顺序：
     * 1. 如果 manifestNewRef 已 promote，先删除精确的 manifestNewRef，并确认成功
     * 2. final 名字腾空
     * 3. restoreBackup(old manifest)
     * 4. 拿 restoredRef
     * 5. setManifestUri(restoredRef.uri)，失败则保留 rollback journal
     *
     * 恢复旧 manifest 后绝对不能再 resolve(final).delete()。
     *
     * #649 评论 5571899956 问题 3：manifest rollback 显式状态。
     * - 先删除新 manifest → 写 MANIFEST_ROLLBACK_NEW_REMOVED
     * - restore old backup → setManifestUri(restoredRef.uri) → 写 MANIFEST_ROLLBACK_OLD_RESTORED
     * - 如果重启时 final 已经是 old manifest，也必须先 setManifestUri(finalRef.uri) 再把 rollback 状态推进
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
            when (val r = storage.lookupBackup(journalContext.txId, manifestRelativePath)) {
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

        // 步骤 1：处理 final 上的 manifest
        var currentJournal: PendingMirrorPublish = journalContext
        when (val step1 = handleFinalManifestForRollback(ctx, currentJournal)) {
            is RollbackManifestStep.Continue -> currentJournal = step1.journal
            RollbackManifestStep.Done -> return true
            RollbackManifestStep.Failed -> return false
        }

        // 步骤 2：恢复旧 manifest
        return rollbackOldManifest(ctx, currentJournal, backup, journalContext.manifestOldRef)
    }

    /**
     * Manifest rollback 各 phase 共享的只读上下文。
     * 把 journalContext/storage/items/manifestRelativePath 打包，避免 LongParameterList。
     */
    private data class ManifestRollbackContext(
        val journalContext: PendingMirrorPublish,
        val storage: ReadableMirrorStorage,
        val items: Map<ChapterKey, PendingItem>,
        val manifestRelativePath: String,
    )

    private fun handleFinalManifestForRollback(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
    ): RollbackManifestStep {
        val finalLookup = ctx.storage.lookup(ctx.manifestRelativePath)
        return when (finalLookup) {
            is MirrorLookupResult.Found -> handleFinalManifestFound(ctx, currentJournal, finalLookup)
            is MirrorLookupResult.Missing -> handleFinalManifestMissing(ctx, currentJournal)
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(TAG, "rollback manifest: lookup final failed: ${finalLookup.cause?.message}")
                RollbackManifestStep.Failed
            }
        }
    }

    private fun handleFinalManifestFound(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
        finalLookup: MirrorLookupResult.Found,
    ): RollbackManifestStep {
        val hashResult = ctx.storage.readTextAndHash(finalLookup.ref)
        if (hashResult == null) {
            DiagnosticsLogger.w(
                TAG,
                "rollback manifest: readTextAndHash failed for final, cannot verify identity",
            )
            return RollbackManifestStep.Failed
        }
        val (_, finalHash) = hashResult
        return when {
            finalHash == ctx.journalContext.manifestNewContentHash ->
                handleFinalManifestMatchesNew(ctx, currentJournal, finalLookup)
            finalHash == ctx.journalContext.manifestOldContentHash ->
                handleFinalManifestMatchesOld(ctx, currentJournal, finalLookup)
            else -> {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback manifest: final hash matches neither old nor new, state unknown",
                )
                RollbackManifestStep.Failed
            }
        }
    }

    private fun handleFinalManifestMatchesNew(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
        finalLookup: MirrorLookupResult.Found,
    ): RollbackManifestStep {
        // final 是本事务新 manifest → 删除
        if (!ctx.storage.delete(finalLookup.ref)) {
            DiagnosticsLogger.w(TAG, "rollback manifest: delete final (new manifest) failed")
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

    private fun handleFinalManifestMatchesOld(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
        finalLookup: MirrorLookupResult.Found,
    ): RollbackManifestStep {
        // final 已经是 old manifest → 直接 setManifestUri，跳过删除+restore
        if (!stateStore.setManifestUri(finalLookup.ref.uri)) {
            DiagnosticsLogger.w(TAG, "rollback manifest: setManifestUri failed (final already old)")
            return RollbackManifestStep.Failed
        }
        val nextJournal =
            currentJournal.copy(
                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
            )
        if (!writeRollbackJournal(nextJournal, ctx.items, ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED)) {
            DiagnosticsLogger.w(
                TAG,
                ROLLBACK_MANIFEST_JOURNAL_FAILED + "OLD_RESTORED (final already old)",
            )
            return RollbackManifestStep.Failed
        }
        return RollbackManifestStep.Done
    }

    private fun handleFinalManifestMissing(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
    ): RollbackManifestStep {
        // final 已空，无需删除新 manifest
        // 如果 manifestNewRef 不为 null（journal 记录过），仍写一次 NEW_REMOVED 推进状态
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
                    ROLLBACK_MANIFEST_JOURNAL_FAILED + "NEW_REMOVED (final missing)",
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

        // 有旧 manifest 的回滚：需要 restoreBackup
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
        val oldStillInFinal = ctx.storage.lookup(ctx.manifestRelativePath)
        return when (oldStillInFinal) {
            is MirrorLookupResult.Found -> rollbackWithMissingBackupFound(ctx, currentJournal, oldStillInFinal)
            is MirrorLookupResult.Missing -> {
                // final Missing 且 backup Missing：old manifest 丢失，无法恢复，保留 journal
                DiagnosticsLogger.w(
                    TAG,
                    "rollback manifest: backup missing and final missing, old manifest lost, keeping journal",
                )
                false
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback manifest: lookup final failed (backup missing): ${oldStillInFinal.cause?.message}, keeping journal",
                )
                false
            }
        }
    }

    private fun rollbackWithMissingBackupFound(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
        oldStillInFinal: MirrorLookupResult.Found,
    ): Boolean {
        // final 上有文件，校验是否是 old manifest（用冻结的 hash 校验身份）
        val oldHash = ctx.journalContext.manifestOldContentHash
        if (oldHash != null) {
            val hashResult = ctx.storage.readTextAndHash(oldStillInFinal.ref)
            if (hashResult != null && hashResult.second == oldHash) {
                // final 上确实是 old manifest → setManifestUri 并推进状态
                if (!stateStore.setManifestUri(oldStillInFinal.ref.uri)) {
                    DiagnosticsLogger.w(
                        TAG,
                        "rollback manifest: setManifestUri failed (backup null, old verified in final)",
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
        // hash 不匹配/读取失败/无 hash：final 上不是 old manifest，无法恢复，保留 journal
        DiagnosticsLogger.w(
            TAG,
            "rollback manifest: backup missing, final exists but not old manifest (hash mismatch/read fail), keeping journal",
        )
        return false
    }

    private fun rollbackWithBackup(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
        backup: MirrorFileRef,
    ): Boolean {
        // 2. 检查 final 是否已恢复（带身份校验，#649 评论 5566303837 问题 3）
        //    #649 评论 5573310799 问题 4：用 restoredFinalLookup 避免与"1."块的 finalLookup 冲突。
        //    走到这里时 final 要么 Missing 要么已被"1."块删成 Missing，Found 分支作为幂等兜底保留。
        val manifestOldHash = ctx.journalContext.manifestOldContentHash
        val restoredFinalLookup = ctx.storage.lookup(ctx.manifestRelativePath)
        when (restoredFinalLookup) {
            is MirrorLookupResult.Found -> {
                val earlyResult = checkRestoredFinalForBackup(ctx, currentJournal, restoredFinalLookup, manifestOldHash)
                if (earlyResult != null) return earlyResult
                // null 表示需要继续 restore
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback manifest: lookup final failed: ${restoredFinalLookup.cause?.message}",
                )
                return false
            }
            is MirrorLookupResult.Missing -> {
                // final 不存在，继续恢复
            }
        }
        // 3. restoreBackup(old manifest) → 拿 restoredRef
        val restoreResult = ctx.storage.restoreBackup(backup, ctx.manifestRelativePath, MIME_JSON, manifestOldHash)
        return applyManifestRestoreResult(ctx, currentJournal, restoreResult)
    }

    /**
     * 返回非 null 表示已决定最终结果（true/false）；返回 null 表示需要继续 restore。
     */
    private fun checkRestoredFinalForBackup(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
        restoredFinalLookup: MirrorLookupResult.Found,
        manifestOldHash: String?,
    ): Boolean? {
        // final 已存在，需要校验是否真的是旧 manifest
        if (manifestOldHash != null) {
            val hashResult = ctx.storage.readTextAndHash(restoredFinalLookup.ref)
            if (hashResult != null) {
                val (_, hash) = hashResult
                if (hash == manifestOldHash) {
                    // hash 匹配 → 真的是旧 manifest，必须先 setManifestUri 再推进状态
                    DiagnosticsLogger.i(TAG, "rollback manifest: final already restored, verified by hash")
                    return finalizeManifestRestored(
                        ctx,
                        currentJournal,
                        restoredFinalLookup.ref.uri,
                        "already restored",
                    )
                }
                // hash 不匹配 → final 上是新 manifest 拮留，继续 restore
            }
            // 读取失败 → 无法确认状态，继续 restore
            return null
        }
        // 无 hash 校验 → 文件已存在视为已恢复，但还是要 setManifestUri
        DiagnosticsLogger.i(TAG, "rollback manifest: final already exists, skipping restore")
        return finalizeManifestRestored(ctx, currentJournal, restoredFinalLookup.ref.uri, "already exists")
    }

    private fun applyManifestRestoreResult(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
        restoreResult: RestoreBackupResult,
    ): Boolean =
        when (restoreResult) {
            is RestoreBackupResult.Restored ->
                finalizeManifestRestored(ctx, currentJournal, restoreResult.ref.uri, "after restoring manifest backup")
            is RestoreBackupResult.AlreadyRestored ->
                finalizeManifestRestored(ctx, currentJournal, restoreResult.ref.uri, "already restored")
            is RestoreBackupResult.Conflict -> {
                DiagnosticsLogger.w(TAG, "rollback manifest: conflict - final has wrong content")
                false
            }
            is RestoreBackupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback manifest: failed to restore manifest backup: ${restoreResult.cause?.message}",
                )
                false
            }
        }

    /**
     * setManifestUri + 写 MANIFEST_ROLLBACK_OLD_RESTORED journal 的公共尾部。
     * [failureReason] 用于 setManifestUri 失败日志。
     */
    private fun finalizeManifestRestored(
        ctx: ManifestRollbackContext,
        currentJournal: PendingMirrorPublish,
        restoredUri: String,
        failureReason: String,
    ): Boolean {
        if (!stateStore.setManifestUri(restoredUri)) {
            DiagnosticsLogger.w(TAG, "rollback manifest: setManifestUri failed ($failureReason)")
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
        private const val MIME_JSON = "application/json"
        private const val ROLLBACK_MANIFEST_JOURNAL_FAILED = "rollback manifest: journal write failed after "
        private const val OLD_RESTORED = "OLD_RESTORED"
    }
}
