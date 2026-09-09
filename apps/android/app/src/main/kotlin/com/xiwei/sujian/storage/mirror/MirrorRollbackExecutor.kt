package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * MirrorRollbackExecutor — 发布事务回滚执行器。
 *
 * 从 ReadableMirrorPublisher 提取，负责回滚单个章节和整个发布事务。
 */
internal class MirrorRollbackExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
) {
    private val manifestHelper = MirrorManifestRollbackHelper(stateStore, journalWriter)

    internal sealed interface RollbackItemResult {
        /**
         * 旧正文已恢复到 final 位置（#649 评论 5572554935 问题 2）。
         *
         * [ref] 是 restoreBackup 返回的真实 ref（URI 可能因 createText/createDocument 变化）。
         * 调用方必须把此 ref 写回 stateStore，否则 stateStore 仍保存失效 URI。
         */
        data class Restored(val ref: MirrorFileRef) : RollbackItemResult

        /**
         * 新建章节的本事务新文件已删除（无旧正文需要恢复）。
         */
        data object NewFileRemoved : RollbackItemResult

        data object StateUnknown : RollbackItemResult

        data object Failed : RollbackItemResult
    }


    private fun readLatestPendingForTxStrict(txId: String): LatestPending {
        val result = stateStore.readPendingPublish()
        return when (result) {
            is PendingPublishResult.NotExists -> LatestPending.NotExists
            is PendingPublishResult.Corrupted -> LatestPending.CorruptedOrMismatch
            is PendingPublishResult.Success -> {
                val journal = PendingMirrorPublish.fromJson(result.json)
                if (journal != null && journal.txId == txId) {
                    LatestPending.Found(journal)
                } else {
                    LatestPending.CorruptedOrMismatch
                }
            }
        }
    }


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
        val ctx = prepareRollbackChapterContext(journal, key, item)
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
        // backup 是 prepareBackup(txId, oldRef, ...) 按 oldRef.relativePath 建的，
        // 章节标题/卷名变化后 oldRef.relativePath != staged.finalRelativePath，
        // rollback 去新路径查 backup 必然找不到。
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
    ): RestoreBackupResult = restoreBackupToFinal(
        journal = ctx.journal,
        key = ctx.key,
        newFinalPath = ctx.newFinalPath,
        oldFinalPath = ctx.oldFinalPath,
        expectedOldHash = ctx.expectedOldHash,
        storage = storage,
    )

    private fun mapRestoreResultToRollbackItem(
        restoreResult: RestoreBackupResult,
        key: ChapterKey,
    ): RollbackItemResult = when (restoreResult) {
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


    /**
     * 从 backup 恢复旧正文到 final 位置（[rollbackChapterToOldState] 内部调用）。
     *
     * #649 评论 5570613481 问题 1：restoreBackup 只在 final 状态明确后才调用。
     *
     * #649 评论 5572554935 问题 1：新增 [oldFinalPath] 参数，区分新文件位置和旧文件恢复位置。
     * - [newFinalPath]：本事务新文件的位置（staged.finalRelativePath，新标题路径），当前未在此 helper 使用，
     *   保留参数供未来扩展和调用方语义清晰。
     * - [oldFinalPath]：旧正文恢复位置（oldRef.relativePath，旧标题路径）。
     *   章节改标题/换卷后 oldFinalPath != newFinalPath，backup 按 oldFinalPath 建，
     *   lookupBackup/restoreBackup 必须用 oldFinalPath 才能找到 backup 并恢复到正确位置。
     *   新建章节无 oldRef 时 oldFinalPath 为 null，调用方不应进入此 helper。
     *
     * #649 评论 5572554935 问题 2：直接返回 [RestoreBackupResult]，不折叠成 Boolean，
     * 保留 Restored(ref) 里的真实 ref（URI 可能因 createText/createDocument 变化），
     * 供调用方写回 stateStore。
     */

    private fun restoreBackupToFinal(
        journal: PendingMirrorPublish,
        key: ChapterKey,
        newFinalPath: String,
        oldFinalPath: String?,
        expectedOldHash: String?,
        storage: ReadableMirrorStorage,
    ): RestoreBackupResult {
        // backup 按 oldFinalPath 建（prepareBackup(txId, oldRef, ...)），
        // 必须用 oldFinalPath 查 backup；oldFinalPath 为 null 时回退到 newFinalPath（防御性）
        val backupLookupPath = oldFinalPath ?: newFinalPath
        val backupResult = storage.lookupBackup(journal.txId, backupLookupPath)
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
        // restoreBackup 也恢复到 oldFinalPath（旧标题路径），不能恢复到 newFinalPath
        return storage.restoreBackup(backup, backupLookupPath, MIME_MARKDOWN, expectedOldHash)
    }


    /**
     * 回滚整个发布事务：恢复所有 item 的 old backup，再删除 tx staging。
     *
     * #649 评论 5564379115 问题 1/2：统一事务回滚，替代逐 item 回滚。
     * 逐 item 回滚会在第 N 章失败时把前 N-1 章的唯一 backup 一起删掉。
     *
     * #649 评论 5564624383 问题 2：rollback 本身做成 journal 状态。
     * 进程死在回滚中间，下次是继续回滚，不会又转回 forward promote。
     * 统一顺序：
     * 0. 从磁盘读取最新 journal，只把 phase 改成 PHASE_ROLLBACK，其余字段沿用最新值
     * 1. 删除所有 promotedRef
     * 2. 逐个恢复 backupOldRef / lookupBackup() 找到的旧正文，每恢复一个更新 journal
     *    恢复前先 lookup(final) 判断是否已恢复（crash-idempotent）
     * 3. 全部恢复成功后：删除 manifest backup → manifest final → rollback(txId) → clearPendingPublish
     *
     * @param txId 事务 ID
     * @param items 当前 items（包含 backupOldRef、promotedRef）
     * @param stagedRefs staged refs（包含 finalRelativePath）
     * @param storage 当前事务的 storage
     * @param journalContext 当前 journal 上下文（用于写 rollback journal）；如果为 null 会尝试从磁盘读取
     * @return true 表示回滚成功；false 表示恢复失败或部分失败（需要后续重试）
     */

    internal suspend fun rollbackWholePublishTransaction(
        txId: String,
        items: Map<ChapterKey, PendingItem>,
        stagedRefs: Map<ChapterKey, StagedMirrorRef>,
        storage: ReadableMirrorStorage,
        journalContext: PendingMirrorPublish? = null,
    ): Boolean {
        // 0. #649 评论 5566303837 问题 1：磁盘最新状态优先
        // 不能用 journalContext 覆盖磁盘上已经前进的事务状态
        val latestJournal = resolveLatestJournalForRollback(txId, journalContext) ?: return false

        // 0.1 写 rollback journal（让进程死在回滚中间时能继续回滚）
        // 只把 phase 改成 PHASE_ROLLBACK，其余字段沿用最新 journal 值
        // 合并磁盘 journal 和调用方 items（调用方有 rollback 状态推进）
        val mergedItems = mergeRollbackItems(latestJournal, items)
        if (!writeRollbackJournalForItems(latestJournal, mergedItems)) {
            DiagnosticsLogger.w(TAG, "rollback: journal write failed at start")
            return false
        }

        // #649 评论 5564820566 问题 2：rollback 和 recovery 共用同一套显式状态机。
        // 逐项处理：每个 item 独立跟踪 rollback 进度，不靠 "final/backup 是否存在" 猜测。
        val currentItems = mergedItems.toMutableMap()

        // 1. 幂等删除 promotedRef（跳过已处理的 item）
        if (!rollbackDeletePromotedRefs(latestJournal, currentItems, storage)) return false

        // 2. 逐个恢复 backupOldRef 到最终路径
        if (!rollbackRestoreOldBackups(latestJournal, currentItems, storage)) return false

        // 3. 全部旧正文恢复成功
        //    #649 评论 5565067997 修复 3：manifest rollback 顺序修正。
        //    正确顺序：1.先删 manifestNewRef → 2.final 腾空 → 3.restoreBackup →
        //    4.setManifestUri → 5.清 staging。
        //    不再先 restoreBackup 再 resolve(final).delete()（会删掉刚恢复的旧 manifest）。
        // #649 评论 5573310799 问题 2：传 currentItems（已推进到 STATE_ROLLBACK_OLD_RESTORED 的最新状态），
        //    不传 mergedItems（正文回滚前的旧状态）。
        if (!rollbackManifest(latestJournal, storage, currentItems)) {
            return false
        }

        // 4. rollback(txId) 删 staging（backup 已不在 staging 内）
        //    #649 评论 5566303837 问题 6：检查 rollback 返回值
        if (!storage.rollback(txId)) {
            DiagnosticsLogger.w(TAG, "rollback: staging cleanup failed for tx $txId, keeping journal")
            return false
        }
        stateStore.clearPendingPublish()
        return true
    }

    private fun resolveLatestJournalForRollback(
        txId: String,
        journalContext: PendingMirrorPublish?,
    ): PendingMirrorPublish? {
        // 0. #649 评论 5566303837 问题 1：磁盘最新状态优先
        // 不能用 journalContext 覆盖磁盘上已经前进的事务状态
        return when (val latest = readLatestPendingForTxStrict(txId)) {
            is LatestPending.Found -> latest.journal
            LatestPending.NotExists -> {
                // 磁盘无 journal，只能用调用方传入的 context 兜底
                journalContext
            }
            is LatestPending.CorruptedOrMismatch -> {
                DiagnosticsLogger.w(TAG, "rollback: cannot read latest journal for tx $txId (corrupted/mismatch)")
                null
            }
        }
    }

    private fun mergeRollbackItems(
        latestJournal: PendingMirrorPublish,
        items: Map<ChapterKey, PendingItem>,
    ): MutableMap<ChapterKey, PendingItem> {
        val mergedItems = latestJournal.items.toMutableMap()
        for ((key, callerItem) in items) {
            mergedItems[key] = callerItem
        }
        return mergedItems
    }

    private fun writeRollbackJournalForItems(
        journal: PendingMirrorPublish,
        items: Map<ChapterKey, PendingItem>,
    ): Boolean = journalWriter.writePendingPublishJournal(
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
            manifestNewRef = journal.manifestNewRef,
            manifestBackupRef = journal.manifestBackupRef,
            manifestSwapState = journal.manifestSwapState,
            journalContext = journal,
        ),
    )

    /**
     * 阶段 1：幂等删除 promotedRef（跳过已处理的 item）。
     * #649 评论 5565067997 修复 4：检查 delete() 返回值，失败时停止推进状态。
     */
    private fun rollbackDeletePromotedRefs(
        latestJournal: PendingMirrorPublish,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ): Boolean {
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue
            if (item.state == PendingItem.STATE_ROLLBACK_NEW_REMOVED) {
                // 新内容已删，跳过删除步骤，直接进入恢复旧内容
            } else {
                // #649 评论 5565067997 修复 4：检查 delete() 返回值
                val removed = item.promotedRef?.let { storage.delete(it) } ?: true
                if (!removed) {
                    // delete 失败：保留当前 rollback journal，停止
                    DiagnosticsLogger.w(
                        TAG,
                        "rollback: delete promotedRef failed for ${key.chapterId}, keeping journal",
                    )
                    return false
                }
                currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_NEW_REMOVED)
            }
            // 更新 journal（记录 NEW_REMOVED）
            if (!writeRollbackJournalForItems(latestJournal, currentItems)) {
                DiagnosticsLogger.w(TAG, "rollback: journal write failed after NEW_REMOVED for ${key.chapterId}")
                return false
            }
        }
        return true
    }

    /**
     * 阶段 2：逐个恢复 backupOldRef 到最终路径。
     * #649 评论 5570613481 问题 1：使用统一 helper，不再内联回滚逻辑。
     * #649 评论 5573310799 问题 1：每章恢复成功后先写 stateStore 再标记 journal，
     * 消除断电丢失窗口（journal 已写 OLD_RESTORED、stateStore 还没写真实 URI 之间，
     * 重启会跳过该 item，真实 URI 永久丢失）。
     * restoreBackup 可能因 createText/createDocument 返回新 URI，
     * stateStore 仍保存事务开始前的旧 URI（可能已被 move 到 backup 最后被 cleanup 删除），
     * 下次镜像操作会拿到失效 URI，所以必须用真实 restoredRef.uri 替换。
     */
    private fun rollbackRestoreOldBackups(
        latestJournal: PendingMirrorPublish,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ): Boolean {
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue

            val rollbackResult = rollbackChapterToOldState(latestJournal, key, item, storage)
            if (!applyRollbackItemResult(rollbackResult, latestJournal, key, item, currentItems)) {
                return false
            }

            // 更新 journal（记录 OLD_RESTORED）
            if (!writeRollbackJournalForItems(latestJournal, currentItems)) {
                DiagnosticsLogger.w(TAG, "rollback: journal write failed after OLD_RESTORED for ${key.chapterId}")
                return false
            }
        }
        return true
    }

    private fun applyRollbackItemResult(
        rollbackResult: RollbackItemResult,
        latestJournal: PendingMirrorPublish,
        key: ChapterKey,
        item: PendingItem,
        currentItems: MutableMap<ChapterKey, PendingItem>,
    ): Boolean {
        when (rollbackResult) {
            is RollbackItemResult.Restored -> {
                // #649 评论 5573310799 问题 1：先把真实 URI 写进 stateStore，再标记 journal
                val oldEntry =
                    latestJournal.oldEntries[key] ?: run {
                        DiagnosticsLogger.w(TAG, "rollback: missing oldEntry for ${key.chapterId}, keeping journal")
                        return false
                    }
                if (!stateStore.putChapterEntry(
                        key.projectId,
                        key.volumeId,
                        key.chapterId,
                        oldEntry.copy(
                            uri = rollbackResult.ref.uri,
                            relativePath = rollbackResult.ref.relativePath,
                        ),
                    )
                ) {
                    DiagnosticsLogger.w(
                        TAG,
                        "rollback: putChapterEntry failed for ${key.chapterId}, keeping journal",
                    )
                    return false
                }
                currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_OLD_RESTORED)
            }
            RollbackItemResult.NewFileRemoved -> {
                currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_OLD_RESTORED)
            }
            RollbackItemResult.StateUnknown -> {
                DiagnosticsLogger.w(TAG, "rollback: state unknown for ${key.chapterId}, keeping journal")
                return false
            }
            RollbackItemResult.Failed -> {
                DiagnosticsLogger.w(TAG, "rollback: failed for ${key.chapterId}, keeping journal")
                return false
            }
        }
        return true
    }


    /**
     * Manifest rollback 入口（委托给 [MirrorManifestRollbackHelper]）。
     *
     * [MirrorRecoveryExecutor.recoverRollbackPhase] 和 [rollbackWholePublishTransaction] 共用此方法。
     *
     * @return true 表示 manifest rollback 成功（或不需要 rollback）；false 表示失败
     */
    internal fun rollbackManifest(
        journalContext: PendingMirrorPublish?,
        storage: ReadableMirrorStorage,
        items: Map<ChapterKey, PendingItem>,
    ): Boolean = manifestHelper.rollbackManifest(journalContext, storage, items)

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val MIME_MARKDOWN = "text/markdown"
    }
}
