package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.storage.mirror.MirrorRollbackExecutor.RollbackItemResult

/**
 * MirrorRecoveryExecutor — pending publish 恢复执行器。
 */
internal class MirrorRecoveryExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val rollbackExecutor: MirrorRollbackExecutor,
    private val publishExecutor: MirrorPublishExecutor,
) {
    /**
     * 恢复 promote 阶段。
     *
     * #649 评论 5562462046 问题 3：只继续未完成的 item（state != COMMITTED），
     * 不能把所有 stagedRefs 从头再跑一遍。
     *
     * #649 评论 5562715833 问题 4a：跳过 STATE_PROMOTED 和 STATE_COMMITTED 两种已完成状态。
     * #649 评论 5562715833 问题 4b：manifest 成功后先写 cleanup journal 再 recoverCleanupPhase。
     * #649 评论 5562715833 问题 2：promote 拆成 backupCommitted + promoteStaged，不先删 old。
     */

    internal suspend fun recoverPromotePhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        val promotedEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        // 把已 PROMOTED 或 COMMITTED 的 item 直接收进 promotedEntries；只对未完成的 item 继续 promote。
        val currentItems = journal.items.toMutableMap()
        for ((key, item) in currentItems.toMap()) {
            val outcome = processItemForRecoverPromote(journal, key, item, storage, currentItems, promotedEntries)
            when (outcome) {
                is PromoteItemResult.Promoted -> promotedEntries[key] = outcome.entry
                PromoteItemResult.Reused -> Unit // 已复用，继续下一项
                PromoteItemResult.RollbackDone -> return // 已回滚
            }
        }
        // 写 manifest + 进入 cleanup 阶段
        finalizeRecoveryPromoteWithManifest(journal, storage, currentItems, promotedEntries)
    }

    /**
     * 单个 item 在 recoverPromotePhase 循环中的处理结果。
     */
    private sealed interface PromoteItemResult {
        /** 该 item 已 promote，[entry] 需收进 promotedEntries。 */
        data class Promoted(val entry: ChapterMirrorEntry) : PromoteItemResult
        /** 该 item 已复用已有 promotedRef，跳过后续步骤。 */
        data object Reused : PromoteItemResult
        /** 已回滚整个事务，调用方应 return。 */
        data object RollbackDone : PromoteItemResult
    }

    /**
     * recoverPromotePhase 循环内各 phase 共享的可变上下文。
     * 把 journal/key/item/storage/currentItems/promotedEntries 打包，避免 LongParameterList。
     */
    private data class RecoverPromoteItemContext(
        val journal: PendingMirrorPublish,
        val key: ChapterKey,
        val item: PendingItem,
        val storage: ReadableMirrorStorage,
        val currentItems: MutableMap<ChapterKey, PendingItem>,
        val promotedEntries: MutableMap<ChapterKey, ChapterMirrorEntry>,
    )

    private suspend fun processItemForRecoverPromote(
        journal: PendingMirrorPublish,
        key: ChapterKey,
        item: PendingItem,
        storage: ReadableMirrorStorage,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        promotedEntries: MutableMap<ChapterKey, ChapterMirrorEntry>,
    ): PromoteItemResult {
        val ctx = RecoverPromoteItemContext(journal, key, item, storage, currentItems, promotedEntries)
        // #649 评论 5562715833 问题 4a：跳过 STATE_PROMOTED 和 STATE_COMMITTED
        // #649 评论 5573750754 修复 5：至少 lookup + contentHash 校验通过后再复用真实 ref
        if ((item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
            item.promotedRef != null
        ) {
            val reuseOutcome = reusePromotedItemForRecover(ctx)
            if (reuseOutcome != null) return reuseOutcome
        }
        val staged = item.stagedRef ?: journal.stagedRefs[key]
        if (staged == null) {
            // #649 评论 5574521549 问题 4：stagedRef 缺失是事务状态不完整，停止恢复并保留 journal
            DiagnosticsLogger.w(
                TAG,
                "Recover promote: missing stagedRef for ${key.chapterId}, " +
                    "transaction state incomplete, keeping journal and stopping",
            )
            rollbackRecoveryWithCleanup(journal, currentItems, promotedEntries, storage)
            return PromoteItemResult.RollbackDone
        }
        val oldRef = item.oldRef
        // #649 评论 5562715833 问题 2：backupCommitted + promoteStaged，不先删 old
        if (oldRef != null && item.state != PendingItem.STATE_OLD_VACATED &&
            !prepareBackupForRecoverItem(ctx, oldRef)
        ) {
            return PromoteItemResult.RollbackDone
        }
        return promoteStagedForRecoverItem(ctx, staged)
    }

    /**
     * 复用已 PROMOTED/COMMITTED 的 item。返回非 null 表示已有结论（复用成功或回滚）。
     */
    private suspend fun reusePromotedItemForRecover(
        ctx: RecoverPromoteItemContext,
    ): PromoteItemResult? {
        val journal = ctx.journal
        val key = ctx.key
        val promotedLookup = ctx.storage.lookup(ctx.item.promotedRef!!.relativePath)
        return when (promotedLookup) {
            is MirrorLookupResult.Found -> {
                val expectedHash = journal.newEntries[key]?.contentHash
                if (expectedHash != null) {
                    val hashResult = ctx.storage.readTextAndHash(promotedLookup.ref)
                    if (hashResult != null && hashResult.second == expectedHash) {
                        // lookup + contentHash 校验通过，复用真实 ref
                        return PromoteItemResult.Promoted(
                            ChapterMirrorEntry(
                                uri = promotedLookup.ref.uri,
                                relativePath = promotedLookup.ref.relativePath,
                                revision = journal.newEntries[key]?.revision ?: 0L,
                                contentHash = expectedHash,
                            ),
                        )
                    }
                }
                // hash 不匹配/读取失败/无期望 hash：不继续用未知内容，保留 journal 停止
                DiagnosticsLogger.w(
                    TAG,
                    "Recover promote: promotedRef hash mismatch for ${key.chapterId}, keeping journal",
                )
                rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
                PromoteItemResult.RollbackDone
            }
            is MirrorLookupResult.Missing -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover promote: promotedRef missing for ${key.chapterId}, keeping journal",
                )
                rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
                PromoteItemResult.RollbackDone
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover promote: lookup promotedRef failed for ${key.chapterId}: " +
                        "${promotedLookup.cause?.message}, keeping journal",
                )
                rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
                PromoteItemResult.RollbackDone
            }
        }
    }

    /**
     * 阶段 3：backup 准备（lookupBackup/prepareBackup + vacateCommitted）。
     * 返回 true 表示继续；false 表示已回滚，调用方应 return。
     */
    private suspend fun prepareBackupForRecoverItem(
        ctx: RecoverPromoteItemContext,
        oldRef: MirrorFileRef,
    ): Boolean {
        val journal = ctx.journal
        val key = ctx.key
        // #649 评论 5564820566 问题 3：两步 journalable backup — recover 同样用 prepareBackup + vacateCommitted
        // 使用 lookupBackup 三态查询（#649 评论 5565862745 问题 3）
        val backupResult = ctx.storage.lookupBackup(journal.txId, oldRef.relativePath)
        val backupReady =
            when (backupResult) {
                is MirrorLookupResult.Found -> {
                    // backup 已存在，直接复用
                    val vacated = checkOldVacatedForRecover(ctx, oldRef) ?: return false
                    BackupReadyRef(backupRef = backupResult.ref, vacated = vacated)
                }
                is MirrorLookupResult.Missing -> {
                    // backup 不存在，需要 prepareBackup
                    val prepared = ctx.storage.prepareBackup(journal.txId, oldRef, MIME_MARKDOWN)
                    if (prepared == null) {
                        DiagnosticsLogger.w(TAG, "Recover backup prepare failed for ${key.chapterId}")
                        rollbackRecoveryWithCleanup(journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
                        return false
                    }
                    prepared
                }
                is MirrorLookupResult.Failed -> {
                    DiagnosticsLogger.w(
                        TAG,
                        "Recover backup: lookupBackup failed for ${key.chapterId}: ${backupResult.cause?.message}",
                    )
                    rollbackRecoveryWithCleanup(journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
                    return false
                }
            }
        // #649 评论 5565067997 修复 1：journal 先写 STATE_BACKUP_READY
        ctx.currentItems[key] = ctx.item.copy(
            backupOldRef = backupReady.backupRef,
            state = PendingItem.STATE_BACKUP_READY,
        )
        if (!writeRecoveryPromoteJournal(journal, ctx.currentItems)) {
            DiagnosticsLogger.w(
                TAG,
                "Recover backup: journal write failed (BACKUP_READY) for ${key.chapterId}, keeping journal",
            )
            rollbackRecoveryWithCleanup(journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
            return false
        }
        // vacate old（如果 prepareBackup 还没 move old）
        if (!backupReady.vacated && !ctx.storage.vacateCommitted(oldRef)) {
            DiagnosticsLogger.w(TAG, "Recover vacate failed for ${key.chapterId}")
            rollbackRecoveryWithCleanup(journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
            return false
        }
        // #649 评论 5565067997 修复 1：vacate 成功后写 STATE_OLD_VACATED
        ctx.currentItems[key] = ctx.currentItems[key]!!.copy(state = PendingItem.STATE_OLD_VACATED)
        if (!writeRecoveryPromoteJournal(journal, ctx.currentItems)) {
            DiagnosticsLogger.w(
                TAG,
                "Recover backup: journal write failed (OLD_VACATED) for ${key.chapterId}, keeping journal",
            )
            rollbackRecoveryWithCleanup(journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
            return false
        }
        return true
    }

    /**
     * 检查 old 是否已 vacate。返回 Boolean（true=已 vacate，false=未 vacate）；null 表示查询失败已回滚。
     */
    private suspend fun checkOldVacatedForRecover(
        ctx: RecoverPromoteItemContext,
        oldRef: MirrorFileRef,
    ): Boolean? {
        val oldLookup = ctx.storage.lookup(oldRef.relativePath)
        return when (oldLookup) {
            is MirrorLookupResult.Missing -> true
            is MirrorLookupResult.Found -> false
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover backup: lookup old failed for ${ctx.key.chapterId}: ${oldLookup.cause?.message}",
                )
                rollbackRecoveryWithCleanup(ctx.journal, ctx.currentItems, ctx.promotedEntries, ctx.storage)
                null
            }
        }
    }

    /**
     * 阶段 4：promote staged（lookup final + hash 校验 + promoteStaged + 写 journal）。
     */
    private suspend fun promoteStagedForRecoverItem(
        ctx: RecoverPromoteItemContext,
        staged: StagedMirrorRef,
    ): PromoteItemResult {
        val journal = ctx.journal
        val key = ctx.key
        // #649 评论 5566303837 问题 2：OLD_VACATED 崩溃窗口检查
        // #649 评论 5569598106 问题2：lookup 返回 Failed / hash 不匹配 / 无法校验时停止保留 journal
        var newRef: MirrorFileRef? = null
        if (ctx.item.state == PendingItem.STATE_OLD_VACATED) {
            val finalOutcome = checkFinalForPromoteStaged(ctx, staged)
            when (finalOutcome) {
                is FinalCheckOutcome.Reuse -> newRef = finalOutcome.ref
                FinalCheckOutcome.Proceed -> Unit // final 不存在，继续 promote
                FinalCheckOutcome.RollbackDone -> return PromoteItemResult.RollbackDone
            }
        }
        if (newRef == null) {
            newRef = ctx.storage.promoteStaged(staged, staged.finalRelativePath)
        }
        if (newRef == null) {
            DiagnosticsLogger.w(TAG, "Recover promote failed for ${key.chapterId}")
            // #649 评论 5564379115 问题 2：统一事务回滚
            rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
            return PromoteItemResult.RollbackDone
        }
        val entry = ChapterMirrorEntry(
            uri = newRef.uri,
            relativePath = newRef.relativePath,
            revision = journal.newEntries[key]?.revision ?: 0L,
            contentHash = journal.newEntries[key]?.contentHash ?: "",
        )
        // 逐项更新 journal（记录该 item 已 PROMOTED）
        ctx.currentItems[key] = ctx.currentItems[key]!!.copy(promotedRef = newRef, state = PendingItem.STATE_PROMOTED)
        // #649 评论 5563333323 缺口 2：journal 写入失败则停止
        if (!writeRecoveryPromoteJournal(journal, ctx.currentItems)) {
            DiagnosticsLogger.w(TAG, "Recover promote: journal write failed for ${key.chapterId}")
            rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
            return PromoteItemResult.RollbackDone
        }
        return PromoteItemResult.Promoted(entry)
    }

    private sealed interface FinalCheckOutcome {
        data class Reuse(val ref: MirrorFileRef) : FinalCheckOutcome
        data object Proceed : FinalCheckOutcome
        data object RollbackDone : FinalCheckOutcome
    }

    private suspend fun checkFinalForPromoteStaged(
        ctx: RecoverPromoteItemContext,
        staged: StagedMirrorRef,
    ): FinalCheckOutcome {
        val journal = ctx.journal
        val key = ctx.key
        val finalLookup = ctx.storage.lookup(staged.finalRelativePath)
        return when (finalLookup) {
            is MirrorLookupResult.Found -> checkFinalFoundForPromoteStaged(ctx, key, journal, finalLookup)
            is MirrorLookupResult.Missing -> FinalCheckOutcome.Proceed
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover promote: lookup final failed for ${key.chapterId}: " +
                        "${finalLookup.cause?.message}, $KEEPING_JOURNAL_NOT_PROMOTING",
                )
                rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
                FinalCheckOutcome.RollbackDone
            }
        }
    }

    private suspend fun checkFinalFoundForPromoteStaged(
        ctx: RecoverPromoteItemContext,
        key: ChapterKey,
        journal: PendingMirrorPublish,
        finalLookup: MirrorLookupResult.Found,
    ): FinalCheckOutcome {
        val expectedHash = journal.newEntries[key]?.contentHash
        if (expectedHash != null) {
            val hashResult = ctx.storage.readTextAndHash(finalLookup.ref)
            if (hashResult != null) {
                val (_, hash) = hashResult
                if (hash == expectedHash) {
                    // final 已是本事务新正文 → promote 已完成，直接复用
                    return FinalCheckOutcome.Reuse(finalLookup.ref)
                }
                // hash 不匹配 → final 上是错误内容，停止保留 journal
                DiagnosticsLogger.w(
                    TAG,
                    "Recover promote: final hash mismatch for ${key.chapterId}, " +
                        KEEPING_JOURNAL_NOT_PROMOTING,
                )
            } else {
                // 读取失败，无法校验身份，停止保留 journal
                DiagnosticsLogger.w(
                    TAG,
                    "Recover promote: readTextAndHash failed for ${key.chapterId}, " +
                        KEEPING_JOURNAL_NOT_PROMOTING,
                )
            }
        } else {
            // 无期望 hash，状态不明确，停止保留 journal
            DiagnosticsLogger.w(
                TAG,
                "Recover promote: no expectedHash in journal.newEntries for ${key.chapterId}, " +
                    KEEPING_JOURNAL_NOT_PROMOTING,
            )
        }
        rollbackRecoveryOnly(journal, ctx.currentItems, ctx.storage)
        return FinalCheckOutcome.RollbackDone
    }

    /**
     * 循环后：生成 manifest json + publishManifest + 写 cleanup journal + 进入 cleanup。
     */
    private suspend fun finalizeRecoveryPromoteWithManifest(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
    ) {
        // #649 评论 5575950895 问题 4：收口到 frozenManifestPlan 路径。
        // #649 评论 5576464076 问题 5：处理旧 pending journal 缺 frozen plan 时的安全回滚。
        val manifestJson = resolveRecoveryManifestJson(journal, storage, currentItems, promotedEntries)
            ?: return // 已回滚
        val manifestParams = MirrorPublishExecutor.ManifestTransactionParams(
            projectId = journal.projectId,
            snapshot = null,
            desiredEntries = promotedEntries,
            txId = journal.txId,
            journalContext = journal,
            items = currentItems,
            storage = storage,
            prebuiltTargetJson = manifestJson,
        )
        val manifestResult = publishExecutor.publishManifestWithDesiredTransactional(manifestParams)
        if (manifestResult == null) {
            DiagnosticsLogger.w(TAG, "Failed to write manifest during recovery")
            rollbackRecoveryOnly(journal, currentItems, storage)
            return
        }
        writeCleanupJournalAndRecover(journal, storage, currentItems, promotedEntries, manifestResult.committedJournal)
    }

    private suspend fun resolveRecoveryManifestJson(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
    ): String? {
        if (journal.manifestTargetJson != null) {
            // manifest 子事务已经开始，直接用已冻结的 targetJson
            return journal.manifestTargetJson
        }
        if (journal.frozenManifestPlan != null && journal.frozenManifestPlanHash != null) {
            return resolveManifestJsonFromFrozenPlan(journal, storage, currentItems, promotedEntries)
        }
        // 旧格式：没有 plan，且 manifest 子事务也没开始，直接安全回滚
        DiagnosticsLogger.w(TAG, "Recover promote: old journal without frozen plan, rolling back")
        rollbackRecoveryOnly(journal, currentItems, storage)
        return null
    }

    private suspend fun resolveManifestJsonFromFrozenPlan(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
    ): String? {
        val frozenPlanJson = journal.frozenManifestPlan
        val frozenPlanHash = journal.frozenManifestPlanHash
        if (computeContentHash(frozenPlanJson) != frozenPlanHash) {
            DiagnosticsLogger.w(TAG, "Recover promote: frozenManifestPlan hash mismatch, rolling back")
            rollbackRecoveryOnly(journal, currentItems, storage)
            return null
        }
        val plan = frozenManifestPlanFromJson(frozenPlanJson) ?: run {
            DiagnosticsLogger.w(TAG, "Recover promote: failed to parse frozenManifestPlan, rolling back")
            rollbackRecoveryOnly(journal, currentItems, storage)
            return null
        }
        return frozenPlanToManifestJson(plan, promotedEntries) ?: run {
            DiagnosticsLogger.w(TAG, "Recover promote: frozenPlanToManifestJson failed, rolling back")
            rollbackRecoveryOnly(journal, currentItems, storage)
            return null
        }
    }

    private suspend fun writeCleanupJournalAndRecover(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
        committedJournal: PendingMirrorPublish,
    ) {
        // #649 评论 5576949398 问题 1：先写 PHASE_CLEANUP journal，再 recoverCleanupPhase。
        // #649 评论 5576949398 问题 2：从 committedJournal.copy(phase=PHASE_CLEANUP, ...) 继续推进。
        val committedItems = currentItems.mapValues { it.value.copy(state = PendingItem.STATE_COMMITTED) }
        val cleanupJournal = committedJournal.copy(
            phase = PendingMirrorPublish.PHASE_CLEANUP,
            newEntries = promotedEntries,
            stagedRefs = emptyMap(),
            items = committedItems,
        )
        if (!journalWriter.persistPendingJournal(cleanupJournal)) {
            DiagnosticsLogger.w(TAG, "Recover promote: cleanup journal write failed, keeping journal for retry")
            return
        }
        // 进入 cleanup 阶段（putChapterEntries / addPublishedProjectId 在 cleanup 路径里幂等执行）
        recoverCleanupPhase(cleanupJournal, storage)
    }

    /**
     * 只回滚（不删 promotedEntries）。用于校验/复用失败、promote/journal 写入失败。
     */
    private suspend fun rollbackRecoveryOnly(
        journal: PendingMirrorPublish,
        currentItems: Map<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ) {
        rollbackExecutor.rollbackWholePublishTransaction(journal.txId, currentItems, journal.stagedRefs, storage, journal)
    }

    /**
     * 删除已 promotedEntries + 回滚。用于 backup/promote 阶段失败时清理已 promote 的文件。
     */
    private suspend fun rollbackRecoveryWithCleanup(
        journal: PendingMirrorPublish,
        currentItems: Map<ChapterKey, PendingItem>,
        promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
        storage: ReadableMirrorStorage,
    ) {
        for ((_, entry) in promotedEntries) {
            storage.delete(MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath))
        }
        rollbackExecutor.rollbackWholePublishTransaction(journal.txId, currentItems, journal.stagedRefs, storage, journal)
    }

    private fun writeRecoveryPromoteJournal(
        journal: PendingMirrorPublish,
        currentItems: Map<ChapterKey, PendingItem>,
    ): Boolean = journalWriter.writePendingPublishJournal(
        PendingJournalParams(
            projectId = journal.projectId,
            transactionType = journal.transactionType,
            phase = PendingMirrorPublish.PHASE_PROMOTE,
            txId = journal.txId,
            backend = journal.backend,
            treeUri = journal.treeUri,
            oldEntries = journal.oldEntries,
            newEntries = journal.newEntries,
            stagedRefs = journal.stagedRefs,
            items = currentItems,
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
     * 恢复 cleanup 阶段。
     *
     * #649 评论 5562462046 问题 4：根据 [MirrorTransactionType] 分支处理。
     * - UPSERT_PROJECT：删 snapshot 中已不存在的旧 key 对应的旧正文。
     * - DELETE_PROJECT：先确保 manifest 已提交成不引用该项目（journal 记录的 manifestNewRef），
     *   再删旧正文，再从 stateStore 删该项目条目。
     */

    internal suspend fun recoverCleanupPhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        when (journal.transactionType) {
            MirrorTransactionType.UPSERT_PROJECT -> recoverCleanupUpsertProject(journal, storage)
            MirrorTransactionType.DELETE_PROJECT -> recoverCleanupDeleteProject(journal, storage)
        }
    }

    private suspend fun recoverCleanupUpsertProject(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        // #649 评论 5576949398 问题 2：cleanup 第一步先补 committed* committed baseline。
        if (!recoverCleanupBaseline(journal, "UPSERT_PROJECT")) return
        // #649 评论 5573310799 问题 5：幂等执行 stateStore 提交
        //    publishProject 先写 PHASE_CLEANUP journal 再更新 stateStore，
        //    恢复时必须先补上 stateStore 更新，再清旧文件。
        if (!stateStore.putChapterEntries(journal.newEntries)) {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: putChapterEntries failed for UPSERT_PROJECT ${journal.projectId}, keeping journal",
            )
            return
        }
        if (!stateStore.addPublishedProjectId(journal.projectId)) {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: addPublishedProjectId failed for UPSERT_PROJECT ${journal.projectId}, keeping journal",
            )
            return
        }
        // #649 评论 5563333323 缺口 3：调用统一 cleanup 函数，不复制两份逻辑。
        // allLiveKeys = journal.newEntries.keys：只清理已删除章节的旧 ref。
        if (publishExecutor.cleanupCommittedTransaction(journal, storage, allLiveKeys = journal.newEntries.keys)) {
            // 全部清理成功，清除 journal
            stateStore.clearPendingPublish()
        } else {
            // 有失败项，保留 journal，下次 recover 继续清
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: partial failure for UPSERT_PROJECT ${journal.projectId}, keeping journal",
            )
        }
    }

    private suspend fun recoverCleanupDeleteProject(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        // #649 评论 5576949398 问题 2：cleanup 第一步先补 committed baseline。
        if (!recoverCleanupBaseline(journal, "DELETE_PROJECT")) return
        // 1. 确保 manifest 已提交成不引用该项目
        //    #649 评论 5562715833 问题 6：isManifestCommitted=true 时不再调 publishManifest，直接 cleanup
        if (!journal.isManifestCommitted) {
            if (!recoverCleanupDeleteManifest(journal, storage)) return
        }
        // 2. 从 stateStore 删除该项目条目（若尚未删）
        //    #649 评论 5563333323 缺口 2：removeAllProjectEntries 返回 Result
        val removeResult = stateStore.removeAllProjectEntries(journal.projectId)
        if (removeResult.isFailure) {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: removeAllProjectEntries failed for ${journal.projectId}, keeping journal",
            )
            return
        }
        // 3. 调用统一 cleanup 删旧正文 + manifestBackup + tx staging
        //    allLiveKeys = null：DELETE_PROJECT 时 oldEntries 全部要删
        if (publishExecutor.cleanupCommittedTransaction(journal, storage, allLiveKeys = null)) {
            // #649 评论 5564820566 问题 5：delete 成功后移除 publishedProjectId
            // #649 评论 5565067997 修复 6：检查 removePublishedProjectId 返回值
            if (!stateStore.removePublishedProjectId(journal.projectId)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover cleanup: removePublishedProjectId failed for ${journal.projectId}, keeping journal",
                )
                return
            }
            stateStore.clearPendingPublish()
        } else {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: partial failure for DELETE_PROJECT ${journal.projectId}, keeping journal",
            )
        }
    }

    /**
     * 幂等补写 committed baseline。返回 true 表示已补/不需要补；false 表示失败，调用方应 return。
     */
    private fun recoverCleanupBaseline(
        journal: PendingMirrorPublish,
        type: String,
    ): Boolean {
        // journal 已落盘且 isManifestCommitted=true 时，committed baseline 可能还没写
        // （断电窗口：journal 落盘后、setCommittedManifest 前）。
        if (journal.isManifestCommitted && !journalWriter.persistCommittedBaselineFromJournal(journal)) {
            DiagnosticsLogger.w(
                TAG,
                RECOVER_CLEANUP_BASELINE_FAILED + "$type " + projectIdKeepingJournal(journal.projectId),
            )
            return false
        }
        return true
    }

    /**
     * DELETE_PROJECT 的 manifest 重写。返回 true 表示成功；false 表示失败，调用方应 return。
     */
    private suspend fun recoverCleanupDeleteManifest(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ): Boolean {
        // #649 评论 5576464076 问题 3：DELETE 恢复也优先用 frozen plan，
        // 与正常删除路径保持一致，不重新从 oldEntries 拼全局 manifest。
        val targetJsonOutcome = resolveDeleteRecoveryManifestTargetJson(journal)
        if (targetJsonOutcome == DeleteManifestTargetJsonOutcome.Failed) return false
        val recoveryManifestTargetJson = (targetJsonOutcome as DeleteManifestTargetJsonOutcome.Resolved).json
        val desiredWithoutDeleted = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        for ((key, entry) in journal.oldEntries) {
            if (key.projectId != journal.projectId) {
                desiredWithoutDeleted[key] = entry
            }
        }
        val manifestParams = MirrorPublishExecutor.ManifestTransactionParams(
            projectId = journal.projectId,
            snapshot = null,
            desiredEntries = if (recoveryManifestTargetJson != null) emptyMap() else desiredWithoutDeleted,
            txId = journal.txId,
            journalContext = journal,
            items = journal.items,
            storage = storage,
            prebuiltTargetJson = recoveryManifestTargetJson,
        )
        val manifestResult = publishExecutor.publishManifestWithDesiredTransactional(manifestParams)
        if (manifestResult == null) {
            DiagnosticsLogger.w(
                TAG,
                "Recover cleanup: manifest rewrite failed for DELETE_PROJECT ${journal.projectId}",
            )
            return false
        }
        // #649 评论 5576464076 问题 2：恢复路径也幂等写入 committed manifest。
        // #649 评论 5576949398 问题 2：用 persistCommittedBaselineFromJournal 统一写入。
        if (!journalWriter.persistCommittedBaselineFromJournal(manifestResult.committedJournal)) {
            DiagnosticsLogger.w(
                TAG,
                RECOVER_CLEANUP_BASELINE_FAILED + "DELETE_PROJECT " + projectIdKeepingJournal(journal.projectId),
            )
            return false
        }
        return true
    }

    private sealed interface DeleteManifestTargetJsonOutcome {
        /** json 可能是 null（旧格式从 oldEntries 构造）。 */
        data class Resolved(val json: String?) : DeleteManifestTargetJsonOutcome
        data object Failed : DeleteManifestTargetJsonOutcome
    }

    private fun resolveDeleteRecoveryManifestTargetJson(
        journal: PendingMirrorPublish,
    ): DeleteManifestTargetJsonOutcome {
        if (journal.manifestTargetJson != null) {
            // manifest 子事务已开始，直接用已冻结的 targetJson
            return DeleteManifestTargetJsonOutcome.Resolved(journal.manifestTargetJson)
        }
        if (journal.frozenManifestPlan != null && journal.frozenManifestPlanHash != null) {
            // 新格式：从 plan 生成 prebuiltTargetJson
            if (computeContentHash(journal.frozenManifestPlan) != journal.frozenManifestPlanHash) {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover cleanup: frozenManifestPlan hash mismatch for DELETE, keeping journal",
                )
                return DeleteManifestTargetJsonOutcome.Failed
            }
            val plan = frozenManifestPlanFromJson(journal.frozenManifestPlan) ?: run {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover cleanup: failed to parse frozenManifestPlan for DELETE, keeping journal",
                )
                return DeleteManifestTargetJsonOutcome.Failed
            }
            return DeleteManifestTargetJsonOutcome.Resolved(frozenPlanToManifestJson(plan, emptyMap()))
        }
        // 旧格式：没有 plan 且 manifest 子事务没开始，从 oldEntries 构造
        return DeleteManifestTargetJsonOutcome.Resolved(null)
    }

    /**
     * 恢复 rollback 阶段。
     *
     * #649 评论 5564624383 问题 2：进程死在回滚中间时继续回滚。
     * journal phase == PHASE_ROLLBACK 时，继续未完成的 rollback 步骤。
     */
    internal suspend fun recoverRollbackPhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        val currentItems = journal.items.toMutableMap()

        if (!rollbackDeletePromotedItems(journal, currentItems, storage)) return

        val allSuccess = rollbackRestoreOldItems(journal, currentItems, storage)
        if (!allSuccess) {
            DiagnosticsLogger.w(TAG, "Recover rollback: partial failure, keeping journal for retry")
            return
        }

        if (!rollbackExecutor.rollbackManifest(journal, storage, currentItems)) {
            DiagnosticsLogger.w(TAG, "Recover rollback: manifest rollback failed, keeping journal")
            return
        }
        if (storage.rollback(journal.txId)) {
            stateStore.clearPendingPublish()
        } else {
            DiagnosticsLogger.w(TAG, "Recover rollback: staging cleanup failed, keeping journal for retry")
        }
    }

    /** 步骤 1：删除 promotedRef（幂等，跳过已处理的 item）。返回 false 表示应停止恢复。 */
    private fun rollbackDeletePromotedItems(
        journal: PendingMirrorPublish,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ): Boolean {
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue
            if (item.state != PendingItem.STATE_ROLLBACK_NEW_REMOVED) {
                val removed = item.promotedRef?.let { storage.delete(it) } ?: true
                if (!removed) {
                    DiagnosticsLogger.w(TAG, "Recover rollback: delete promotedRef failed for ${key.chapterId}, keeping journal")
                    return false
                }
                currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_NEW_REMOVED)
            }
            if (!writeRollbackJournal(journal, currentItems)) {
                DiagnosticsLogger.w(TAG, "Recover rollback: journal write failed after NEW_REMOVED for ${key.chapterId}")
                return false
            }
        }
        return true
    }

    /** 步骤 2：恢复 backupOldRef 到最终路径。返回 false 表示应停止恢复。 */
    private fun rollbackRestoreOldItems(
        journal: PendingMirrorPublish,
        currentItems: MutableMap<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ): Boolean {
        var allSuccess = true
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue
            when (val result = rollbackExecutor.rollbackChapterToOldState(journal, key, item, storage)) {
                is RollbackItemResult.Restored -> {
                    val oldEntry = journal.oldEntries[key] ?: run {
                        DiagnosticsLogger.w(TAG, "Recover rollback: missing oldEntry for ${key.chapterId}, keeping journal")
                        return false
                    }
                    if (!stateStore.putChapterEntry(key.projectId, key.volumeId, key.chapterId, oldEntry.copy(uri = result.ref.uri, relativePath = result.ref.relativePath))) {
                        DiagnosticsLogger.w(TAG, "Recover rollback: putChapterEntry failed for ${key.chapterId}, keeping journal")
                        return false
                    }
                    currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_OLD_RESTORED)
                }
                RollbackItemResult.NewFileRemoved -> {
                    currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_OLD_RESTORED)
                }
                RollbackItemResult.StateUnknown -> {
                    DiagnosticsLogger.w(TAG, "Recover rollback: state unknown for ${key.chapterId}, keeping journal")
                    return false
                }
                RollbackItemResult.Failed -> {
                    DiagnosticsLogger.w(TAG, "Recover rollback: failed for ${key.chapterId}, keeping journal")
                    allSuccess = false
                    continue
                }
            }
            if (!writeRollbackJournal(journal, currentItems)) {
                DiagnosticsLogger.w(TAG, "Recover rollback: journal write failed after OLD_RESTORED for ${key.chapterId}")
                return false
            }
        }
        return allSuccess
    }

    /** 写 rollback 阶段 journal。返回 false 表示写入失败。 */
    private fun writeRollbackJournal(
        journal: PendingMirrorPublish,
        items: Map<ChapterKey, PendingItem>,
    ): Boolean = journalWriter.writePendingPublishJournal(PendingJournalParams(
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
    ))

    // Moved to MirrorPublishPlanner

    private fun projectIdKeepingJournal(projectId: String): String = "$projectId, keeping journal"


    private fun writeRecoveryJournal(
        journal: PendingMirrorPublish,
        items: Map<ChapterKey, PendingItem>,
        phase: String,
        manifestSwapState: ManifestTransactionState,
        manifestNewRef: MirrorFileRef? = journal.manifestNewRef,
        isManifestCommitted: Boolean = journal.isManifestCommitted,
    ): Boolean = journalWriter.writePendingPublishJournal(
        PendingJournalParams(
            projectId = journal.projectId,
            transactionType = journal.transactionType,
            phase = phase,
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
            isManifestCommitted = isManifestCommitted,
            manifestSwapState = manifestSwapState,
            manifestNewContentHash = journal.manifestNewContentHash,
            manifestOldContentHash = journal.manifestOldContentHash,
            journalContext = journal,
        ),
    )

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val MIME_MARKDOWN = "text/markdown"
        private const val KEEPING_JOURNAL_NOT_PROMOTING = "keeping journal, not promoting"
        private const val RECOVER_CLEANUP_BASELINE_FAILED = "Recover cleanup: committed baseline failed for "
        private const val OLD_RESTORED = "OLD_RESTORED"
    }
}
