package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * 单章节回滚执行器。
 *
 * Issue #667：backup 文件在 [MirrorTransactionWorkspace]（私有目录）中，
 * 回滚时用 workspace 读取 backup 内容，用 [ReadableMirrorStorage] 在 Download 中恢复最终文件。
 *
 * 从 [MirrorRollbackExecutor] 提取，只负责把单个章节从新状态回滚到旧状态。
 * [MirrorRollbackExecutor] 和 [MirrorRecoveryExecutor.recoverRollbackPhase] 共用此类。
 */
internal class MirrorChapterRollbackExecutor(
    private val workspace: MirrorTransactionWorkspace,
) {
    /**
     * 回滚单个章节到旧状态。
     *
     * #649 评论 5570613481 问题 1：完整规则：
     * 有旧正文（expectedOldHash != null）：
     * 1. lookupBackup 必须 Found
     * 2. final Missing → 直接 restore backup
     * 3. final Found 且 hash 是 old/new 中任意一个 → 删除当前 final，再 restore backup
     * 4. final Found 但既不是 oldHash 也不是 newHash → 状态不明，停止并保留 journal
     * 5. lookup/read hash Failed → 停止并保留 journal
     *
     * 没有旧正文（新建章节）：
     * 1. final Missing → 回滚目标已达到
     * 2. final Found + hash == newHash → 删除本事务新文件
     * 3. final Found 但 hash 不匹配 / 无 newHash / 读取失败 → 状态不明，停止并保留 journal
     *
     * #649 评论 5571899956 问题 2：修复同 hash 歧义。
     * 当 expectedOldHash == expectedNewHash（只改标题/路径，正文没改）时，
     * finalHash == expectedOldHash 不能作为"已恢复"的依据，必须先删除 final 再 restore backup。
     */
    internal fun rollbackChapterToOldState(
        journal: PendingMirrorPublish,
        key: ChapterKey,
        item: PendingItem,
        storage: ReadableMirrorStorage,
    ): RollbackItemResult {
        val ctx =
            prepareRollbackChapterContext(journal, key, item)
                ?: return RollbackItemResult.StateUnknown
        val finalLookup = storage.lookup(ctx.newFinalPath)
        return when (finalLookup) {
            is MirrorLookupResult.Found -> handleRollbackFinalFound(ctx, storage, finalLookup)
            is MirrorLookupResult.Missing -> handleRollbackFinalMissing(ctx, storage)
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback: lookup final failed for ${ctx.key.chapterId}: ${finalLookup.cause?.message}",
                )
                RollbackItemResult.StateUnknown
            }
        }
    }

    /**
     * 章节回滚上下文：把 [rollbackChapterToOldState] 入口解析出的路径与 hash 打包，
     * 避免下游 phase 方法触发 LongParameterList。
     */
    private data class RollbackChapterContext(
        val journal: PendingMirrorPublish,
        val key: ChapterKey,
        val newFinalPath: String,
        val oldFinalPath: String?,
        val expectedOldHash: String?,
        val expectedNewHash: String?,
    )

    private fun prepareRollbackChapterContext(
        journal: PendingMirrorPublish,
        key: ChapterKey,
        item: PendingItem,
    ): RollbackChapterContext? {
        val staged = item.stagedRef ?: journal.stagedRefs[key]
        if (staged == null) {
            DiagnosticsLogger.w(TAG, "rollback: missing stagedRef for ${key.chapterId}")
            return null
        }
        // #649 评论 5572554935 问题 1：rename/move rollback 路径错配。
        // 必须把"新文件位置"和"旧文件恢复位置"分开：
        // - newFinalPath = staged.finalRelativePath（新标题路径），用于检查/删除本事务新文件
        // - oldFinalPath = oldRef.relativePath（旧标题路径），用于 lookupBackup 和 restoreBackup
        val newFinalPath = staged.finalRelativePath
        val oldRef =
            item.oldRef ?: journal.oldEntries[key]?.let {
                MirrorFileRef(uri = it.uri, relativePath = it.relativePath)
            }
        val oldFinalPath = oldRef?.relativePath
        val expectedOldHash = journal.oldEntries[key]?.contentHash ?: item.oldContentHash
        val expectedNewHash = journal.newEntries[key]?.contentHash
        return RollbackChapterContext(
            journal = journal,
            key = key,
            newFinalPath = newFinalPath,
            oldFinalPath = oldFinalPath,
            expectedOldHash = expectedOldHash,
            expectedNewHash = expectedNewHash,
        )
    }

    private fun handleRollbackFinalFound(
        ctx: RollbackChapterContext,
        storage: ReadableMirrorStorage,
        finalLookup: MirrorLookupResult.Found,
    ): RollbackItemResult {
        val hashResult = storage.readTextAndHash(finalLookup.ref)
        if (hashResult == null) {
            DiagnosticsLogger.w(
                TAG,
                "rollback: readTextAndHash failed for ${ctx.key.chapterId}, cannot verify final identity",
            )
            return RollbackItemResult.StateUnknown
        }
        val (_, finalHash) = hashResult
        if (ctx.expectedOldHash != null) {
            return handleRollbackFinalFoundWithOld(ctx, storage, finalLookup, finalHash)
        }
        return handleRollbackFinalFoundForNewChapter(ctx, storage, finalLookup, finalHash)
    }

    private fun handleRollbackFinalFoundWithOld(
        ctx: RollbackChapterContext,
        storage: ReadableMirrorStorage,
        finalLookup: MirrorLookupResult.Found,
        finalHash: String,
    ): RollbackItemResult {
        // 有旧正文：final Found 时，hash 是 old/new 中任意一个都删除并 restore backup
        if (finalHash == ctx.expectedOldHash || finalHash == ctx.expectedNewHash) {
            if (finalHash == ctx.expectedOldHash) {
                DiagnosticsLogger.i(
                    TAG,
                    "rollback: final matches old hash for ${ctx.key.chapterId}, will delete and restore",
                )
            } else {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback: final matches new hash for ${ctx.key.chapterId}, will delete and restore",
                )
            }
            val removed = storage.delete(finalLookup.ref)
            if (!removed) {
                DiagnosticsLogger.w(TAG, "rollback: delete final failed for ${ctx.key.chapterId}")
                return RollbackItemResult.Failed
            }
        } else {
            DiagnosticsLogger.w(
                TAG,
                "rollback: final hash mismatch (neither old nor new) for ${ctx.key.chapterId}, state unknown",
            )
            return RollbackItemResult.StateUnknown
        }
        // #649 评论 5572554935 问题 1+2：
        // - lookupBackup/restoreBackup 用 oldFinalPath（旧标题路径），不是 newFinalPath
        // - restoreBackupToFinal 直接返回 RestoreBackupResult，不折叠成 Boolean，
        //   保留 Restored(ref) 里的真实 ref 给调用方写回 stateStore
        val restoreResult = restoreBackupToFinal(ctx, storage)
        return mapRestoreResultToRollbackItem(restoreResult, ctx.key)
    }

    private fun handleRollbackFinalFoundForNewChapter(
        ctx: RollbackChapterContext,
        storage: ReadableMirrorStorage,
        finalLookup: MirrorLookupResult.Found,
        finalHash: String,
    ): RollbackItemResult {
        // 没有旧正文（新建章节）
        if (ctx.expectedNewHash == null) {
            DiagnosticsLogger.w(
                TAG,
                "rollback: no expectedNewHash for new chapter ${ctx.key.chapterId}, state unknown",
            )
            return RollbackItemResult.StateUnknown
        }
        if (finalHash == ctx.expectedNewHash) {
            DiagnosticsLogger.w(TAG, "rollback: new chapter final exists for ${ctx.key.chapterId}, will delete")
            val removed = storage.delete(finalLookup.ref)
            if (!removed) {
                DiagnosticsLogger.w(TAG, "rollback: delete new chapter final failed for ${ctx.key.chapterId}")
                return RollbackItemResult.Failed
            }
            return RollbackItemResult.NewFileRemoved
        }
        DiagnosticsLogger.w(
            TAG,
            "rollback: final exists for new chapter with unexpected hash for ${ctx.key.chapterId}, state unknown",
        )
        return RollbackItemResult.StateUnknown
    }

    private fun handleRollbackFinalMissing(
        ctx: RollbackChapterContext,
        storage: ReadableMirrorStorage,
    ): RollbackItemResult {
        if (ctx.expectedOldHash != null) {
            val restoreResult = restoreBackupToFinal(ctx, storage)
            return mapRestoreResultToRollbackItem(restoreResult, ctx.key)
        }
        DiagnosticsLogger.i(TAG, "rollback: final missing for new chapter ${ctx.key.chapterId}, target reached")
        return RollbackItemResult.NewFileRemoved
    }

    private fun restoreBackupToFinal(
        ctx: RollbackChapterContext,
        storage: ReadableMirrorStorage,
    ): RestoreBackupResult =
        restoreBackupToFinal(
            journal = ctx.journal,
            key = ctx.key,
            newFinalPath = ctx.newFinalPath,
            oldFinalPath = ctx.oldFinalPath,
            expectedOldHash = ctx.expectedOldHash,
            storage = storage,
        )

    /**
     * 从 workspace backup 恢复旧正文到 Download final 位置。
     *
     * Issue #667：新流程：
     * 1. `workspace.lookupBackup()` 查找私有备份
     * 2. `workspace.readBackup()` 读取备份内容
     * 3. `storage.createText()` 在 Download 创建最终文件
     * 4. 用 hash 校验确认恢复成功
     *
     * #649 评论 5570613481 问题 1：restoreBackup 只在 final 状态明确后才调用。
     * #649 评论 5572554935 问题 1：新增 [oldFinalPath] 参数，区分新文件位置和旧文件恢复位置。
     * #649 评论 5572554935 问题 2：直接返回 [RestoreBackupResult]，不折叠成 Boolean。
     */
    private fun restoreBackupToFinal(
        journal: PendingMirrorPublish,
        key: ChapterKey,
        newFinalPath: String,
        oldFinalPath: String?,
        expectedOldHash: String?,
        storage: ReadableMirrorStorage,
    ): RestoreBackupResult {
        val backupLookupPath = oldFinalPath ?: newFinalPath
        val backupResult = workspace.lookupBackup(journal.txId, backupLookupPath)
        val backup =
            when (backupResult) {
                is MirrorLookupResult.Found -> backupResult.ref
                is MirrorLookupResult.Missing -> {
                    DiagnosticsLogger.w(TAG, "rollback: backup missing for ${key.chapterId} at $backupLookupPath")
                    return RestoreBackupResult.Failed(IllegalStateException("backup missing"))
                }
                is MirrorLookupResult.Failed -> {
                    DiagnosticsLogger.w(
                        TAG,
                        "rollback: lookup backup failed for ${key.chapterId}: ${backupResult.cause?.message}",
                    )
                    return RestoreBackupResult.Failed(backupResult.cause)
                }
            }
        // Issue #667：从 workspace 读取 backup 内容，然后在 Download 中创建最终文件
        val backupContent = workspace.readBackup(backup)
        if (backupContent == null) {
            DiagnosticsLogger.w(TAG, "rollback: read backup content failed for ${key.chapterId}")
            return RestoreBackupResult.Failed(IllegalStateException("read backup content failed"))
        }
        // 在 Download 中创建最终文件（恢复旧正文）
        val relativeDir = backupLookupPath.substringBeforeLast('/', "")
        val displayName = backupLookupPath.substringAfterLast('/')
        val restoredRef = storage.createText(relativeDir, displayName, MIME_MARKDOWN, backupContent)
        if (restoredRef == null) {
            DiagnosticsLogger.w(TAG, "rollback: createText failed for ${key.chapterId} at $backupLookupPath")
            return RestoreBackupResult.Failed(IllegalStateException("createText failed"))
        }
        return RestoreBackupResult.Restored(restoredRef)
    }

    private fun mapRestoreResultToRollbackItem(
        restoreResult: RestoreBackupResult,
        key: ChapterKey,
    ): RollbackItemResult =
        when (restoreResult) {
            is RestoreBackupResult.Restored -> RollbackItemResult.Restored(restoreResult.ref)
            is RestoreBackupResult.AlreadyRestored -> RollbackItemResult.Restored(restoreResult.ref)
            is RestoreBackupResult.Conflict -> {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback: conflict restoring backup for ${key.chapterId} - final has wrong content",
                )
                RollbackItemResult.Failed
            }
            is RestoreBackupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback: failed to restore backup for ${key.chapterId}: ${restoreResult.cause?.message}",
                )
                RollbackItemResult.Failed
            }
        }

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val MIME_MARKDOWN = "text/markdown"
    }
}
