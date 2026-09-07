package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import java.time.Instant
import java.time.format.DateTimeFormatter

/**
 * 发布结果密封接口。
 *
 * - [Committed]：发布成功，事件已提交，可以安全移除
 * - [PendingRecovery]：存在未恢复的 pending publish，需要等待恢复完成
 * - [RetryableFailure]：可重试的失败，事件应保留并稍后重试
 */
sealed interface MirrorPublishResult {
    data object Committed : MirrorPublishResult

    data object PendingRecovery : MirrorPublishResult

    data object RetryableFailure : MirrorPublishResult
}

/**
 * ReadableMirrorPublisher — 异步发布正文到 Download/Sujian 镜像。
 *
 * #649 评论 5560971132 修复 4/6/7：重构发布器。
 * #649 评论 5561465552 第 3+4 点：改用 [ReadableMirrorStorage] 接口 + 事务性发布 + journal。
 * #649 评论 5561974464 问题 1：SAF 恢复后 Publisher 不会立即切换到 DocumentTree 后端。
 *
 * ## 修复 4：用户可读路径
 * 旧路径 `projects/<id>/volumes/<vid>/chapters/<cid>.md` 对用户不可读。
 * 新路径 `作品/<作品名>/<卷名>/<章节名>.md`，标题经 [sanitizeFileName] 净化。
 * 同目录重名时给文件名追加 chapterId 前 8 字符。
 *
 * ## 修复 6：删除旧文件
 * 用 [ReadableMirrorStateStore] 跟踪每个章节对应的 URI。发布时：
 * - 章节仍存在：覆盖写旧 URI（或 URI 失效时新建）。
 * - 章节已删除：从 state store 拿旧 URI，调 [ReadableMirrorStorage.delete]。
 * - 项目删除：逐个删旧 URI，不查 Core。
 *
 * ## 修复 7：contentHash 用 SHA-256
 * manifest 的 `contentHash` 用 [computeContentHash]（SHA-256）对实际正文计算，
 * 不再用 Core 的 `chapter.hash`（MD5）。恢复时用 [verifyContentHash] 校验。
 *
 * ## #649 评论 5561465552 第 3 点：统一存储接口
 * 构造时注入 [MirrorStorageRouter]，每次事务开始时从 [router.current] 获取 storage，
 * 不再持有固定 storage 实例。由 [MirrorStorageRouter] 根据 stateStore.backend 选择 MediaStore 或 SAF 后端。
 *
 * ## #649 评论 5561465552 第 4 点：事务性发布
 * publishProject 改成真正的"准备 → 写入 → 提交 manifest → 清旧文件"顺序：
 * 1. **准备阶段**：先把整个作品当前快照和所有章节正文全部读完，只放内存，不碰 Download。
 *    计算完整 desired state：每章目标路径、hash、旧 ref、新 ref。
 * 2. **写入阶段**：写新/更新后的正文，得到一份完整的新镜像状态；路径变化时旧文件先保留
 *    （不立即删）。任一章节写入失败 → 整个 publishProject 返回，不动 stateStore，不写 manifest，
 *    旧镜像保持不变。
 * 3. **提交 manifest**：`manifest.json` 成功写成这份新状态后，才把新 state 持久化为 committed state
 *    （批量更新 stateStore）。
 * 4. **清理阶段**：最后再删除已经不被新 manifest 引用的旧文件。
 *
 * `deleteProject()` / `cleanupStaleProjects()` 也一样：先让新 manifest 不再引用旧项目，
 * manifest 成功后再删旧 URI，不能先删正文再尝试写 manifest。
 *
 * ## pendingPublish journal
 * 在 [ReadableMirrorStateStore] 里加一份 `pendingPublish` journal（JSON 文件，记录正在进行的发布：
 * 目标 state、已写入的文件、已删除的文件）。发布开始时写 journal，每步更新，成功后删除 journal。
 * 下次启动/下一次 worker 如果发现 pendingPublish journal，继续完成这次发布（重新写未完成的文件、
 * 删旧文件），不猜旧状态。journal 文件路径：`noBackupFilesDir/sujian-mirror/pending-publish.json`。
 *
 * ## 循环依赖
 * 只依赖 [MirrorSnapshotSource]（只读快照），不持有 AppServiceBridge/ProjectBridge，
 * 切断 `AppServiceBridge → MirrorChangeSink → Publisher → AppServiceBridge` 循环。
 *
 * ## 安全约束
 * - 不把 `content://` URI 传给 Rust——只把文本写入存储。
 * - 所有 I/O 失败只记日志，不阻断业务。
 */
class ReadableMirrorPublisher(
    private val source: MirrorSnapshotSource,
    private val router: MirrorStorageRouter,
    private val stateStore: ReadableMirrorStateStore,
) {
    /**
     * #649 评论 5564379115 问题 4：不再使用一次性 AtomicBoolean。
     * pendingRecovered 一旦被设成 true，就永远不再检查 journal，
     * 导致新产生的 pending 被下一笔事务覆盖。
     *
     * 新逻辑：每次开启新事务前都要检查当前 journal：
     * - NotExists → 可以开新事务
     * - Success → 先恢复；恢复后重新读取，只有变成 NotExists 才能继续
     * - Corrupted → 停止，不开新事务
     */
    private suspend fun ensurePendingRecovered(): Boolean {
        // #649 评论 5564624383 问题 1：不要靠异常判断恢复结果。
        // recoverPendingPublishIfNeeded() 遇到 Corrupted / forBackend 失败 / storage 不支持
        // 只是 return，不抛异常，导致 ensurePendingRecovered 仍返回 true，
        // 下一笔 writePendingPublishJournal 会覆盖旧 journal。
        // 新逻辑：恢复前读一次、恢复后再读一次，只有 journal 不存在才返回 true。
        return when (stateStore.readPendingPublish()) {
            is PendingPublishResult.NotExists -> true
            is PendingPublishResult.Corrupted -> false
            is PendingPublishResult.Success -> {
                recoverPendingPublishIfNeeded()
                stateStore.readPendingPublish() is PendingPublishResult.NotExists
            }
        }
    }

    /**
     * 检查并恢复 pending publish（如果存在）。
     *
     * #649 评论 5561974464 问题 3：pendingPublish 没有恢复逻辑。
     * 在应用初始化/worker 开始消费业务事件前调用恢复方法。
     *
     * 恢复策略：
     * - `stage`：staging 未完成，旧镜像完整 → rollback(txId) + clearPendingPublish
     * - `promote`：staging 已写完，promote 部分完成 → 继续 promote 剩余，然后 cleanup
     * - `cleanup`：已 promote 完，stateStore 未更新/清理未完成 → 继续 cleanup
     *
     * #649 评论 5563333323 缺口 2：损坏的 pending journal 必须阻止启动新事务。
     * 损坏时直接返回，不启动新事务；也不清理损坏的 journal（保留供人工排查）。
     *
     * #649 评论 5564379115 问题 4/5：恢复完成后重新读取 journal，确认已清理。
     * 如果恢复后 journal 仍然存在（恢复失败或部分完成），下次 ensurePendingRecovered
     * 会再次尝试恢复，不会创建新事务覆盖未完成的 journal。
     * 使用 forBackendResult() 替代已废弃的 forBackend()。
     */
    suspend fun recoverPendingPublishIfNeeded() {
        val pendingResult = stateStore.readPendingPublish()
        when (pendingResult) {
            is PendingPublishResult.NotExists -> return
            is PendingPublishResult.Corrupted -> {
                // #649 评论 5563333323 缺口 2：损坏时记录日志，不启动新事务，不清理 journal
                DiagnosticsLogger.e(
                    TAG,
                    "Pending publish journal is corrupted, cannot start new transaction",
                    pendingResult.error,
                )
                return
            }
            is PendingPublishResult.Success -> {
                val journalJson = pendingResult.json
                val journal = PendingMirrorPublish.fromJson(journalJson) ?: return
                DiagnosticsLogger.i(
                    TAG,
                    "Recovering pending publish: phase=${journal.phase}, " +
                        "projectId=${journal.projectId}, txType=${journal.transactionType}",
                )

                // #649 评论 5564379115 问题 5：用 forBackendResult() 替代已废弃的 forBackend()
                // 恢复时按 journal 记录的 backend/treeUri 构造当时那套 storage
                val storageResult = router.forBackendResult(journal.backend, journal.treeUri)
                if (storageResult.isFailure) {
                    // #649 评论 5564379115 问题 5：storage 不可用时保留 journal，不要 clearPendingPublish()
                    DiagnosticsLogger.w(
                        TAG,
                        "Storage not available during recovery: ${storageResult.exceptionOrNull()?.message}, " +
                            "keeping journal",
                    )
                    return
                }
                val storage = storageResult.getOrThrow()
                if (!storage.isSupported()) {
                    // #649 评论 5564379115 问题 5：storage 不可用时保留 journal
                    DiagnosticsLogger.w(TAG, "Storage not supported during recovery, keeping journal")
                    return
                }

                when (journal.phase) {
                    PendingMirrorPublish.PHASE_STAGE -> {
                        // staging 未完成，旧镜像完整 → rollback + clear
                        // #649 评论 5566303837 问题 6：检查 rollback 返回值
                        if (storage.rollback(journal.txId)) {
                            stateStore.clearPendingPublish()
                        } else {
                            DiagnosticsLogger.w(TAG, "Recover stage: rollback failed, keeping journal for retry")
                        }
                    }
                    PendingMirrorPublish.PHASE_PROMOTE -> {
                        // staging 已写完，promote 部分完成 → 继续 promote
                        recoverPromotePhase(journal, storage)
                    }
                    PendingMirrorPublish.PHASE_CLEANUP -> {
                        // 已 promote 完，stateStore 未更新/清理未完成 → 继续 cleanup
                        recoverCleanupPhase(journal, storage)
                    }
                    // #649 评论 5564624383 问题 2：恢复回滚阶段
                    PendingMirrorPublish.PHASE_ROLLBACK -> {
                        recoverRollbackPhase(journal, storage)
                    }
                    else -> {
                        DiagnosticsLogger.w(TAG, "Unknown phase in pending publish: ${journal.phase}")
                        stateStore.clearPendingPublish()
                    }
                }

                // #649 评论 5564379115 问题 4：恢复完成后重新读取 journal 确认已清理
                // 如果恢复失败或部分完成，journal 仍然存在，下次会再次尝试恢复
            }
        }
    }

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
    private suspend fun recoverPromotePhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        val promotedEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        // 把已 PROMOTED 或 COMMITTED 的 item 直接收进 promotedEntries；只对未完成的 item 继续 promote。
        val currentItems = journal.items.toMutableMap()
        for ((key, item) in currentItems.toMap()) {
            // #649 评论 5562715833 问题 4a：跳过 STATE_PROMOTED 和 STATE_COMMITTED
            if ((item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
                item.promotedRef != null
            ) {
                promotedEntries[key] =
                    ChapterMirrorEntry(
                        uri = item.promotedRef.uri,
                        relativePath = item.promotedRef.relativePath,
                        revision = journal.newEntries[key]?.revision ?: 0L,
                        contentHash = journal.newEntries[key]?.contentHash ?: "",
                    )
                continue
            }
            val staged = item.stagedRef ?: journal.stagedRefs[key]
            if (staged == null) {
                DiagnosticsLogger.w(TAG, "Recover promote missing stagedRef for ${key.chapterId}, skipping")
                continue
            }
            val oldRef = item.oldRef
            // #649 评论 5562715833 问题 2：backupCommitted + promoteStaged，不先删 old
            // #649 评论 5563798095：崩溃窗口恢复 — backupCommitted 已移动 old 但 journal 未更新时，
            // 检测 backup 是否已存在于备份目录，如果存在则跳过重复 backup。
            // #649 评论 5565067997 修复 1：用 STATE_BACKUP_READY / STATE_OLD_VACATED 显式状态。
            if (oldRef != null && item.state != PendingItem.STATE_OLD_VACATED) {
                // #649 评论 5564820566 问题 3：两步 journalable backup — recover 同样用 prepareBackup + vacateCommitted
                // 使用 lookupBackup 三态查询（#649 评论 5565862745 问题 3）
                val backupResult = storage.lookupBackup(journal.txId, oldRef.relativePath)
                val backupReady =
                    when (backupResult) {
                        is MirrorLookupResult.Found -> {
                            // backup 已存在，直接复用
                            // 用 lookup() 三态查询判断 old 是否已 vacate（#649 评论 5565067997 修复 1）
                            val oldLookup = storage.lookup(oldRef.relativePath)
                            val vacated =
                                when (oldLookup) {
                                    is MirrorLookupResult.Missing -> true
                                    is MirrorLookupResult.Found -> false
                                    is MirrorLookupResult.Failed -> {
                                        // 查询失败，不能继续，回滚
                                        DiagnosticsLogger.w(
                                            TAG,
                                            "Recover backup: " +
                                                "lookup old failed for ${key.chapterId}: ${oldLookup.cause?.message}",
                                        )
                                        for ((_, entry) in promotedEntries) {
                                            storage.delete(
                                                MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath),
                                            )
                                        }
                                        rollbackWholePublishTransaction(
                                            journal.txId,
                                            currentItems,
                                            journal.stagedRefs,
                                            storage,
                                            journal,
                                        )
                                        return
                                    }
                                }
                            BackupReadyRef(backupRef = backupResult.ref, vacated = vacated)
                        }
                        is MirrorLookupResult.Missing -> {
                            // backup 不存在，需要 prepareBackup
                            val prepared = storage.prepareBackup(journal.txId, oldRef, MIME_MARKDOWN)
                            if (prepared == null) {
                                DiagnosticsLogger.w(TAG, "Recover backup prepare failed for ${key.chapterId}")
                                for ((_, entry) in promotedEntries) {
                                    storage.delete(MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath))
                                }
                                rollbackWholePublishTransaction(
                                    journal.txId,
                                    currentItems,
                                    journal.stagedRefs,
                                    storage,
                                    journal,
                                )
                                return
                            }
                            prepared
                        }
                        is MirrorLookupResult.Failed -> {
                            // 查询失败，不能继续，回滚
                            DiagnosticsLogger.w(
                                TAG,
                                "Recover backup: " +
                                    "lookupBackup failed for ${key.chapterId}: ${backupResult.cause?.message}",
                            )
                            for ((_, entry) in promotedEntries) {
                                storage.delete(MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath))
                            }
                            rollbackWholePublishTransaction(
                                journal.txId,
                                currentItems,
                                journal.stagedRefs,
                                storage,
                                journal,
                            )
                            return
                        }
                    }
                // #649 评论 5565067997 修复 1：journal 先写 STATE_BACKUP_READY
                currentItems[key] =
                    item.copy(
                        backupOldRef = backupReady.backupRef,
                        state = PendingItem.STATE_BACKUP_READY,
                    )
                if (!writePendingPublishJournal(
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
                    )
                ) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Recover backup: journal write failed (BACKUP_READY) for ${key.chapterId}, keeping journal",
                    )
                    for ((_, entry) in promotedEntries) {
                        storage.delete(MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath))
                    }
                    rollbackWholePublishTransaction(journal.txId, currentItems, journal.stagedRefs, storage, journal)
                    return
                }
                // vacate old（如果 prepareBackup 还没 move old）
                if (!backupReady.vacated) {
                    if (!storage.vacateCommitted(oldRef)) {
                        DiagnosticsLogger.w(TAG, "Recover vacate failed for ${key.chapterId}")
                        for ((_, entry) in promotedEntries) {
                            storage.delete(MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath))
                        }
                        rollbackWholePublishTransaction(
                            journal.txId,
                            currentItems,
                            journal.stagedRefs,
                            storage,
                            journal,
                        )
                        return
                    }
                }
                // #649 评论 5565067997 修复 1：vacate 成功后写 STATE_OLD_VACATED
                currentItems[key] = currentItems[key]!!.copy(state = PendingItem.STATE_OLD_VACATED)
                if (!writePendingPublishJournal(
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
                    )
                ) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Recover backup: journal write failed (OLD_VACATED) for ${key.chapterId}, keeping journal",
                    )
                    for ((_, entry) in promotedEntries) {
                        storage.delete(MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath))
                    }
                    rollbackWholePublishTransaction(journal.txId, currentItems, journal.stagedRefs, storage, journal)
                    return
                }
            }
            // 2. promote staged（不删 old）
            //    #649 评论 5566303837 问题 2：OLD_VACATED 崩溃窗口检查
            //    #649 评论 5569598106 问题2：lookup 返回 Failed / hash 不匹配 / 无法校验时
            //    必须停止保留 journal，不继续猜测式 promote。只允许明确确认时推进。
            var newRef: MirrorFileRef? = null
            if (item.state == PendingItem.STATE_OLD_VACATED) {
                val finalLookup = storage.lookup(staged.finalRelativePath)
                when (finalLookup) {
                    is MirrorLookupResult.Found -> {
                        val expectedHash = journal.newEntries[key]?.contentHash
                        if (expectedHash != null) {
                            val hashResult = storage.readTextAndHash(finalLookup.ref)
                            if (hashResult != null) {
                                val (_, hash) = hashResult
                                if (hash == expectedHash) {
                                    // final 已是本事务新正文 → promote 已完成，直接复用
                                    newRef = finalLookup.ref
                                } else {
                                    // hash 不匹配 → final 上是错误内容，停止保留 journal
                                    DiagnosticsLogger.w(
                                        TAG,
                                        "Recover promote: final hash mismatch for ${key.chapterId}, " +
                                            "keeping journal, not promoting",
                                    )
                                    rollbackWholePublishTransaction(
                                        journal.txId,
                                        currentItems,
                                        journal.stagedRefs,
                                        storage,
                                        journal,
                                    )
                                    return
                                }
                            } else {
                                // 读取失败，无法校验身份，停止保留 journal
                                DiagnosticsLogger.w(
                                    TAG,
                                    "Recover promote: readTextAndHash failed for ${key.chapterId}, " +
                                        "keeping journal, not promoting",
                                )
                                rollbackWholePublishTransaction(
                                    journal.txId,
                                    currentItems,
                                    journal.stagedRefs,
                                    storage,
                                    journal,
                                )
                                return
                            }
                        } else {
                            // 无期望 hash（journal.newEntries[key] 缺失），状态不明确，停止保留 journal
                            DiagnosticsLogger.w(
                                TAG,
                                "Recover promote: no expectedHash in journal.newEntries for ${key.chapterId}, " +
                                    "keeping journal, not promoting",
                            )
                            rollbackWholePublishTransaction(
                                journal.txId,
                                currentItems,
                                journal.stagedRefs,
                                storage,
                                journal,
                            )
                            return
                        }
                    }
                    is MirrorLookupResult.Missing -> {
                        // final 不存在，继续 promote（正常路径）
                    }
                    is MirrorLookupResult.Failed -> {
                        // lookup 失败，状态不明确，停止保留 journal，不继续 promote
                        DiagnosticsLogger.w(
                            TAG,
                            "Recover promote: lookup final failed for ${key.chapterId}: " +
                                "${finalLookup.cause?.message}, keeping journal, not promoting",
                        )
                        rollbackWholePublishTransaction(
                            journal.txId,
                            currentItems,
                            journal.stagedRefs,
                            storage,
                            journal,
                        )
                        return
                    }
                }
            }
            if (newRef == null) {
                newRef = storage.promoteStaged(staged, staged.finalRelativePath)
            }
            if (newRef == null) {
                DiagnosticsLogger.w(TAG, "Recover promote failed for ${key.chapterId}")
                // #649 评论 5564379115 问题 2：统一事务回滚
                rollbackWholePublishTransaction(journal.txId, currentItems, journal.stagedRefs, storage, journal)
                return
            }
            promotedEntries[key] =
                ChapterMirrorEntry(
                    uri = newRef.uri,
                    relativePath = newRef.relativePath,
                    revision = journal.newEntries[key]?.revision ?: 0L,
                    contentHash = journal.newEntries[key]?.contentHash ?: "",
                )
            // 逐项更新 journal（记录该 item 已 PROMOTED）
            currentItems[key] = currentItems[key]!!.copy(promotedRef = newRef, state = PendingItem.STATE_PROMOTED)
            // #649 评论 5563333323 缺口 2：journal 写入失败则停止
            if (!writePendingPublishJournal(
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
                )
            ) {
                DiagnosticsLogger.w(TAG, "Recover promote: journal write failed for ${key.chapterId}")
                // #649 评论 5564379115 问题 2：统一事务回滚
                rollbackWholePublishTransaction(journal.txId, currentItems, journal.stagedRefs, storage, journal)
                return
            }
        }

        // 写 manifest（走事务性 manifest 写入）
        val snapshotResult = source.getProjectWorkspaceSnapshot(journal.projectId)
        if (snapshotResult !is BridgeResult.Success) {
            DiagnosticsLogger.w(TAG, "Failed to get snapshot for project ${journal.projectId} during recovery")
            // #649 评论 5564379115 问题 2：统一事务回滚
            rollbackWholePublishTransaction(journal.txId, currentItems, journal.stagedRefs, storage, journal)
            return
        }
        val manifestResult =
            publishManifestWithDesiredTransactional(
                projectId = journal.projectId,
                snapshot = snapshotResult.data,
                desiredEntries = promotedEntries,
                txId = journal.txId,
                journalContext = journal,
                items = currentItems,
                storage = storage,
            )
        if (manifestResult == null) {
            DiagnosticsLogger.w(TAG, "Failed to write manifest during recovery")
            // #649 评论 5564379115 问题 2：统一事务回滚
            rollbackWholePublishTransaction(journal.txId, currentItems, journal.stagedRefs, storage, journal)
            return
        }
        // manifest 成功后批量更新 stateStore
        // #649 评论 5563333323 缺口 2：putChapterEntries 失败也不清 journal
        if (!stateStore.putChapterEntries(promotedEntries)) {
            DiagnosticsLogger.w(TAG, "Recover promote: putChapterEntries failed, keeping journal for retry")
            return
        }
        // #649 评论 5564820566 问题 5：恢复成功后也标记作品已发布
        // #649 评论 5565067997 修复 6：检查 addPublishedProjectId 返回值，失败时不清 journal
        if (!stateStore.addPublishedProjectId(journal.projectId)) {
            DiagnosticsLogger.w(TAG, "Recover promote: addPublishedProjectId failed, keeping journal for retry")
            return
        }
        // 标记所有 item 为 COMMITTED
        val committedItems = currentItems.mapValues { it.value.copy(state = PendingItem.STATE_COMMITTED) }
        // #649 评论 5562715833 问题 4b：manifest 成功后先写 cleanup journal，再 recoverCleanupPhase
        // #649 评论 5563333323 缺口 2：journal 写入失败则保留 journal 重试
        if (!writePendingPublishJournal(
                projectId = journal.projectId,
                transactionType = journal.transactionType,
                phase = PendingMirrorPublish.PHASE_CLEANUP,
                txId = journal.txId,
                backend = journal.backend,
                treeUri = journal.treeUri,
                oldEntries = journal.oldEntries,
                newEntries = promotedEntries,
                stagedRefs = emptyMap(),
                items = committedItems,
                removedProjectIds = journal.removedProjectIds,
                manifestOldRef = manifestResult.manifestOldRef,
                manifestStagedRef = manifestResult.manifestStagedRef,
                manifestNewRef = manifestResult.newRef,
                manifestBackupRef = manifestResult.backupOldRef,
                isManifestCommitted = true,
                manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                manifestNewContentHash = manifestResult.manifestNewContentHash,
                manifestOldContentHash = manifestResult.manifestOldContentHash,
            )
        ) {
            DiagnosticsLogger.w(TAG, "Recover promote: cleanup journal write failed, keeping journal for retry")
            return
        }
        // 进入 cleanup 阶段
        recoverCleanupPhase(
            journal.copy(
                phase = PendingMirrorPublish.PHASE_CLEANUP,
                newEntries = promotedEntries,
                items = committedItems,
                manifestNewRef = manifestResult.newRef,
                manifestBackupRef = manifestResult.backupOldRef,
                isManifestCommitted = true,
                manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
            ),
            storage,
        )
    }

    /**
     * 恢复 cleanup 阶段。
     *
     * #649 评论 5562462046 问题 4：根据 [MirrorTransactionType] 分支处理。
     * - UPSERT_PROJECT：删 snapshot 中已不存在的旧 key 对应的旧正文。
     * - DELETE_PROJECT：先确保 manifest 已提交成不引用该项目（journal 记录的 manifestNewRef），
     *   再删旧正文，再从 stateStore 删该项目条目。
     */
    private suspend fun recoverCleanupPhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        when (journal.transactionType) {
            MirrorTransactionType.UPSERT_PROJECT -> {
                // #649 评论 5573310799 问题 5：幂等执行 stateStore 提交
                //    （journal 已落盘，stateStore 可能已写也可能没写）。
                //    publishProject 现在先写 PHASE_CLEANUP journal 再更新 stateStore，
                //    恢复时必须先补上 stateStore 更新，再清旧文件。
                if (!stateStore.putChapterEntries(journal.newEntries)) {
                    DiagnosticsLogger.w(TAG, "Recover cleanup: putChapterEntries failed for UPSERT_PROJECT ${journal.projectId}, keeping journal")
                    return
                }
                if (!stateStore.addPublishedProjectId(journal.projectId)) {
                    DiagnosticsLogger.w(TAG, "Recover cleanup: addPublishedProjectId failed for UPSERT_PROJECT ${journal.projectId}, keeping journal")
                    return
                }
                // #649 评论 5563333323 缺口 3：调用统一 cleanup 函数，不复制两份逻辑。
                // allLiveKeys = journal.newEntries.keys：只清理已删除章节的旧 ref。
                if (cleanupCommittedTransaction(journal, storage, allLiveKeys = journal.newEntries.keys)) {
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
            MirrorTransactionType.DELETE_PROJECT -> {
                // 1. 确保 manifest 已提交成不引用该项目
                //    #649 评论 5562462046 问题 4：区分 manifest 是否已提交
                //    #649 评论 5562715833 问题 1：改用事务 manifest 路径传 snapshot=null
                //    #649 评论 5562715833 问题 6：isManifestCommitted=true 时不再调 publishManifest，直接 cleanup
                if (!journal.isManifestCommitted) {
                    // manifest 事务未完成：构造 desiredEntries 手动排除被删项目，走事务 manifest 路径
                    val desiredWithoutDeleted = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
                    for ((key, entry) in journal.oldEntries) {
                        if (key.projectId != journal.projectId) {
                            desiredWithoutDeleted[key] = entry
                        }
                    }
                    val manifestResult =
                        publishManifestWithDesiredTransactional(
                            projectId = journal.projectId,
                            snapshot = null,
                            desiredEntries = desiredWithoutDeleted,
                            txId = journal.txId,
                            journalContext = journal,
                            items = journal.items,
                            storage = storage,
                        )
                    if (manifestResult == null) {
                        DiagnosticsLogger.w(
                            TAG,
                            "Recover cleanup: manifest rewrite failed for DELETE_PROJECT ${journal.projectId}",
                        )
                        return
                    }
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
                if (cleanupCommittedTransaction(journal, storage, allLiveKeys = null)) {
                    // #649 评论 5564820566 问题 5：delete 成功后移除 publishedProjectId
                    // #649 评论 5565067997 修复 6：检查 removePublishedProjectId 返回值
                    if (!stateStore.removePublishedProjectId(journal.projectId)) {
                        DiagnosticsLogger.w(
                            TAG,
                            "Recover cleanup: removePublishedProjectId failed for ${journal.projectId}, " +
                                "keeping journal",
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
        }
    }

    /**
     * 统一清理已提交事务的残留文件。
     *
     * #649 评论 5563333323 缺口 3：正常 publishProject() 和 recoverCleanupPhase() 共用此函数，
     * 不复制两份逻辑。任一必须删除的项失败时返回 false，调用方不清 journal，下次 recover 继续清。
     *
     * #649 评论 5564379115 问题 3：backupCommitted() 是移动 old，不是复制。
     * oldRef 和 backupOldRef 指向同一 MediaStore row（只是路径变了），不能两个都删。
     * - 对已完成 swap 的 item：只删 backupOldRef（old 已被移走，不在 final 路径）；
     *   final 路径上是 promotedRef，promotedRef 由 stateStore 跟踪，不是 cleanup 目标。
     * - 对"章节已从 Core 删除、从未进入本轮 item"的 journal.oldEntries：单独删旧 ref。
     * - 删除前先 resolve() 确认文件仍存在，不存在视为目标已达到（幂等）。
     *
     * 处理：
     * - 每个 COMMITTED item 的 backupOldRef（旧正文备份，已被 backupCommitted 移到 backup 区）
     * - 已删除章节的旧 ref（snapshot 中已不存在的旧 key）
     * - manifestBackupRef（manifest 事务备份）
     * - tx staging/backup 根（事务暂存目录）
     *
     * @param journal 已提交的 journal
     * @param storage 当前事务的 storage
     * @param allLiveKeys 当前 Core 中仍存在的章节 key 集合（用于清理已删除章节的旧 ref）；
     *   null 表示不清理已删除章节（DELETE_PROJECT 场景，oldEntries 全部要删）。
     * @return true 表示全部清理成功；false 表示有失败项（保留 journal，下次继续清）。
     */
    private fun cleanupCommittedTransaction(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
        allLiveKeys: Set<ChapterKey>? = null,
    ): Boolean {
        var allSuccess = true
        // 1. 每个 COMMITTED item 的 backupOldRef（oldRef 已被 backupCommitted 移走，不删 oldRef）
        //    #649 评论 5564379115 问题 3：只删 backupOldRef，不要同时删 oldRef（同一 MediaStore row）
        //    #649 评论 5565067997 修复 5：用 lookup() 三态查询，Failed 时停止。
        for ((_, item) in journal.items) {
            try {
                item.backupOldRef?.let { ref ->
                    // #649 评论 5565067997 修复 5：用 lookup() 区分 Missing 和 Failed
                    when (val lookupResult = storage.lookup(ref.relativePath)) {
                        is MirrorLookupResult.Found -> {
                            if (!storage.delete(lookupResult.ref)) allSuccess = false
                        }
                        is MirrorLookupResult.Missing -> {
                            // 文件已不存在，目标已达到，视为成功
                        }
                        is MirrorLookupResult.Failed -> {
                            // 查询失败，不能当 Missing，保留 journal
                            DiagnosticsLogger.w(
                                TAG,
                                "cleanup: lookup backupOldRef failed ${ref.uri}: ${lookupResult.cause?.message}",
                            )
                            allSuccess = false
                        }
                    }
                }
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "cleanup: failed to delete backupOldRef ${item.backupOldRef?.uri}", e)
                allSuccess = false
            }
        }
        // 2. 已删除章节的旧 ref（snapshot 中已不存在的旧 key）
        //    先 lookup 确认文件存在再删，避免 MediaStore 同一条 row 被删两次
        //    #649 评论 5565067997 修复 5：用 lookup() 三态查询
        for ((key, entry) in journal.oldEntries) {
            val shouldDelete = allLiveKeys?.let { key !in it } ?: true
            if (shouldDelete) {
                try {
                    when (val lookupResult = storage.lookup(entry.relativePath)) {
                        is MirrorLookupResult.Found -> {
                            if (!storage.delete(lookupResult.ref)) allSuccess = false
                        }
                        is MirrorLookupResult.Missing -> {
                            // 文件已不存在，目标已达到
                        }
                        is MirrorLookupResult.Failed -> {
                            DiagnosticsLogger.w(
                                TAG,
                                "cleanup: lookup old entry failed ${entry.uri}: ${lookupResult.cause?.message}",
                            )
                            allSuccess = false
                        }
                    }
                } catch (e: Exception) {
                    DiagnosticsLogger.w(TAG, "cleanup: failed to delete old entry ${entry.uri}", e)
                    allSuccess = false
                }
                // 同时从 stateStore 移除该条目
                // #649 评论 5564379115 问题 3：必须检查 removeChapterEntry 返回值
                try {
                    if (!stateStore.removeChapterEntry(key.projectId, key.volumeId, key.chapterId)) {
                        allSuccess = false
                    }
                } catch (e: Exception) {
                    DiagnosticsLogger.w(TAG, "cleanup: failed to removeChapterEntry for ${key.chapterId}", e)
                    allSuccess = false
                }
            }
        }
        // 3. manifestBackupRef（manifest 事务备份）
        //    #649 评论 5565067997 修复 5：用 lookup() 三态查询
        try {
            journal.manifestBackupRef?.let { ref ->
                when (val lookupResult = storage.lookup(ref.relativePath)) {
                    is MirrorLookupResult.Found -> {
                        if (!storage.delete(lookupResult.ref)) allSuccess = false
                    }
                    is MirrorLookupResult.Missing -> {
                        // 文件已不存在，目标已达到
                    }
                    is MirrorLookupResult.Failed -> {
                        DiagnosticsLogger.w(
                            TAG,
                            "cleanup: lookup manifestBackupRef failed ${ref.uri}: ${lookupResult.cause?.message}",
                        )
                        allSuccess = false
                    }
                }
            }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to delete manifestBackupRef ${journal.manifestBackupRef?.uri}", e)
            allSuccess = false
        }
        // 4. tx staging/backup 根（事务暂存目录）
        //    #649 评论 5566303837 问题 6：检查 rollback 返回值，失败时不清 journal
        try {
            if (!storage.rollback(journal.txId)) {
                DiagnosticsLogger.w(TAG, "cleanup: tx staging cleanup failed for ${journal.txId}")
                allSuccess = false
            }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to rollback tx ${journal.txId}", e)
            allSuccess = false
        }
        return allSuccess
    }

    /**
     * 发布单章正文。
     *
     * #649 评论 5562715833 问题 6：单章发布委托 [publishProject] 的事务性路径，
     * 不另走非事务简化路径（旧 writeChapterContent + publishManifest）。
     * 统一走 stage → promote → 事务性 manifest → cleanup → journal 的事务性发布。
     *
     * @return 发布结果 [MirrorPublishResult]
     */
    suspend fun publishChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): MirrorPublishResult {
        return publishProject(projectId)
    }

    /**
     * 发布整个项目：事务性发布流程。
     *
     * #649 评论 5561465552 第 4 点：准备 → 写入 → 提交 manifest → 清旧文件。
     * #649 评论 5561974464 问题 2：publishManifest() 仍用旧 state，且"失败保留旧镜像"不成立。
     *
     * 1. **准备**：读完整快照和所有章节正文到内存，计算 desired state。
     * 2. **暂存**：所有新正文先写到 staging（不能覆盖 committed ref）。
     * 3. **提升**：promote 所有暂存文件到最终位置。
     * 4. **提交 manifest**：用 desiredEntries 直接构造 manifest，manifest 成功后才批量更新 stateStore。
     * 5. **清理**：删除不再被引用的旧文件。
     *
     * pendingPublish journal 在整个流程中记录进度，成功后清除。
     *
     * @return 发布结果 [MirrorPublishResult]
     */
    suspend fun publishProject(projectId: String): MirrorPublishResult {
        try {
            // #649 评论 5564379115 问题 4：每次开启新事务前都要检查当前 journal，
            // 不再用一次性 AtomicBoolean 表示"整个进程以后都没有 pending"。
            if (!ensurePendingRecovered()) {
                return MirrorPublishResult.PendingRecovery
            }
            // #649 评论 5565862745 问题 4：使用 currentTransactionResult() 获取完整事务上下文
            // 事务开始时拿一次 backend/treeUri/storage，整笔事务复用此 context
            val txContextResult = router.currentTransactionResult()
            if (txContextResult.isFailure) {
                // state.json 损坏或 DOCUMENT_TREE + treeUri=null
                val error = txContextResult.exceptionOrNull()
                DiagnosticsLogger.e(TAG, "Failed to get transaction context: ${error?.message}")
                return MirrorPublishResult.RetryableFailure
            }
            val txContext = txContextResult.getOrThrow()
            val storage = txContext.storage
            if (!storage.isSupported()) {
                DiagnosticsLogger.i(TAG, SKIP_NOT_SUPPORTED)
                return MirrorPublishResult.RetryableFailure
            }
            val snapshotResult = source.getProjectWorkspaceSnapshot(projectId)
            if (snapshotResult !is BridgeResult.Success) {
                logNotLoaded(snapshotResult, "publishProject")
                return MirrorPublishResult.RetryableFailure
            }
            val snapshot = snapshotResult.data
            val oldEntries = stateStore.getProjectEntries(projectId)

            // 1. 准备阶段：读完整快照和所有章节正文到内存，计算 desired state
            val allKeys = mutableSetOf<ChapterKey>()
            for (volumeWithChapters in snapshot.volumes) {
                for (chapter in volumeWithChapters.chapters) {
                    allKeys.add(ChapterKey(projectId, volumeWithChapters.volume.id, chapter.id))
                }
            }
            val usedRelativePaths = mutableSetOf<String>()
            val writePlan = buildWritePlan(projectId, snapshot, oldEntries, usedRelativePaths)
            if (writePlan == null) {
                DiagnosticsLogger.w(TAG, "Publish project $projectId aborted: failed to build write plan")
                return MirrorPublishResult.RetryableFailure
            }

            // 生成事务 ID
            val txId = "${System.currentTimeMillis()}-${projectId.take(8)}"

            // 2. 暂存阶段：所有新正文先写到 staging（不能覆盖 committed ref）
            val stagedRefs = mutableMapOf<ChapterKey, StagedMirrorRef>()
            val desiredEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
            val items = mutableMapOf<ChapterKey, PendingItem>()
            for (planEntry in writePlan) {
                val contentHash = computeContentHash(planEntry.content)
                val staged =
                    storage.stageText(
                        txId = txId,
                        relativePath = planEntry.relativePath,
                        mimeType = MIME_MARKDOWN,
                        text = planEntry.content,
                    )
                if (staged == null) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Publish project $projectId aborted: stage failed for ${planEntry.key.chapterId}",
                    )
                    storage.rollback(txId)
                    return MirrorPublishResult.RetryableFailure
                }
                stagedRefs[planEntry.key] = staged
                // 记录 desired entry（promote 后填 URI）
                desiredEntries[planEntry.key] =
                    ChapterMirrorEntry(
                        uri = "",
                        relativePath = planEntry.relativePath,
                        revision = planEntry.chapter.updatedAt.toEpochMillis(),
                        contentHash = contentHash,
                    )
                val oldRef =
                    planEntry.oldEntry?.let { MirrorFileRef(uri = it.uri, relativePath = it.relativePath) }
                items[planEntry.key] =
                    PendingItem(
                        key = planEntry.key,
                        stagedRef = staged,
                        oldRef = oldRef,
                        backupOldRef = null,
                        promotedRef = null,
                        state = PendingItem.STATE_STAGED,
                        // #649 评论 5566303837 问题 2：记录旧正文 hash 用于崩溃恢复校验
                        oldContentHash = planEntry.oldEntry?.contentHash,
                    )
            }

            // 写 pendingPublish journal（记录 staging 完成）
            // #649 评论 5563333323 缺口 2：journal 写入失败则停止本轮镜像操作
            // 使用 txContext 中的 backend/treeUri，不再从 stateStore 读取（#649 评论 5565862745 问题 4）
            if (!writePendingPublishJournal(
                    projectId = projectId,
                    transactionType = MirrorTransactionType.UPSERT_PROJECT,
                    phase = PendingMirrorPublish.PHASE_PROMOTE,
                    txId = txId,
                    backend = txContext.backend,
                    treeUri = txContext.treeUri,
                    oldEntries = oldEntries,
                    newEntries = desiredEntries,
                    stagedRefs = stagedRefs,
                    items = items,
                    removedProjectIds = emptySet(),
                    manifestOldRef = null,
                    manifestStagedRef = null,
                    manifestNewRef = null,
                    manifestBackupRef = null,
                    manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                )
            ) {
                DiagnosticsLogger.w(TAG, "Publish project $projectId aborted: journal write failed after stage")
                storage.rollback(txId)
                return MirrorPublishResult.RetryableFailure
            }

            // #649 评论 5564624383 问题 2：journalContext 在 promote 循环前创建，
            // 让 promote 失败时能传给 rollbackWholePublishTransaction 写 rollback journal
            // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
            val journalContext =
                PendingMirrorPublish(
                    txId = txId,
                    backend = txContext.backend,
                    treeUri = txContext.treeUri,
                    projectId = projectId,
                    transactionType = MirrorTransactionType.UPSERT_PROJECT,
                    phase = PendingMirrorPublish.PHASE_PROMOTE,
                    oldEntries = oldEntries,
                    newEntries = desiredEntries,
                    stagedRefs = stagedRefs,
                    items = items,
                    removedProjectIds = emptySet(),
                    manifestOldRef = null,
                    manifestStagedRef = null,
                    manifestNewRef = null,
                    manifestBackupRef = null,
                )

            // 3. 提升阶段：promote 所有暂存文件到最终位置（逐项更新 journal）
            // #649 评论 5564820566 问题 3：两步 journalable backup — prepareBackup + vacateCommitted
            val promotedEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
            for ((key, staged) in stagedRefs) {
                val item = items[key]!!
                // 已完成则跳过（恢复场景）
                if (item.state == PendingItem.STATE_PROMOTED && item.promotedRef != null) {
                    promotedEntries[key] =
                        ChapterMirrorEntry(
                            uri = item.promotedRef.uri,
                            relativePath = item.promotedRef.relativePath,
                            revision = desiredEntries[key]!!.revision,
                            contentHash = desiredEntries[key]!!.contentHash,
                        )
                    continue
                }
                val oldRef = item.oldRef
                // 1. 两步备份 old（如果有）
                // #649 评论 5565067997 修复 1：用 STATE_BACKUP_READY / STATE_OLD_VACATED 显式状态
                var oldVacated = false
                if (oldRef != null && item.state != PendingItem.STATE_OLD_VACATED) {
                    // 检查崩溃窗口：backup 已就绪但 vacate 未完成
                    // 使用 lookupBackup 三态查询（#649 评论 5565862745 问题 3）
                    val backupResult = storage.lookupBackup(txId, oldRef.relativePath)
                    val backupReady =
                        when (backupResult) {
                            is MirrorLookupResult.Found -> {
                                // backup 已存在，直接复用
                                // #649 评论 5565067997 修复 5：用 lookup() 三态查询判断 old 是否已 vacate
                                val oldLookup = storage.lookup(oldRef.relativePath)
                                when (oldLookup) {
                                    is MirrorLookupResult.Missing -> {
                                        oldVacated = true
                                        BackupReadyRef(backupRef = backupResult.ref, vacated = true)
                                    }
                                    is MirrorLookupResult.Found -> {
                                        oldVacated = false
                                        BackupReadyRef(backupRef = backupResult.ref, vacated = false)
                                    }
                                    is MirrorLookupResult.Failed -> {
                                        DiagnosticsLogger.w(
                                            TAG,
                                            "Publish project $projectId aborted: " +
                                                "lookup old failed for ${key.chapterId}: ${oldLookup.cause?.message}",
                                        )
                                        rollbackWholePublishTransaction(
                                            txId,
                                            items,
                                            stagedRefs,
                                            storage,
                                            journalContext,
                                        )
                                        return MirrorPublishResult.RetryableFailure
                                    }
                                }
                            }
                            is MirrorLookupResult.Missing -> {
                                // backup 不存在，需要 prepareBackup
                                val prepared = storage.prepareBackup(txId, oldRef, MIME_MARKDOWN)
                                if (prepared == null) {
                                    DiagnosticsLogger.w(
                                        TAG,
                                        "Publish project $projectId aborted: " +
                                            "backup prepare failed for ${key.chapterId}",
                                    )
                                    rollbackWholePublishTransaction(txId, items, stagedRefs, storage, journalContext)
                                    return MirrorPublishResult.RetryableFailure
                                }
                                oldVacated = prepared.vacated
                                prepared
                            }
                            is MirrorLookupResult.Failed -> {
                                DiagnosticsLogger.w(
                                    TAG,
                                    "Publish project $projectId aborted: " +
                                        "lookupBackup failed for ${key.chapterId}: ${backupResult.cause?.message}",
                                )
                                rollbackWholePublishTransaction(txId, items, stagedRefs, storage, journalContext)
                                return MirrorPublishResult.RetryableFailure
                            }
                        }
                    // #649 评论 5565067997 修复 1：journal 先写 STATE_BACKUP_READY
                    items[key] = item.copy(backupOldRef = backupReady.backupRef, state = PendingItem.STATE_BACKUP_READY)
                    // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
                    if (!writePendingPublishJournal(
                            projectId = projectId,
                            transactionType = MirrorTransactionType.UPSERT_PROJECT,
                            phase = PendingMirrorPublish.PHASE_PROMOTE,
                            txId = txId,
                            backend = txContext.backend,
                            treeUri = txContext.treeUri,
                            oldEntries = oldEntries,
                            newEntries = desiredEntries,
                            stagedRefs = stagedRefs,
                            items = items,
                            removedProjectIds = emptySet(),
                            manifestOldRef = null,
                            manifestStagedRef = null,
                            manifestNewRef = null,
                            manifestBackupRef = null,
                            manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                            journalContext = journalContext,
                        )
                    ) {
                        DiagnosticsLogger.w(
                            TAG,
                            "Publish project $projectId aborted: " +
                                "journal write failed after backup prepare for ${key.chapterId}",
                        )
                        rollbackWholePublishTransaction(txId, items, stagedRefs, storage, journalContext)
                        return MirrorPublishResult.RetryableFailure
                    }
                    // 2. vacate old（如果 prepareBackup 还没 move old）
                    if (!oldVacated) {
                        if (!storage.vacateCommitted(oldRef)) {
                            DiagnosticsLogger.w(
                                TAG,
                                "Publish project $projectId aborted: vacate failed for ${key.chapterId}",
                            )
                            rollbackWholePublishTransaction(txId, items, stagedRefs, storage, journalContext)
                            return MirrorPublishResult.RetryableFailure
                        }
                    }
                    // #649 评论 5565067997 修复 1：vacate 成功后写 STATE_OLD_VACATED
                    items[key] = items[key]!!.copy(state = PendingItem.STATE_OLD_VACATED)
                    // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
                    if (!writePendingPublishJournal(
                            projectId = projectId,
                            transactionType = MirrorTransactionType.UPSERT_PROJECT,
                            phase = PendingMirrorPublish.PHASE_PROMOTE,
                            txId = txId,
                            backend = txContext.backend,
                            treeUri = txContext.treeUri,
                            oldEntries = oldEntries,
                            newEntries = desiredEntries,
                            stagedRefs = stagedRefs,
                            items = items,
                            removedProjectIds = emptySet(),
                            manifestOldRef = null,
                            manifestStagedRef = null,
                            manifestNewRef = null,
                            manifestBackupRef = null,
                            manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                            journalContext = journalContext,
                        )
                    ) {
                        DiagnosticsLogger.w(
                            TAG,
                            "Publish project $projectId aborted: " +
                                "journal write failed after vacate for ${key.chapterId}",
                        )
                        rollbackWholePublishTransaction(txId, items, stagedRefs, storage, journalContext)
                        return MirrorPublishResult.RetryableFailure
                    }
                }
                // 2. promote staged（不删 old）
                //    #649 评论 5566303837 问题 2：OLD_VACATED 崩溃窗口检查
                //    promote 成功后 journal 可能还没写 PROMOTED，final 上可能已有新内容
                var newRef: MirrorFileRef? = null
                if (item.state == PendingItem.STATE_OLD_VACATED) {
                    val finalLookup = storage.lookup(staged.finalRelativePath)
                    if (finalLookup is MirrorLookupResult.Found) {
                        // final 已存在，校验是否是本事务的新正文
                        val expectedHash = desiredEntries[key]?.contentHash
                        if (expectedHash != null) {
                            val hashResult = storage.readTextAndHash(finalLookup.ref)
                            if (hashResult != null) {
                                val (_, hash) = hashResult
                                if (hash == expectedHash) {
                                    // final 已是本事务的新正文 → promote 已完成，直接复用
                                    newRef = finalLookup.ref
                                }
                            }
                        }
                    }
                }
                if (newRef == null) {
                    newRef = storage.promoteStaged(staged, staged.finalRelativePath)
                }
                if (newRef == null) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Publish project $projectId aborted: promote failed for ${key.chapterId}",
                    )
                    // #649 评论 5564379115 问题 2：统一事务回滚，不逐 item 回滚
                    rollbackWholePublishTransaction(txId, items, stagedRefs, storage, journalContext)
                    return MirrorPublishResult.RetryableFailure
                }
                promotedEntries[key] =
                    ChapterMirrorEntry(
                        uri = newRef.uri,
                        relativePath = newRef.relativePath,
                        revision = desiredEntries[key]!!.revision,
                        contentHash = desiredEntries[key]!!.contentHash,
                    )
                // 逐项更新 journal（记录该 item 已 PROMOTED）
                items[key] = items[key]!!.copy(promotedRef = newRef, state = PendingItem.STATE_PROMOTED)
                // #649 评论 5563333323 缺口 2：journal 写入失败则停止
                // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
                if (!writePendingPublishJournal(
                        projectId = projectId,
                        transactionType = MirrorTransactionType.UPSERT_PROJECT,
                        phase = PendingMirrorPublish.PHASE_PROMOTE,
                        txId = txId,
                        backend = txContext.backend,
                        treeUri = txContext.treeUri,
                        oldEntries = oldEntries,
                        newEntries = desiredEntries,
                        stagedRefs = stagedRefs,
                        items = items,
                        removedProjectIds = emptySet(),
                        manifestOldRef = null,
                        manifestStagedRef = null,
                        manifestNewRef = null,
                        manifestBackupRef = null,
                        manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                        journalContext = journalContext,
                    )
                ) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Publish project $projectId aborted: journal write failed after promote for ${key.chapterId}",
                    )
                    // #649 评论 5564379115 问题 2：统一事务回滚，不逐 item 回滚
                    rollbackWholePublishTransaction(txId, items, stagedRefs, storage, journalContext)
                    return MirrorPublishResult.RetryableFailure
                }
            }

            // 4. 提交 manifest：走事务性 manifest 写入（stage → promote → setManifestUri → 删 backup）
            //    #649 评论 5562715833 问题 5：传 journalContext，manifest 事务每步落 journal
            val manifestResult =
                publishManifestWithDesiredTransactional(
                    projectId = projectId,
                    snapshot = snapshot,
                    desiredEntries = promotedEntries,
                    txId = txId,
                    journalContext = journalContext,
                    items = items,
                    storage = storage,
                )
            if (manifestResult == null) {
                DiagnosticsLogger.w(TAG, "Publish project $projectId aborted: manifest write failed")
                // #649 评论 5564379115 问题 2：统一事务回滚
                rollbackWholePublishTransaction(txId, items, stagedRefs, storage, journalContext)
                return MirrorPublishResult.RetryableFailure
            }
            // #649 评论 5573310799 问题 5：先写 PHASE_CLEANUP journal（在 stateStore 更新之前），
            //    这样死在 stateStore 已更新、cleanup journal 还没落盘之间，磁盘 journal 仍是 PHASE_PROMOTE，
            //    下次恢复走 rollback 只恢复 old 章节，不会把本次新建章节从 stateStore 移掉。
            //    journal 里放 committedItems + promotedEntries + manifest committed 状态。
            // 标记所有 item 为 COMMITTED，更新 journal 到 cleanup 阶段
            val committedItems = items.mapValues { it.value.copy(state = PendingItem.STATE_COMMITTED) }
            // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
            if (!writePendingPublishJournal(
                    projectId = projectId,
                    transactionType = MirrorTransactionType.UPSERT_PROJECT,
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    txId = txId,
                    backend = txContext.backend,
                    treeUri = txContext.treeUri,
                    oldEntries = oldEntries,
                    newEntries = promotedEntries,
                    stagedRefs = emptyMap(),
                    items = committedItems,
                    removedProjectIds = emptySet(),
                    manifestOldRef = manifestResult.manifestOldRef,
                    manifestStagedRef = manifestResult.manifestStagedRef,
                    manifestNewRef = manifestResult.newRef,
                    manifestBackupRef = manifestResult.backupOldRef,
                    isManifestCommitted = true,
                    manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                    manifestNewContentHash = manifestResult.manifestNewContentHash,
                    manifestOldContentHash = manifestResult.manifestOldContentHash,
                )
            ) {
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: cleanup journal write failed, keeping journal for retry",
                )
                return MirrorPublishResult.RetryableFailure
            }
            // journal 落盘后再更新 stateStore（cleanup 阶段幂等执行）
            // #649 评论 5563333323 缺口 2：putChapterEntries 失败也不清 journal
            if (!stateStore.putChapterEntries(promotedEntries)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: putChapterEntries failed, keeping journal for retry",
                )
                return MirrorPublishResult.RetryableFailure
            }
            // #649 评论 5564820566 问题 5：manifest 提交成功后标记作品已发布，
            // 让零章节作品在 cleanupStaleProjects 中也能被找到。
            if (!stateStore.addPublishedProjectId(projectId)) {
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: addPublishedProjectId failed, keeping journal for retry",
                )
                return MirrorPublishResult.RetryableFailure
            }

            // 5. 清理阶段：调用统一 cleanup 函数
            //    #649 评论 5563333323 缺口 3：统一 cleanupCommittedTransaction
            // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
            val cleanupJournal =
                PendingMirrorPublish(
                    txId = txId,
                    backend = txContext.backend,
                    treeUri = txContext.treeUri,
                    projectId = projectId,
                    transactionType = MirrorTransactionType.UPSERT_PROJECT,
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    oldEntries = oldEntries,
                    newEntries = promotedEntries,
                    stagedRefs = emptyMap(),
                    items = committedItems,
                    removedProjectIds = emptySet(),
                    manifestOldRef = manifestResult.manifestOldRef,
                    manifestStagedRef = manifestResult.manifestStagedRef,
                    manifestNewRef = manifestResult.newRef,
                    manifestBackupRef = manifestResult.backupOldRef,
                    isManifestCommitted = true,
                    manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                )
            if (cleanupCommittedTransaction(cleanupJournal, storage, allLiveKeys = allKeys)) {
                // 全部清理成功，清除 journal
                stateStore.clearPendingPublish()
            } else {
                // 有失败项，保留 journal，下次 recover 继续清
                DiagnosticsLogger.w(
                    TAG,
                    "Publish project $projectId: cleanup partial failure, keeping journal for retry",
                )
                return MirrorPublishResult.RetryableFailure
            }
            return MirrorPublishResult.Committed
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish project: ${e.message}", e)
            // 异常时保留 journal，下次启动可继续
            return MirrorPublishResult.RetryableFailure
        }
    }

    /**
     * 删除项目镜像：先让新 manifest 不再引用旧项目，manifest 成功后再删旧 URI。
     *
     * #649 评论 5561465552 第 4 点：不能先删正文再尝试写 manifest。
     * #649 评论 5561974464 问题 3：确保 deleteProject() 也走同一套 mirror transaction。
     * #649 评论 5562462046 问题 4：正确顺序——先写 journal(transactionType=DELETE_PROJECT, phase=CLEANUP)，
     * 再事务提交"不含该项目"的 manifest，manifest 成功后才从 stateStore 删项目条目，最后删旧正文。
     */
    suspend fun deleteProject(projectId: String): MirrorPublishResult {
        try {
            // 门控：确保 pending 已恢复
            if (!ensurePendingRecovered()) {
                return MirrorPublishResult.PendingRecovery
            }
            // #649 评论 5565862745 问题 4：使用 currentTransactionResult() 获取完整事务上下文
            val txContextResult = router.currentTransactionResult()
            if (txContextResult.isFailure) {
                val error = txContextResult.exceptionOrNull()
                DiagnosticsLogger.e(TAG, "Failed to get transaction context for delete: ${error?.message}")
                return MirrorPublishResult.RetryableFailure
            }
            val txContext = txContextResult.getOrThrow()
            val storage = txContext.storage
            if (!storage.isSupported()) {
                DiagnosticsLogger.i(TAG, "Mirror delete skipped: storage not supported")
                return MirrorPublishResult.RetryableFailure
            }
            // 1. 获取旧条目
            val removed = stateStore.getProjectEntries(projectId)
            // #649 评论 5562715833 问题 7：不在 removed.isEmpty() 时 early return，
            // 即使空作品也继续走事务流程，提交 snapshot=null 的新 manifest（确保 manifest 不再引用该项目）
            // 2. 写 pending journal（transactionType=DELETE_PROJECT, phase=CLEANUP）
            //    #649 评论 5563333323 缺口 2：journal 写入失败则停止
            val txId = "${System.currentTimeMillis()}-${projectId.take(8)}"
            // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
            if (!writePendingPublishJournal(
                    projectId = projectId,
                    transactionType = MirrorTransactionType.DELETE_PROJECT,
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    txId = txId,
                    backend = txContext.backend,
                    treeUri = txContext.treeUri,
                    oldEntries = removed,
                    newEntries = emptyMap(),
                    stagedRefs = emptyMap(),
                    items = emptyMap(),
                    removedProjectIds = setOf(projectId),
                    manifestOldRef = null,
                    manifestStagedRef = null,
                    manifestNewRef = null,
                    manifestBackupRef = null,
                    manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                )
            ) {
                DiagnosticsLogger.w(TAG, "Delete project $projectId aborted: journal write failed")
                return MirrorPublishResult.RetryableFailure
            }
            // 3. 事务提交新 manifest（已不含该项目）
            //    用 desiredEntries=emptyMap 表示该项目不再有任何章节
            //    #649 评论 5562715833 问题 7：snapshot=null 确保 manifest 不再引用该项目
            val snapshotResult = source.getProjectWorkspaceSnapshot(projectId)
            val snapshot = (snapshotResult as? BridgeResult.Success)?.data
            // #649 评论 5562715833 问题 5：传 journalContext，manifest 事务每步落 journal
            // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
            val deleteJournalContext =
                PendingMirrorPublish(
                    txId = txId,
                    backend = txContext.backend,
                    treeUri = txContext.treeUri,
                    projectId = projectId,
                    transactionType = MirrorTransactionType.DELETE_PROJECT,
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    oldEntries = removed,
                    newEntries = emptyMap(),
                    stagedRefs = emptyMap(),
                    items = emptyMap(),
                    removedProjectIds = setOf(projectId),
                    manifestOldRef = null,
                    manifestStagedRef = null,
                    manifestNewRef = null,
                    manifestBackupRef = null,
                )
            val manifestResult =
                publishManifestWithDesiredTransactional(
                    projectId = projectId,
                    snapshot = snapshot,
                    desiredEntries = emptyMap(),
                    txId = txId,
                    journalContext = deleteJournalContext,
                    items = emptyMap(),
                    storage = storage,
                )
            if (manifestResult == null) {
                DiagnosticsLogger.w(TAG, "Delete project $projectId aborted: manifest write failed")
                // manifest 失败不清除 journal，下次恢复会重试
                return MirrorPublishResult.RetryableFailure
            }
            // 4. manifest 成功后更新 journal（标记 manifest 已提交）
            //    #649 评论 5562462046 问题 4：恢复时需区分 manifest 是否已提交
            //    #649 评论 5563333323 缺口 2：journal 写入失败则保留 journal 重试
            // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
            if (!writePendingPublishJournal(
                    projectId = projectId,
                    transactionType = MirrorTransactionType.DELETE_PROJECT,
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    txId = txId,
                    backend = txContext.backend,
                    treeUri = txContext.treeUri,
                    oldEntries = removed,
                    newEntries = emptyMap(),
                    stagedRefs = emptyMap(),
                    items = emptyMap(),
                    removedProjectIds = setOf(projectId),
                    manifestOldRef = manifestResult.manifestOldRef,
                    manifestStagedRef = manifestResult.manifestStagedRef,
                    manifestNewRef = manifestResult.newRef,
                    manifestBackupRef = manifestResult.backupOldRef,
                    isManifestCommitted = true,
                    manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                    manifestNewContentHash = manifestResult.manifestNewContentHash,
                    manifestOldContentHash = manifestResult.manifestOldContentHash,
                )
            ) {
                DiagnosticsLogger.w(
                    TAG,
                    "Delete project $projectId: cleanup journal write failed, keeping journal for retry",
                )
                return MirrorPublishResult.RetryableFailure
            }
            // 5. 从 state store 删除该项目条目
            //    #649 评论 5563333323 缺口 2：removeAllProjectEntries 返回 Result
            val removeResult = stateStore.removeAllProjectEntries(projectId)
            if (removeResult.isFailure) {
                DiagnosticsLogger.w(
                    TAG,
                    "Delete project $projectId: removeAllProjectEntries failed, keeping journal for retry",
                )
                return MirrorPublishResult.RetryableFailure
            }
            // 6. 调用统一 cleanup 删旧正文 + manifestBackup + tx staging
            //    #649 评论 5563333323 缺口 3：统一 cleanupCommittedTransaction
            // 使用 txContext 中的 backend/treeUri（#649 评论 5565862745 问题 4）
            val deleteCleanupJournal =
                PendingMirrorPublish(
                    txId = txId,
                    backend = txContext.backend,
                    treeUri = txContext.treeUri,
                    projectId = projectId,
                    transactionType = MirrorTransactionType.DELETE_PROJECT,
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    oldEntries = removed,
                    newEntries = emptyMap(),
                    stagedRefs = emptyMap(),
                    items = emptyMap(),
                    removedProjectIds = setOf(projectId),
                    manifestOldRef = manifestResult.manifestOldRef,
                    manifestStagedRef = manifestResult.manifestStagedRef,
                    manifestNewRef = manifestResult.newRef,
                    manifestBackupRef = manifestResult.backupOldRef,
                    isManifestCommitted = true,
                    manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                )
            if (cleanupCommittedTransaction(deleteCleanupJournal, storage, allLiveKeys = null)) {
                // #649 评论 5564820566 问题 5：delete 成功后移除 publishedProjectId
                // #649 评论 5565067997 修复 6：检查 removePublishedProjectId 返回值
                if (!stateStore.removePublishedProjectId(projectId)) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Delete project $projectId: removePublishedProjectId failed, keeping journal for retry",
                    )
                    return MirrorPublishResult.RetryableFailure
                }
                stateStore.clearPendingPublish()
            } else {
                DiagnosticsLogger.w(
                    TAG,
                    "Delete project $projectId: cleanup partial failure, keeping journal for retry",
                )
                return MirrorPublishResult.RetryableFailure
            }
            return MirrorPublishResult.Committed
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to delete project: ${e.message}", e)
            // 异常时保留 journal，下次启动可继续
            return MirrorPublishResult.RetryableFailure
        }
    }

    /**
     * 全量发布：遍历所有项目。
     *
     * @return 发布结果 [MirrorPublishResult]
     */
    suspend fun publishAll(): MirrorPublishResult {
        try {
            // 门控：确保 pending 已恢复
            if (!ensurePendingRecovered()) {
                return MirrorPublishResult.PendingRecovery
            }
            // #649 评论 5564379115 问题 5：用严格接口
            val storageResult = router.currentResult()
            if (storageResult.isFailure) {
                val error = storageResult.exceptionOrNull()
                DiagnosticsLogger.e(TAG, "Failed to get storage for publishAll: ${error?.message}")
                return MirrorPublishResult.RetryableFailure
            }
            val storage = storageResult.getOrThrow()
            if (!storage.isSupported()) {
                DiagnosticsLogger.i(TAG, SKIP_NOT_SUPPORTED)
                return MirrorPublishResult.RetryableFailure
            }
            val projectsResult = source.listProjects()
            if (projectsResult !is BridgeResult.Success) {
                logNotLoaded(projectsResult, "publishAll")
                return MirrorPublishResult.RetryableFailure
            }
            // #649 评论 5564624383 问题 6：检查 cleanupStaleProjects 结果
            val liveProjectIds = projectsResult.data.map { it.id }.toSet()
            val cleanupResult = cleanupStaleProjects(liveProjectIds)
            if (cleanupResult !is MirrorPublishResult.Committed) return cleanupResult
            // #649 评论 5564379115 问题 4：publishAll 在第一笔 publishProject/deleteProject
            // 返回非 Committed 时立刻停止，不能只记 anyFailure=true 后继续下一个项目
            for (project in projectsResult.data) {
                val result = publishProject(project.id)
                if (result !is MirrorPublishResult.Committed) {
                    return result
                }
            }
            return MirrorPublishResult.Committed
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish all: ${e.message}", e)
            return MirrorPublishResult.RetryableFailure
        }
    }

    /**
     * 清理 state store 中已不在 Core 的旧项目镜像。
     * 提取自 publishAll 以控制嵌套深度。
     *
     * #649 评论 5564624383 问题 6：返回结果并向上传播。
     * 旧实现吞掉 deleteProject() 的返回值，导致 stale 删除失败后 publishAll 仍返回 Committed，
     * Sink 会把 everythingChanged 事件移掉。
     */
    private suspend fun cleanupStaleProjects(liveProjectIds: Set<String>): MirrorPublishResult {
        for (staleProjectId in stateStore.getAllProjectIds()) {
            if (staleProjectId !in liveProjectIds) {
                val result = deleteProject(staleProjectId)
                if (result !is MirrorPublishResult.Committed) return result
            }
        }
        return MirrorPublishResult.Committed
    }

    /**
     * 从磁盘读取指定事务 ID 的最新 pending publish journal（#649 评论 5566303837 问题 1）。
     *
     * 返回 [LatestPending] 三态：Found（同一事务）、NotExists、CorruptedOrMismatch。
     * 用于 rollback 前获取最新 journal，避免使用调用方传入的旧对象。
     *
     * @param txId 事务 ID
     */
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

    private sealed interface RollbackItemResult {
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
    private fun rollbackChapterToOldState(
        journal: PendingMirrorPublish,
        key: ChapterKey,
        item: PendingItem,
        storage: ReadableMirrorStorage,
    ): RollbackItemResult {
        val staged = item.stagedRef ?: journal.stagedRefs[key]
        if (staged == null) {
            DiagnosticsLogger.w(TAG, "rollback: missing stagedRef for ${key.chapterId}")
            return RollbackItemResult.StateUnknown
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

        val finalLookup = storage.lookup(newFinalPath)
        when (finalLookup) {
            is MirrorLookupResult.Found -> {
                val hashResult = storage.readTextAndHash(finalLookup.ref)
                if (hashResult == null) {
                    DiagnosticsLogger.w(
                        TAG,
                        "rollback: readTextAndHash failed for ${key.chapterId}, cannot verify final identity",
                    )
                    return RollbackItemResult.StateUnknown
                }
                val (_, finalHash) = hashResult

                if (expectedOldHash != null) {
                    // 有旧正文：final Found 时，hash 是 old/new 中任意一个都删除并 restore backup
                    if (finalHash == expectedOldHash || finalHash == expectedNewHash) {
                        if (finalHash == expectedOldHash) {
                            DiagnosticsLogger.i(
                                TAG,
                                "rollback: final matches old hash for ${key.chapterId}, will delete and restore",
                            )
                        } else {
                            DiagnosticsLogger.w(
                                TAG,
                                "rollback: final matches new hash for ${key.chapterId}, will delete and restore",
                            )
                        }
                        val removed = storage.delete(finalLookup.ref)
                        if (!removed) {
                            DiagnosticsLogger.w(TAG, "rollback: delete final failed for ${key.chapterId}")
                            return RollbackItemResult.Failed
                        }
                    } else {
                        DiagnosticsLogger.w(
                            TAG,
                            "rollback: final hash mismatch (neither old nor new) for ${key.chapterId}, state unknown",
                        )
                        return RollbackItemResult.StateUnknown
                    }
                    // #649 评论 5572554935 问题 1+2：
                    // - lookupBackup/restoreBackup 用 oldFinalPath（旧标题路径），不是 newFinalPath
                    // - restoreBackupToFinal 直接返回 RestoreBackupResult，不折叠成 Boolean，
                    //   保留 Restored(ref) 里的真实 ref 给调用方写回 stateStore
                    val restoreResult =
                        restoreBackupToFinal(
                            journal,
                            key,
                            newFinalPath = newFinalPath,
                            oldFinalPath = oldFinalPath,
                            expectedOldHash = expectedOldHash,
                            storage = storage,
                        )
                    return when (restoreResult) {
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
                                "rollback: " +
                                    "failed to restore backup for ${key.chapterId}: ${restoreResult.cause?.message}",
                            )
                            RollbackItemResult.Failed
                        }
                    }
                } else {
                    // 没有旧正文（新建章节）
                    if (expectedNewHash == null) {
                        DiagnosticsLogger.w(
                            TAG,
                            "rollback: no expectedNewHash for new chapter ${key.chapterId}, state unknown",
                        )
                        return RollbackItemResult.StateUnknown
                    }
                    if (finalHash == expectedNewHash) {
                        DiagnosticsLogger.w(TAG, "rollback: new chapter final exists for ${key.chapterId}, will delete")
                        val removed = storage.delete(finalLookup.ref)
                        if (!removed) {
                            DiagnosticsLogger.w(TAG, "rollback: delete new chapter final failed for ${key.chapterId}")
                            return RollbackItemResult.Failed
                        }
                        return RollbackItemResult.NewFileRemoved
                    } else {
                        DiagnosticsLogger.w(
                            TAG,
                            "rollback: final exists for new chapter with unexpected hash for ${key.chapterId}, " +
                                "state unknown",
                        )
                        return RollbackItemResult.StateUnknown
                    }
                }
            }
            is MirrorLookupResult.Missing -> {
                if (expectedOldHash != null) {
                    val restoreResult =
                        restoreBackupToFinal(
                            journal,
                            key,
                            newFinalPath = newFinalPath,
                            oldFinalPath = oldFinalPath,
                            expectedOldHash = expectedOldHash,
                            storage = storage,
                        )
                    return when (restoreResult) {
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
                                "rollback: " +
                                    "failed to restore backup for ${key.chapterId}: ${restoreResult.cause?.message}",
                            )
                            RollbackItemResult.Failed
                        }
                    }
                } else {
                    DiagnosticsLogger.i(TAG, "rollback: final missing for new chapter ${key.chapterId}, target reached")
                    return RollbackItemResult.NewFileRemoved
                }
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback: lookup final failed for ${key.chapterId}: ${finalLookup.cause?.message}",
                )
                return RollbackItemResult.StateUnknown
            }
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

    private suspend fun rollbackWholePublishTransaction(
        txId: String,
        items: Map<ChapterKey, PendingItem>,
        stagedRefs: Map<ChapterKey, StagedMirrorRef>,
        storage: ReadableMirrorStorage,
        journalContext: PendingMirrorPublish? = null,
    ): Boolean {
        // 0. #649 评论 5566303837 问题 1：磁盘最新状态优先
        // 不能用 journalContext 覆盖磁盘上已经前进的事务状态
        val latestJournal =
            when (val latest = readLatestPendingForTxStrict(txId)) {
                is LatestPending.Found -> latest.journal
                LatestPending.NotExists -> {
                    // 磁盘无 journal，只能用调用方传入的 context 兜底
                    journalContext ?: return false
                }
                is LatestPending.CorruptedOrMismatch -> {
                    DiagnosticsLogger.w(TAG, "rollback: cannot read latest journal for tx $txId (corrupted/mismatch)")
                    return false
                }
            }

        // 0.1 写 rollback journal（让进程死在回滚中间时能继续回滚）
        // 只把 phase 改成 PHASE_ROLLBACK，其余字段沿用最新 journal 值
        // 合并磁盘 journal 和调用方 items（调用方有 rollback 状态推进）
        val mergedItems = latestJournal.items.toMutableMap()
        for ((key, callerItem) in items) {
            mergedItems[key] = callerItem
        }
        if (!writePendingPublishJournal(
                projectId = latestJournal.projectId,
                transactionType = latestJournal.transactionType,
                phase = PendingMirrorPublish.PHASE_ROLLBACK,
                txId = txId,
                backend = latestJournal.backend,
                treeUri = latestJournal.treeUri,
                oldEntries = latestJournal.oldEntries,
                newEntries = latestJournal.newEntries,
                stagedRefs = latestJournal.stagedRefs,
                items = mergedItems,
                removedProjectIds = latestJournal.removedProjectIds,
                manifestOldRef = latestJournal.manifestOldRef,
                manifestStagedRef = latestJournal.manifestStagedRef,
                manifestNewRef = latestJournal.manifestNewRef,
                manifestBackupRef = latestJournal.manifestBackupRef,
                manifestSwapState = latestJournal.manifestSwapState,
                journalContext = latestJournal,
            )
        ) {
            DiagnosticsLogger.w(TAG, "rollback: journal write failed at start")
            return false
        }

        // #649 评论 5564820566 问题 2：rollback 和 recovery 共用同一套显式状态机。
        // 逐项处理：每个 item 独立跟踪 rollback 进度，不靠 "final/backup 是否存在" 猜测。

        // 1. 幂等删除 promotedRef（跳过已处理的 item）
        //    #649 评论 5565067997 修复 4：检查 delete() 返回值，失败时停止推进状态
        val currentItems = mergedItems.toMutableMap()
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
            if (!writePendingPublishJournal(
                    projectId = latestJournal.projectId,
                    transactionType = latestJournal.transactionType,
                    phase = PendingMirrorPublish.PHASE_ROLLBACK,
                    txId = txId,
                    backend = latestJournal.backend,
                    treeUri = latestJournal.treeUri,
                    oldEntries = latestJournal.oldEntries,
                    newEntries = latestJournal.newEntries,
                    stagedRefs = latestJournal.stagedRefs,
                    items = currentItems,
                    removedProjectIds = latestJournal.removedProjectIds,
                    manifestOldRef = latestJournal.manifestOldRef,
                    manifestStagedRef = latestJournal.manifestStagedRef,
                    manifestNewRef = latestJournal.manifestNewRef,
                    manifestBackupRef = latestJournal.manifestBackupRef,
                    manifestSwapState = latestJournal.manifestSwapState,
                    journalContext = latestJournal,
                )
            ) {
                DiagnosticsLogger.w(TAG, "rollback: journal write failed after NEW_REMOVED for ${key.chapterId}")
                return false
            }
        }

        // 2. 逐个恢复 backupOldRef 到最终路径
        //    #649 评论 5570613481 问题 1：使用统一 helper，不再内联回滚逻辑。
        //    #649 评论 5573310799 问题 1：每章恢复成功后先写 stateStore 再标记 journal，
        //    消除断电丢失窗口（journal 已写 OLD_RESTORED、stateStore 还没写真实 URI 之间，
        //    重启会跳过该 item，真实 URI 永久丢失）。
        //    restoreBackup 可能因 createText/createDocument 返回新 URI，
        //    stateStore 仍保存事务开始前的旧 URI（可能已被 move 到 backup 最后被 cleanup 删除），
        //    下次镜像操作会拿到失效 URI，所以必须用真实 restoredRef.uri 替换。
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue

            val rollbackResult = rollbackChapterToOldState(latestJournal, key, item, storage)
            when (rollbackResult) {
                is RollbackItemResult.Restored -> {
                    // #649 评论 5573310799 问题 1：先把真实 URI 写进 stateStore，再标记 journal
                    val oldEntry = latestJournal.oldEntries[key] ?: run {
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
                        DiagnosticsLogger.w(TAG, "rollback: putChapterEntry failed for ${key.chapterId}, keeping journal")
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

            // 更新 journal（记录 OLD_RESTORED）
            if (!writePendingPublishJournal(
                    projectId = latestJournal.projectId,
                    transactionType = latestJournal.transactionType,
                    phase = PendingMirrorPublish.PHASE_ROLLBACK,
                    txId = txId,
                    backend = latestJournal.backend,
                    treeUri = latestJournal.treeUri,
                    oldEntries = latestJournal.oldEntries,
                    newEntries = latestJournal.newEntries,
                    stagedRefs = latestJournal.stagedRefs,
                    items = currentItems,
                    removedProjectIds = latestJournal.removedProjectIds,
                    manifestOldRef = latestJournal.manifestOldRef,
                    manifestStagedRef = latestJournal.manifestStagedRef,
                    manifestNewRef = latestJournal.manifestNewRef,
                    manifestBackupRef = latestJournal.manifestBackupRef,
                    manifestSwapState = latestJournal.manifestSwapState,
                    journalContext = latestJournal,
                )
            ) {
                DiagnosticsLogger.w(TAG, "rollback: journal write failed after OLD_RESTORED for ${key.chapterId}")
                return false
            }
        }

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

    /**
     * Manifest rollback helper（#649 评论 5565067997 修复 3）。
     *
     * 正确顺序：
     * 1. 如果 manifestNewRef 已 promote，先删除精确的 manifestNewRef，并确认成功
     * 2. final 名字腾空
     * 3. restoreBackup(old manifest)
     * 4. 拿 restoredRef
     * 5. setManifestUri(restoredRef.uri)，失败则保留 rollback journal
     *
     * 恢复旧 manifest 后绝对不能再 resolve(final).delete()。
     * [rollbackWholePublishTransaction] 和 [recoverRollbackPhase] 共用此 helper。
     *
     * #649 评论 5571899956 问题 3：manifest rollback 显式状态。
     * - 先删除新 manifest → 写 MANIFEST_ROLLBACK_NEW_REMOVED
     * - restore old backup → setManifestUri(restoredRef.uri) → 写 MANIFEST_ROLLBACK_OLD_RESTORED
     * - 如果重启时 final 已经是 old manifest，也必须先 setManifestUri(finalRef.uri) 再把 rollback 状态推进
     *
     * @param items 当前 items（用于写 journal）
     * @return true 表示 manifest rollback 成功（或不需要 rollback）；false 表示失败
     */
    private fun rollbackManifest(
        journalContext: PendingMirrorPublish?,
        storage: ReadableMirrorStorage,
        items: Map<ChapterKey, PendingItem>,
    ): Boolean {
        if (journalContext == null) return true
        // #649 评论 5573310799 问题 2：用 currentJournal 前进，每次写 journal 后更新，
        //    后续 writePendingPublishJournal 传 currentJournal（而非原始 journalContext），
        //    避免每次写 journal 都从原始状态重建。
        var currentJournal = journalContext
        val manifestRelativePath = "$META_DIR/$MANIFEST_FILE_NAME"
        // #649 评论 5572554935 问题 4：no-old manifest rollback 不能直接 return true。
        // 必须按是否有旧 manifest 分两条：
        // - manifestOldRef != null → 删除 txn new manifest → 恢复 old backup → setManifestUri(restoredOld.uri)
        // - manifestOldRef == null → 删除 txn new manifest → clearManifestUri() → 写 MANIFEST_ROLLBACK_OLD_RESTORED
        // 旧实现 `val backup = journalContext?.manifestBackupRef ?: return true` 在首次发布 manifest
        // （无旧 manifest，manifestBackupRef==null）时直接 return true，新 manifest/manifestUri 都可能残留。
        val manifestOldRef = journalContext.manifestOldRef
        val backup = journalContext.manifestBackupRef

        // 1. 先确认 final 上的 manifest 身份并处理（不依赖 manifestNewRef 是否已写进 journal）
        //    #649 评论 5573310799 问题 4：覆盖"new 已物理落到 final 但 manifestNewRef 还没写进 journal"的窗口。
        //    可达路径：promoteStaged() 已成功 → MANIFEST_PROMOTED/COMMITTED journal 写失败 →
        //    外层 rollback → 磁盘 journal 里 manifestNewRef 仍是 null → 旧逻辑不会删 final 上的新 manifest。
        //    改用已冻结的两个 hash 判身份。
        val finalLookup = storage.lookup(manifestRelativePath)
        when (finalLookup) {
            is MirrorLookupResult.Found -> {
                val hashResult = storage.readTextAndHash(finalLookup.ref)
                if (hashResult == null) {
                    DiagnosticsLogger.w(TAG, "rollback manifest: readTextAndHash failed for final, cannot verify identity")
                    return false
                }
                val (_, finalHash) = hashResult
                when {
                    finalHash == journalContext.manifestNewContentHash -> {
                        // final 是本事务新 manifest → 删除
                        if (!storage.delete(finalLookup.ref)) {
                            DiagnosticsLogger.w(TAG, "rollback manifest: delete final (new manifest) failed")
                            return false
                        }
                        // 写 MANIFEST_ROLLBACK_NEW_REMOVED
                        currentJournal = currentJournal.copy(
                            manifestNewRef = null,
                            manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_NEW_REMOVED,
                        )
                        if (!writePendingPublishJournal(
                                projectId = currentJournal.projectId,
                                transactionType = currentJournal.transactionType,
                                phase = PendingMirrorPublish.PHASE_ROLLBACK,
                                txId = currentJournal.txId,
                                backend = currentJournal.backend,
                                treeUri = currentJournal.treeUri,
                                oldEntries = currentJournal.oldEntries,
                                newEntries = currentJournal.newEntries,
                                stagedRefs = currentJournal.stagedRefs,
                                items = items,
                                removedProjectIds = currentJournal.removedProjectIds,
                                manifestOldRef = currentJournal.manifestOldRef,
                                manifestStagedRef = currentJournal.manifestStagedRef,
                                manifestNewRef = null,
                                manifestBackupRef = currentJournal.manifestBackupRef,
                                isManifestCommitted = currentJournal.isManifestCommitted,
                                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_NEW_REMOVED,
                                manifestNewContentHash = currentJournal.manifestNewContentHash,
                                manifestOldContentHash = currentJournal.manifestOldContentHash,
                                journalContext = currentJournal,
                            )
                        ) {
                            DiagnosticsLogger.w(TAG, "rollback manifest: journal write failed after NEW_REMOVED")
                            return false
                        }
                    }
                    finalHash == journalContext.manifestOldContentHash -> {
                        // final 已经是 old manifest → 直接 setManifestUri，跳过删除+restore
                        if (!stateStore.setManifestUri(finalLookup.ref.uri)) {
                            DiagnosticsLogger.w(TAG, "rollback manifest: setManifestUri failed (final already old)")
                            return false
                        }
                        currentJournal = currentJournal.copy(
                            manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                        )
                        if (!writePendingPublishJournal(
                                projectId = currentJournal.projectId,
                                transactionType = currentJournal.transactionType,
                                phase = PendingMirrorPublish.PHASE_ROLLBACK,
                                txId = currentJournal.txId,
                                backend = currentJournal.backend,
                                treeUri = currentJournal.treeUri,
                                oldEntries = currentJournal.oldEntries,
                                newEntries = currentJournal.newEntries,
                                stagedRefs = currentJournal.stagedRefs,
                                items = items,
                                removedProjectIds = currentJournal.removedProjectIds,
                                manifestOldRef = currentJournal.manifestOldRef,
                                manifestStagedRef = currentJournal.manifestStagedRef,
                                manifestNewRef = currentJournal.manifestNewRef,
                                manifestBackupRef = currentJournal.manifestBackupRef,
                                isManifestCommitted = currentJournal.isManifestCommitted,
                                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                                manifestNewContentHash = currentJournal.manifestNewContentHash,
                                manifestOldContentHash = currentJournal.manifestOldContentHash,
                                journalContext = currentJournal,
                            )
                        ) {
                            DiagnosticsLogger.w(TAG, "rollback manifest: journal write failed after OLD_RESTORED (final already old)")
                            return false
                        }
                        return true
                    }
                    else -> {
                        DiagnosticsLogger.w(TAG, "rollback manifest: final hash matches neither old nor new, state unknown")
                        return false
                    }
                }
            }
            is MirrorLookupResult.Missing -> {
                // final 已空，无需删除新 manifest
                // 如果 manifestNewRef 不为 null（journal 记录过），仍写一次 NEW_REMOVED 推进状态
                if (journalContext.manifestNewRef != null) {
                    currentJournal = currentJournal.copy(
                        manifestNewRef = null,
                        manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_NEW_REMOVED,
                    )
                    if (!writePendingPublishJournal(
                            projectId = currentJournal.projectId,
                            transactionType = currentJournal.transactionType,
                            phase = PendingMirrorPublish.PHASE_ROLLBACK,
                            txId = currentJournal.txId,
                            backend = currentJournal.backend,
                            treeUri = currentJournal.treeUri,
                            oldEntries = currentJournal.oldEntries,
                            newEntries = currentJournal.newEntries,
                            stagedRefs = currentJournal.stagedRefs,
                            items = items,
                            removedProjectIds = currentJournal.removedProjectIds,
                            manifestOldRef = currentJournal.manifestOldRef,
                            manifestStagedRef = currentJournal.manifestStagedRef,
                            manifestNewRef = null,
                            manifestBackupRef = currentJournal.manifestBackupRef,
                            isManifestCommitted = currentJournal.isManifestCommitted,
                            manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_NEW_REMOVED,
                            manifestNewContentHash = currentJournal.manifestNewContentHash,
                            manifestOldContentHash = currentJournal.manifestOldContentHash,
                            journalContext = currentJournal,
                        )
                    ) {
                        DiagnosticsLogger.w(TAG, "rollback manifest: journal write failed after NEW_REMOVED (final missing)")
                        return false
                    }
                }
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(TAG, "rollback manifest: lookup final failed: ${finalLookup.cause?.message}")
                return false
            }
        }

        // 2. 按是否有旧 manifest 分两条路径
        if (manifestOldRef == null) {
            // #649 评论 5572554935 问题 4：首次发布 manifest（无旧 manifest）的回滚。
            // 正确结果：stateStore 不再持有 manifestUri（恢复成"没有 manifest"）。
            // 旧实现直接 return true，manifestUri 仍指向新 manifest。
            if (!stateStore.clearManifestUri()) {
                DiagnosticsLogger.w(TAG, "rollback manifest: clearManifestUri failed (no-old manifest rollback)")
                return false
            }
            // 写 MANIFEST_ROLLBACK_OLD_RESTORED（语义：已恢复到"没有 manifest"的初始状态）
            currentJournal = currentJournal.copy(
                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
            )
            if (!writePendingPublishJournal(
                    projectId = currentJournal.projectId,
                    transactionType = currentJournal.transactionType,
                    phase = PendingMirrorPublish.PHASE_ROLLBACK,
                    txId = currentJournal.txId,
                    backend = currentJournal.backend,
                    treeUri = currentJournal.treeUri,
                    oldEntries = currentJournal.oldEntries,
                    newEntries = currentJournal.newEntries,
                    stagedRefs = currentJournal.stagedRefs,
                    items = items,
                    removedProjectIds = currentJournal.removedProjectIds,
                    manifestOldRef = currentJournal.manifestOldRef,
                    manifestStagedRef = currentJournal.manifestStagedRef,
                    manifestNewRef = currentJournal.manifestNewRef,
                    manifestBackupRef = currentJournal.manifestBackupRef,
                    isManifestCommitted = currentJournal.isManifestCommitted,
                    manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                    manifestNewContentHash = currentJournal.manifestNewContentHash,
                    manifestOldContentHash = currentJournal.manifestOldContentHash,
                    journalContext = currentJournal,
                )
            ) {
                DiagnosticsLogger.w(TAG, "rollback manifest: journal write failed after OLD_RESTORED (no-old)")
                return false
            }
            return true
        }

        // 有旧 manifest 的回滚：需要 restoreBackup
        // backup 可能为 null（事务在 prepareBackup 之前就回滚），此时只需 setManifestUri(oldRef.uri)
        if (backup == null) {
            // 事务在 prepareBackup 之前就回滚：old manifest 仍在 final，只需 setManifestUri
            if (!stateStore.setManifestUri(manifestOldRef.uri)) {
                DiagnosticsLogger.w(TAG, "rollback manifest: setManifestUri failed (backup null, old still in final)")
                return false
            }
            currentJournal = currentJournal.copy(
                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
            )
            if (!writePendingPublishJournal(
                    projectId = currentJournal.projectId,
                    transactionType = currentJournal.transactionType,
                    phase = PendingMirrorPublish.PHASE_ROLLBACK,
                    txId = currentJournal.txId,
                    backend = currentJournal.backend,
                    treeUri = currentJournal.treeUri,
                    oldEntries = currentJournal.oldEntries,
                    newEntries = currentJournal.newEntries,
                    stagedRefs = currentJournal.stagedRefs,
                    items = items,
                    removedProjectIds = currentJournal.removedProjectIds,
                    manifestOldRef = currentJournal.manifestOldRef,
                    manifestStagedRef = currentJournal.manifestStagedRef,
                    manifestNewRef = currentJournal.manifestNewRef,
                    manifestBackupRef = currentJournal.manifestBackupRef,
                    isManifestCommitted = currentJournal.isManifestCommitted,
                    manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                    manifestNewContentHash = currentJournal.manifestNewContentHash,
                    manifestOldContentHash = currentJournal.manifestOldContentHash,
                    journalContext = currentJournal,
                )
            ) {
                DiagnosticsLogger.w(TAG, "rollback manifest: journal write failed after OLD_RESTORED (backup null)")
                return false
            }
            return true
        }
        // 2. 检查 final 是否已恢复（带身份校验，#649 评论 5566303837 问题 3）
        //    #649 评论 5573310799 问题 4：用 restoredFinalLookup 避免与"1."块的 finalLookup 冲突。
        //    走到这里时 final 要么 Missing 要么已被"1."块删成 Missing，Found 分支作为幂等兜底保留。
        val manifestOldHash = journalContext.manifestOldContentHash
        val restoredFinalLookup = storage.lookup(manifestRelativePath)
        when (restoredFinalLookup) {
            is MirrorLookupResult.Found -> {
                // final 已存在，需要校验是否真的是旧 manifest
                if (manifestOldHash != null) {
                    val hashResult = storage.readTextAndHash(restoredFinalLookup.ref)
                    if (hashResult != null) {
                        val (_, hash) = hashResult
                        if (hash == manifestOldHash) {
                            // hash 匹配 → 真的是旧 manifest，必须先 setManifestUri 再推进状态
                            DiagnosticsLogger.i(TAG, "rollback manifest: final already restored, verified by hash")
                            if (!stateStore.setManifestUri(restoredFinalLookup.ref.uri)) {
                                DiagnosticsLogger.w(TAG, "rollback manifest: setManifestUri failed (already restored)")
                                return false
                            }
                            // 写 MANIFEST_ROLLBACK_OLD_RESTORED
                            currentJournal = currentJournal.copy(
                                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                            )
                            if (!writePendingPublishJournal(
                                    projectId = currentJournal.projectId,
                                    transactionType = currentJournal.transactionType,
                                    phase = PendingMirrorPublish.PHASE_ROLLBACK,
                                    txId = currentJournal.txId,
                                    backend = currentJournal.backend,
                                    treeUri = currentJournal.treeUri,
                                    oldEntries = currentJournal.oldEntries,
                                    newEntries = currentJournal.newEntries,
                                    stagedRefs = currentJournal.stagedRefs,
                                    items = items,
                                    removedProjectIds = currentJournal.removedProjectIds,
                                    manifestOldRef = currentJournal.manifestOldRef,
                                    manifestStagedRef = currentJournal.manifestStagedRef,
                                    manifestNewRef = currentJournal.manifestNewRef,
                                    manifestBackupRef = currentJournal.manifestBackupRef,
                                    isManifestCommitted = currentJournal.isManifestCommitted,
                                    manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                                    manifestNewContentHash = currentJournal.manifestNewContentHash,
                                    manifestOldContentHash = currentJournal.manifestOldContentHash,
                                    journalContext = currentJournal,
                                )
                            ) {
                                DiagnosticsLogger.w(TAG, "rollback manifest: journal write failed after OLD_RESTORED")
                                return false
                            }
                            return true
                        }
                        // hash 不匹配 → final 上是新 manifest 残留，继续 restore
                    }
                    // 读取失败 → 无法确认状态，继续 restore
                } else {
                    // 无 hash 校验 → 文件已存在视为已恢复，但还是要 setManifestUri
                    DiagnosticsLogger.i(TAG, "rollback manifest: final already exists, skipping restore")
                    if (!stateStore.setManifestUri(restoredFinalLookup.ref.uri)) {
                        DiagnosticsLogger.w(TAG, "rollback manifest: setManifestUri failed (already exists)")
                        return false
                    }
                    // 写 MANIFEST_ROLLBACK_OLD_RESTORED
                    currentJournal = currentJournal.copy(
                        manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                    )
                    if (!writePendingPublishJournal(
                            projectId = currentJournal.projectId,
                            transactionType = currentJournal.transactionType,
                            phase = PendingMirrorPublish.PHASE_ROLLBACK,
                            txId = currentJournal.txId,
                            backend = currentJournal.backend,
                            treeUri = currentJournal.treeUri,
                            oldEntries = currentJournal.oldEntries,
                            newEntries = currentJournal.newEntries,
                            stagedRefs = currentJournal.stagedRefs,
                            items = items,
                            removedProjectIds = currentJournal.removedProjectIds,
                            manifestOldRef = currentJournal.manifestOldRef,
                            manifestStagedRef = currentJournal.manifestStagedRef,
                            manifestNewRef = currentJournal.manifestNewRef,
                            manifestBackupRef = currentJournal.manifestBackupRef,
                            isManifestCommitted = currentJournal.isManifestCommitted,
                            manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                            manifestNewContentHash = currentJournal.manifestNewContentHash,
                            manifestOldContentHash = currentJournal.manifestOldContentHash,
                            journalContext = currentJournal,
                        )
                    ) {
                        DiagnosticsLogger.w(TAG, "rollback manifest: journal write failed after OLD_RESTORED")
                        return false
                    }
                    return true
                }
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(TAG, "rollback manifest: lookup final failed: ${restoredFinalLookup.cause?.message}")
                return false
            }
            is MirrorLookupResult.Missing -> {
                // final 不存在，继续恢复
            }
        }
        // 3. restoreBackup(old manifest) → 拿 restoredRef
        val restoreResult = storage.restoreBackup(backup, manifestRelativePath, MIME_JSON, manifestOldHash)
        when (restoreResult) {
            is RestoreBackupResult.Restored -> {
                // 新恢复成功，更新 manifestUri
                if (!stateStore.setManifestUri(restoreResult.ref.uri)) {
                DiagnosticsLogger.w(TAG, "rollback manifest: setManifestUri failed after restoring manifest backup")
                return false
            }
            // 写 MANIFEST_ROLLBACK_OLD_RESTORED
            currentJournal = currentJournal.copy(
                manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
            )
            if (!writePendingPublishJournal(
                    projectId = currentJournal.projectId,
                    transactionType = currentJournal.transactionType,
                    phase = PendingMirrorPublish.PHASE_ROLLBACK,
                    txId = currentJournal.txId,
                    backend = currentJournal.backend,
                    treeUri = currentJournal.treeUri,
                    oldEntries = currentJournal.oldEntries,
                    newEntries = currentJournal.newEntries,
                    stagedRefs = currentJournal.stagedRefs,
                    items = items,
                    removedProjectIds = currentJournal.removedProjectIds,
                    manifestOldRef = currentJournal.manifestOldRef,
                    manifestStagedRef = currentJournal.manifestStagedRef,
                    manifestNewRef = currentJournal.manifestNewRef,
                    manifestBackupRef = currentJournal.manifestBackupRef,
                    isManifestCommitted = currentJournal.isManifestCommitted,
                    manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                    manifestNewContentHash = currentJournal.manifestNewContentHash,
                    manifestOldContentHash = currentJournal.manifestOldContentHash,
                    journalContext = currentJournal,
                )
            ) {
                DiagnosticsLogger.w(TAG, "rollback manifest: journal write failed after OLD_RESTORED")
                return false
            }
            return true
        }
            is RestoreBackupResult.AlreadyRestored -> {
                // hash 校验通过，manifest 已恢复
                if (!stateStore.setManifestUri(restoreResult.ref.uri)) {
                    DiagnosticsLogger.w(TAG, "rollback manifest: setManifestUri failed (already restored)")
                    return false
                }
                // 写 MANIFEST_ROLLBACK_OLD_RESTORED
                currentJournal = currentJournal.copy(
                    manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                )
                if (!writePendingPublishJournal(
                        projectId = currentJournal.projectId,
                        transactionType = currentJournal.transactionType,
                        phase = PendingMirrorPublish.PHASE_ROLLBACK,
                        txId = currentJournal.txId,
                        backend = currentJournal.backend,
                        treeUri = currentJournal.treeUri,
                        oldEntries = currentJournal.oldEntries,
                        newEntries = currentJournal.newEntries,
                        stagedRefs = currentJournal.stagedRefs,
                        items = items,
                        removedProjectIds = currentJournal.removedProjectIds,
                        manifestOldRef = currentJournal.manifestOldRef,
                        manifestStagedRef = currentJournal.manifestStagedRef,
                        manifestNewRef = currentJournal.manifestNewRef,
                        manifestBackupRef = currentJournal.manifestBackupRef,
                        isManifestCommitted = currentJournal.isManifestCommitted,
                        manifestSwapState = ManifestTransactionState.MANIFEST_ROLLBACK_OLD_RESTORED,
                        manifestNewContentHash = currentJournal.manifestNewContentHash,
                        manifestOldContentHash = currentJournal.manifestOldContentHash,
                        journalContext = currentJournal,
                    )
                ) {
                    DiagnosticsLogger.w(TAG, "rollback manifest: journal write failed after OLD_RESTORED")
                    return false
                }
                return true
            }
            is RestoreBackupResult.Conflict -> {
                DiagnosticsLogger.w(TAG, "rollback manifest: conflict - final has wrong content")
                return false
            }
            is RestoreBackupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "rollback manifest: failed to restore manifest backup: ${restoreResult.cause?.message}",
                )
                return false
            }
        }
    }

    /**
     * 恢复回滚阶段。
     *
     * #649 评论 5564624383 问题 2：rollback 本身做成 journal 状态。
     * 进程死在回滚中间，下次继续回滚剩余 item，不会又转回 forward promote。
     * 恢复策略：逐个恢复尚未恢复的 item（backupOldRef 仍非空），全部恢复后清 journal。
     */
    private suspend fun recoverRollbackPhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        // #649 评论 5564820566 问题 2：rollback 和 recovery 共用同一套显式状态机。
        // 不再用 "final 是否存在" 判断它是新正文还是旧正文。
        // 每个 item 用 STATE_ROLLBACK_NEW_REMOVED / STATE_ROLLBACK_OLD_RESTORED 跟踪进度。
        val currentItems = journal.items.toMutableMap()
        var allSuccess = true

        // 步骤 1：删除 promotedRef（幂等，跳过已处理的 item）
        // #649 评论 5565067997 修复 4：检查 delete() 返回值，失败时停止推进状态
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue
            if (item.state != PendingItem.STATE_ROLLBACK_NEW_REMOVED) {
                // #649 评论 5565067997 修复 4：检查 delete() 返回值
                val removed = item.promotedRef?.let { storage.delete(it) } ?: true
                if (!removed) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Recover rollback: delete promotedRef failed for ${key.chapterId}, keeping journal",
                    )
                    return
                }
                currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_NEW_REMOVED)
            }
            // 更新 journal
            if (!writePendingPublishJournal(
                    projectId = journal.projectId,
                    transactionType = journal.transactionType,
                    phase = PendingMirrorPublish.PHASE_ROLLBACK,
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
                )
            ) {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover rollback: journal write failed after NEW_REMOVED for ${key.chapterId}",
                )
                return
            }
        }

        // 步骤 2：恢复 backupOldRef 到最终路径
        //    #649 评论 5570613481 问题 1：使用统一 helper，不再内联回滚逻辑。
        //    #649 评论 5573310799 问题 1：每章恢复成功后先写 stateStore 再标记 journal，
        //    消除断电丢失窗口（journal 已写 OLD_RESTORED、stateStore 还没写真实 URI）。
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_ROLLBACK_OLD_RESTORED) continue

            val rollbackResult = rollbackChapterToOldState(journal, key, item, storage)
            when (rollbackResult) {
                is RollbackItemResult.Restored -> {
                    // #649 评论 5573310799 问题 1：先把真实 URI 写进 stateStore，再标记 journal。
                    //    putChapterEntry 失败时 return（state 没写成功不能继续）。
                    val oldEntry = journal.oldEntries[key] ?: run {
                        DiagnosticsLogger.w(TAG, "Recover rollback: missing oldEntry for ${key.chapterId}, keeping journal")
                        return
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
                        DiagnosticsLogger.w(TAG, "Recover rollback: putChapterEntry failed for ${key.chapterId}, keeping journal")
                        return
                    }
                    currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_OLD_RESTORED)
                }
                RollbackItemResult.NewFileRemoved -> {
                    currentItems[key] = item.copy(state = PendingItem.STATE_ROLLBACK_OLD_RESTORED)
                }
                RollbackItemResult.StateUnknown -> {
                    DiagnosticsLogger.w(TAG, "Recover rollback: state unknown for ${key.chapterId}, keeping journal")
                    return
                }
                RollbackItemResult.Failed -> {
                    DiagnosticsLogger.w(TAG, "Recover rollback: failed for ${key.chapterId}, keeping journal")
                    allSuccess = false
                    continue
                }
            }
            // 更新 journal
            if (!writePendingPublishJournal(
                    projectId = journal.projectId,
                    transactionType = journal.transactionType,
                    phase = PendingMirrorPublish.PHASE_ROLLBACK,
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
                )
            ) {
                DiagnosticsLogger.w(
                    TAG,
                    "Recover rollback: journal write failed after OLD_RESTORED for ${key.chapterId}",
                )
                return
            }
        }

        if (!allSuccess) {
            DiagnosticsLogger.w(TAG, "Recover rollback: partial failure, keeping journal for retry")
            return
        }

        // 步骤 3：恢复 manifest
        //    #649 评论 5565067997 修复 3：共用 rollbackManifest helper，正确顺序。
        if (!rollbackManifest(journal, storage, currentItems)) {
            DiagnosticsLogger.w(TAG, "Recover rollback: manifest rollback failed, keeping journal")
            return
        }
        // #649 评论 5566303837 问题 6：检查 rollback 返回值
        if (storage.rollback(journal.txId)) {
            stateStore.clearPendingPublish()
        } else {
            DiagnosticsLogger.w(TAG, "Recover rollback: staging cleanup failed, keeping journal for retry")
        }
    }

    // ── 事务性发布内部 ──

    /**
     * 写入计划单条。
     *
     * @property key 章节定位。
     * @property chapter 章节元数据。
     * @property relativePath 目标相对路径。
     * @property content 预读的正文。
     * @property oldEntry 旧条目（null 表示新建）。
     */
    private data class WritePlanEntry(
        val key: ChapterKey,
        val chapter: ChapterMeta,
        val relativePath: String,
        val content: String,
        val oldEntry: ChapterMirrorEntry?,
    )

    /**
     * 准备阶段：读完整快照和所有章节正文到内存，构建写入计划。
     *
     * @return 写入计划列表；任一章节读取失败返回 null。
     */
    private suspend fun buildWritePlan(
        projectId: String,
        snapshot: ProjectWorkspaceSnapshot,
        oldEntries: Map<ChapterKey, ChapterMirrorEntry>,
        usedRelativePaths: MutableSet<String>,
    ): List<WritePlanEntry>? {
        val plan = mutableListOf<WritePlanEntry>()
        for (volumeWithChapters in snapshot.volumes) {
            for (chapter in volumeWithChapters.chapters) {
                val openResult = source.openChapter(projectId, volumeWithChapters.volume.id, chapter.id)
                if (openResult !is BridgeResult.Success) {
                    DiagnosticsLogger.w(TAG, "Failed to open chapter ${chapter.id} for plan")
                    return null
                }
                val content = openResult.data.content
                val key = ChapterKey(projectId, volumeWithChapters.volume.id, chapter.id)
                val relativePath =
                    resolveChapterRelativePath(
                        snapshot.project.title,
                        volumeWithChapters.volume.title,
                        chapter,
                        oldEntries,
                        key,
                        usedRelativePaths,
                    )
                usedRelativePaths.add(relativePath)
                plan.add(
                    WritePlanEntry(
                        key = key,
                        chapter = chapter,
                        relativePath = relativePath,
                        content = content,
                        oldEntry = oldEntries[key],
                    ),
                )
            }
        }
        return plan
    }

    /**
     * 写 pendingPublish journal。
     *
     * #649 评论 5563333323 缺口 2：返回 Boolean，失败时调用方停止本轮镜像操作。
     *
     * journal JSON 结构：
     * ```json
     * {
     *   "txId": "<txId>",
     *   "backend": "media_store" | "document_tree",
     *   "treeUri": "<treeUri>",
     *   "projectId": "<id>",
     *   "transactionType": "upsert_project" | "delete_project",
     *   "phase": "stage" | "promote" | "cleanup",
     *   "oldEntries": {...},
     *   "newEntries": {...},
     *   "stagedRefs": {...},
     *   "items": {...},
     *   "removedProjectIds": [...],
     *   "manifestOldRef": {...},
     *   "manifestStagedRef": {...},
     *   "manifestNewRef": {...},
     *   "manifestBackupRef": {...}
     * }
     * ```
     *
     * @param journalContext 当前事务 journal 上下文（#649 评论 5569598106 问题3）。
     *   当 `manifestNewContentHash`/`manifestOldContentHash` 参数为 null 时，自动从
     *   `journalContext` 继承对应 hash，避免调用点遗漏传递导致 journal 更新后 hash 丢失。
     *   传 null 表示无上下文可继承（如事务刚开始、manifest 尚未生成）。
     * @return true 表示持久化成功；false 表示失败（调用方应停止本轮操作，不继续移动文件）。
     */
    private fun writePendingPublishJournal(
        projectId: String,
        transactionType: MirrorTransactionType,
        phase: String,
        txId: String,
        backend: MirrorBackend,
        treeUri: String?,
        oldEntries: Map<ChapterKey, ChapterMirrorEntry>,
        newEntries: Map<ChapterKey, ChapterMirrorEntry>,
        stagedRefs: Map<ChapterKey, StagedMirrorRef>,
        items: Map<ChapterKey, PendingItem>,
        removedProjectIds: Set<String>,
        manifestOldRef: MirrorFileRef?,
        manifestStagedRef: StagedMirrorRef?,
        manifestNewRef: MirrorFileRef?,
        manifestBackupRef: MirrorFileRef?,
        isManifestCommitted: Boolean = false,
        manifestSwapState: ManifestTransactionState = ManifestTransactionState.MANIFEST_STAGED,
        manifestNewContentHash: String? = null,
        manifestOldContentHash: String? = null,
        // #649 评论 5572554935 额外修复：manifestTargetJson 字段继承。
        // 旧 builder 重新构造 PendingMirrorPublish 时没把 manifestTargetJson 从 journalContext 继承，
        // 导致 rollback 阶段每次 writePendingPublishJournal 都把冻结的 manifest 目标 JSON 静默清空。
        // 显式传参或从 journalContext 继承，避免字段新增后被旧 builder 静默清空。
        manifestTargetJson: String? = null,
        journalContext: PendingMirrorPublish? = null,
    ): Boolean {
        // #649 评论 5569598106 问题3：自动从 journalContext 继承 manifest hash，
        // 避免调用点遗漏传递导致 journal 更新后 hash 丢失为 null。
        val effectiveManifestNewContentHash = manifestNewContentHash ?: journalContext?.manifestNewContentHash
        val effectiveManifestOldContentHash = manifestOldContentHash ?: journalContext?.manifestOldContentHash
        // #649 评论 5572554935 额外修复：manifestTargetJson 同样从 journalContext 继承
        val effectiveManifestTargetJson = manifestTargetJson ?: journalContext?.manifestTargetJson
        val journal =
            PendingMirrorPublish(
                txId = txId,
                backend = backend,
                treeUri = treeUri,
                projectId = projectId,
                transactionType = transactionType,
                phase = phase,
                oldEntries = oldEntries,
                newEntries = newEntries,
                stagedRefs = stagedRefs,
                items = items,
                removedProjectIds = removedProjectIds,
                manifestOldRef = manifestOldRef,
                manifestStagedRef = manifestStagedRef,
                manifestNewRef = manifestNewRef,
                manifestBackupRef = manifestBackupRef,
                isManifestCommitted = isManifestCommitted,
                manifestSwapState = manifestSwapState,
                manifestNewContentHash = effectiveManifestNewContentHash,
                manifestOldContentHash = effectiveManifestOldContentHash,
                manifestTargetJson = effectiveManifestTargetJson,
            )
        return stateStore.writePendingPublish(journal.toJson())
    }

    /**
     * 前进式持久化 journal：把当前 journal 状态写入磁盘。
     *
     * #649 评论 5570613481 问题 3：manifest 事务中用当前 currentJournal 持久化，
     * 确保 hash 和状态不会在后续步骤中丢失。
     *
     * @param journal 当前 journal 状态
     * @return true 表示持久化成功；false 表示失败
     */
    private fun persistPendingJournal(journal: PendingMirrorPublish): Boolean =
        stateStore.writePendingPublish(journal.toJson())

    // ── 内部 ──

    /**
     * 计算章节正文的相对路径（相对 `Download/Sujian/`）。
     *
     * 同目录重名处理：若 [usedRelativePaths] 或 [oldEntries] 中已有相同 relativePath
     * 但属于不同 chapterId，给文件名追加 `_<chapterId 前 8 字符>`。
     */
    private fun resolveChapterRelativePath(
        projectTitle: String,
        volumeTitle: String,
        chapter: ChapterMeta,
        oldEntries: Map<ChapterKey, ChapterMirrorEntry>,
        chapterKey: ChapterKey,
        usedRelativePaths: MutableSet<String> = mutableSetOf(),
    ): String {
        val dir = chapterRelativeDir(projectTitle, volumeTitle)
        val baseName = chapterFileName(chapter.title).removeSuffix(".md")
        var fileName = "$baseName.md"
        var relativePath = "$dir/$fileName"
        // 检查 usedRelativePaths 和 oldEntries 是否已被不同 chapter 占用
        val occupiedPaths = mutableSetOf<String>()
        occupiedPaths.addAll(usedRelativePaths)
        for ((key, entry) in oldEntries) {
            if (key.volumeId == chapterKey.volumeId && key.chapterId != chapterKey.chapterId) {
                occupiedPaths.add(entry.relativePath)
            }
        }
        if (relativePath in occupiedPaths) {
            val shortId = chapterKey.chapterId.take(MIN_ID_LENGTH)
            fileName = "${baseName}_$shortId.md"
            relativePath = "$dir/$fileName"
        }
        return relativePath
    }

    /**
     * 构造"目标项目用 desiredEntries、其他项目从 stateStore 取"的 manifest JSON。
     *
     * @param snapshot 目标项目快照；null 表示该项目已不存在（从 manifest 省略），
     *   用于 deleteProject 场景。
     * @return manifest JSON；listProjects 失败返回 null。
     */
    private suspend fun buildManifestJsonForDesired(
        projectId: String,
        snapshot: ProjectWorkspaceSnapshot?,
        desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
    ): String? {
        val projectsResult = source.listProjects()
        if (projectsResult !is BridgeResult.Success) return null
        val projects = projectsResult.data

        val mirrorProjects = mutableListOf<MirrorProject>()
        for (project in projects) {
            if (project.id == projectId) {
                // 目标项目：用 desiredEntries 构造；snapshot=null 时省略（项目已删）
                if (snapshot != null) {
                    mirrorProjects.add(snapshot.toMirrorProject(desiredEntries))
                }
            } else {
                // 其他项目：从 stateStore 取旧 entries
                val snapshotResult = source.getProjectWorkspaceSnapshot(project.id)
                if (snapshotResult is BridgeResult.Success) {
                    val s = snapshotResult.data
                    val entries = stateStore.getProjectEntries(project.id)
                    mirrorProjects.add(s.toMirrorProject(entries))
                }
            }
        }

        val now = Instant.now()
        val updatedAt = DateTimeFormatter.ISO_INSTANT.format(now)
        val manifest =
            MirrorManifest(
                schemaVersion = 1,
                revision = now.toEpochMilli(),
                updatedAt = updatedAt,
                projects = mirrorProjects,
            )
        return manifestToJson(manifest)
    }

    /**
     * manifest 事务性写入的结果。
     *
     * #649 评论 5562462046 问题 2：manifest 走和正文同一套事务。
     *
     * @property newRef 新 manifest 引用（已 [ReadableMirrorStateStore.setManifestUri]）。
     * @property manifestOldRef 旧 manifest 引用（promote 前）。
     * @property manifestStagedRef manifest staging 引用。
     * @property backupOldRef 旧 manifest 备份引用（= manifestOldRef，新 manifest 提交成功后由调用方删）。
     * @property manifestNewContentHash 新 manifest 的内容 hash（#649 评论 5569598106 问题3），
     *   供调用方写 cleanup journal 时持续传递，避免 hash 丢失。
     * @property manifestOldContentHash 旧 manifest 的内容 hash（#649 评论 5569598106 问题3）。
     */
    private data class ManifestTransactionResult(
        val newRef: MirrorFileRef,
        val manifestOldRef: MirrorFileRef?,
        val manifestStagedRef: StagedMirrorRef,
        val backupOldRef: MirrorFileRef?,
        val manifestNewContentHash: String? = null,
        val manifestOldContentHash: String? = null,
    )

    /**
     * 事务性写入 manifest（stage → promote → setManifestUri），每步落 journal。
     *
     * #649 评论 5562462046 问题 2：旧 [writeManifestFile] 直接 replaceText 覆盖，
     * SAF openOutputStream 出错时旧 manifest 可能被截断。新实现走和正文同一套事务：
     * 1. stage manifest（用 storage.stageText）
     * 2. promote 新 manifest（promoteStaged 不删旧 manifest，旧 manifest 由调用方在事务提交后删）
     * 3. setManifestUri(newRef)
     *
     * #649 评论 5562715833 问题 5：每个不可逆步骤后写 journal，
     * 记录 manifestStagedRef / manifestNewRef / manifestBackupRef / isManifestCommitted。
     * isManifestCommitted 在 UPSERT 和 DELETE_PROJECT 两种事务中都使用，
     * recovery 据此判断是否需要重做 manifest。
     *
     * #649 评论 5571899956 问题 1：冻结 manifest 目标。
     * 恢复时（manifestTargetJson != null）直接使用 journal 中保存的 manifest JSON，
     * 不再重新 buildManifestJsonForDesired() + Instant.now()，确保恢复时生成的 JSON/hash
     * 与原事务完全一致。严格按当前 manifestSwapState 前进，不回退到 STAGED。
     *
     * @param storage 当前事务的 storage（由调用方传入，避免 router.currentResult() 在事务中途变化）
     * @param journalContext 当前 journal 上下文（包含写 journal 所需的全部字段）
     * @param items 当前最新 items（可能比 journalContext.items 更新）
     * @return [ManifestTransactionResult]；任何步骤失败返回 null（不修改 stateStore 的 manifestUri）
     */
    private suspend fun publishManifestWithDesiredTransactional(
        projectId: String,
        snapshot: ProjectWorkspaceSnapshot?,
        desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        txId: String,
        journalContext: PendingMirrorPublish,
        items: Map<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage,
    ): ManifestTransactionResult? {
        val manifestRelativePath = "$META_DIR/$MANIFEST_FILE_NAME"
        // 读取旧 manifest hash（备份前记录，用于恢复时校验身份）
        // #649 评论 5570613481 问题 3：恢复已有 manifest 事务时沿用已有 manifestOldContentHash；
        // #649 评论 5572554935 问题 3：首次 manifest old 身份未冻结。
        // 一旦 manifestTargetJson != null，说明 manifest 事务已经开始；
        // 恢复时只能用 journalContext.manifestOldRef / manifestOldContentHash，
        // 绝对不能再从当前 stateStore.getManifestUri() 反推 old。
        // manifestOldRef == null 在这个阶段明确表示"原事务没有旧 manifest"。
        val isResumingManifest = journalContext.manifestTargetJson != null
        val frozenOldRef: MirrorFileRef? =
            if (isResumingManifest) {
                journalContext.manifestOldRef
            } else {
                // 首次进入 manifest 事务：从 stateStore 读当前 old 身份，准备冻结进 journal
                val initialOldUri = stateStore.getManifestUri()
                initialOldUri?.let { MirrorFileRef(uri = it, relativePath = manifestRelativePath) }
            }
        val manifestOldContentHash =
            journalContext.manifestOldContentHash ?: run {
                frozenOldRef?.let { storage.readTextAndHash(it)?.second }
            }

        // 旧 manifest 存在但读取失败：在移动旧 manifest 之前就停止事务
        if (frozenOldRef != null && manifestOldContentHash == null) {
            DiagnosticsLogger.w(
                TAG,
                "Manifest transaction: old manifest exists but readTextAndHash failed, stopping before vacate",
            )
            return null
        }
        // 恢复路径下 oldRef 必须用 journalContext.manifestOldRef，不能从 stateStore 反推
        val oldRef = frozenOldRef

        // #649 评论 5571899956 问题 1：冻结 manifest 目标
        // 恢复时（manifestTargetJson != null）直接使用保存的 JSON，不再重新生成
        val json: String
        val manifestNewContentHash: String
        val staged: StagedMirrorRef?
        val resumeState: ManifestTransactionState
        // currentJournal 在首次进入时赋值为 journalContext.copy(...)，恢复时直接用 journalContext
        var currentJournal: PendingMirrorPublish = journalContext

        if (journalContext.manifestTargetJson != null) {
            // 恢复路径：使用冻结的 manifest JSON，跳过重新 stage
            json = journalContext.manifestTargetJson
            manifestNewContentHash = computeContentHash(json)
            // 恢复时 staged 可能已存在（MANIFEST_STAGED/MANIFEST_BACKUP_READY/MANIFEST_OLD_VACATED），
            // 也可能已 promote（MANIFEST_PROMOTED/MANIFEST_COMMITTED）
            staged = journalContext.manifestStagedRef
            resumeState = journalContext.manifestSwapState
            // 恢复路径：使用 journalContext 作为当前 journal
            currentJournal = journalContext
        } else {
            // 首次进入 manifest 事务：构建并 stage，写 journal 保存 manifestTargetJson
            val builtJson = buildManifestJsonForDesired(projectId, snapshot, desiredEntries) ?: return null
            json = builtJson
            manifestNewContentHash = computeContentHash(json)
            val newStaged =
                storage.stageText(
                    txId = txId,
                    relativePath = manifestRelativePath,
                    mimeType = MIME_JSON,
                    text = json,
                ) ?: return null
            staged = newStaged
            // 写 journal：保存 manifestTargetJson，状态为 STAGED
            // #649 评论 5572554935 问题 3：首次事务必须把 old 身份冻结进 journal。
            // 一旦 manifestTargetJson != null，说明 manifest 事务已经开始；
            // 恢复时只能用 journalContext.manifestOldRef，不能再从 stateStore.getManifestUri() 反推。
            // manifestOldRef == null 在这个阶段明确表示"原事务没有旧 manifest"。
            currentJournal =
                journalContext.copy(
                    manifestOldRef = oldRef,
                    manifestStagedRef = staged,
                    manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                    manifestNewContentHash = manifestNewContentHash,
                    manifestOldContentHash = manifestOldContentHash,
                    manifestTargetJson = json,
                )
            if (!persistPendingJournal(currentJournal)) {
                storage.delete(
                    MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath),
                )
                return null
            }
            resumeState = ManifestTransactionState.MANIFEST_STAGED
        }

        // #649 评论 5565067997 修复 2：根据 journalContext.manifestSwapState 决定从哪一步继续。
        // 不再用 "backup + final 同时存在" 猜测，而是看 journal 记录的显式状态。
        // 恢复路径严格按当前状态前进，不回退到 STAGED。
        var manifestBackupRef: MirrorFileRef? = null
        // #649 评论 5573310799 问题 3：lookupBackup 完整三态 when，不把 Missing 和 Failed 揉在一起。
        //    SAF 权限异常、MediaStore query 失败时不再重新 prepareBackup（状态不明还继续改文件）。
        when (val backupResult = storage.lookupBackup(txId, manifestRelativePath)) {
            is MirrorLookupResult.Found -> {
                manifestBackupRef = backupResult.ref
            // backup 已存在。根据 journal 的 manifestSwapState 决定下一步：
            // - MANIFEST_BACKUP_READY：backup 就绪，old 可能还没 vacate → 需 vacate
            // - MANIFEST_OLD_VACATED：old 已腾空 → 需 promote
            // - MANIFEST_PROMOTED：已 promote → 需 setManifestUri
            // - MANIFEST_COMMITTED：已提交 → 直接返回
            when (resumeState) {
                ManifestTransactionState.MANIFEST_COMMITTED -> {
                    // 已提交，final 就是 new manifest。用 lookup() 三态查询。
                    val existingFinal = storage.lookup(manifestRelativePath)
                    when (existingFinal) {
                        is MirrorLookupResult.Found -> {
                            return ManifestTransactionResult(
                                newRef = existingFinal.ref,
                                manifestOldRef = oldRef,
                                manifestStagedRef = staged ?: journalContext.manifestStagedRef!!,
                                backupOldRef = manifestBackupRef,
                                manifestNewContentHash = manifestNewContentHash,
                                manifestOldContentHash = manifestOldContentHash,
                            )
                        }
                        is MirrorLookupResult.Missing -> {
                            DiagnosticsLogger.w(TAG, "Manifest transaction: COMMITTED but final missing, rolling back")
                            if (staged != null) {
                                storage.delete(
                                    MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath),
                                )
                            }
                            return null
                        }
                        is MirrorLookupResult.Failed -> {
                            DiagnosticsLogger.w(
                                TAG,
                                "Manifest transaction: " +
                                    "lookup final failed (COMMITTED): ${existingFinal.cause?.message}",
                            )
                            if (staged != null) {
                                storage.delete(
                                    MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath),
                                )
                            }
                            return null
                        }
                    }
                }
                ManifestTransactionState.MANIFEST_PROMOTED -> {
                    // 已 promote，继续 setManifestUri。用 lookup() 三态查询。
                    val existingFinal = storage.lookup(manifestRelativePath)
                    when (existingFinal) {
                        is MirrorLookupResult.Found -> {
                            if (!stateStore.setManifestUri(existingFinal.ref.uri)) {
                                DiagnosticsLogger.w(
                                    TAG,
                                    "Manifest transaction: setManifestUri failed (resume PROMOTED)",
                                )
                                return null
                            }
                            currentJournal =
                                currentJournal.copy(
                                    manifestNewRef = existingFinal.ref,
                                    manifestBackupRef = manifestBackupRef,
                                    isManifestCommitted = true,
                                    manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                                )
                            if (!persistPendingJournal(currentJournal)) {
                                return null
                            }
                            return ManifestTransactionResult(
                                newRef = existingFinal.ref,
                                manifestOldRef = oldRef,
                                manifestStagedRef = staged ?: journalContext.manifestStagedRef!!,
                                backupOldRef = manifestBackupRef,
                                manifestNewContentHash = manifestNewContentHash,
                                manifestOldContentHash = manifestOldContentHash,
                            )
                        }
                        is MirrorLookupResult.Missing -> {
                            DiagnosticsLogger.w(TAG, "Manifest transaction: PROMOTED but final missing")
                            if (staged != null) {
                                storage.delete(
                                    MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath),
                                )
                            }
                            return null
                        }
                        is MirrorLookupResult.Failed -> {
                            DiagnosticsLogger.w(
                                TAG,
                                "Manifest transaction: lookup final failed (PROMOTED): ${existingFinal.cause?.message}",
                            )
                            if (staged != null) {
                                storage.delete(
                                    MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath),
                                )
                            }
                            return null
                        }
                    }
                }
                ManifestTransactionState.MANIFEST_OLD_VACATED -> {
                    // old 已腾空，继续 promote
                    currentJournal =
                        currentJournal.copy(
                            manifestBackupRef = manifestBackupRef,
                            manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED,
                        )
                    if (!persistPendingJournal(currentJournal)) {
                        if (staged != null) {
                            storage.delete(
                                MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath),
                            )
                        }
                        return null
                    }
                    // 跳到 promote 步骤
                }
                else -> {
                    // MANIFEST_STAGED 或 MANIFEST_BACKUP_READY：backup 就绪，需 vacate old
                    // #649 评论 5565067997 修复 5：用 lookup() 三态查询判断 old 是否已 vacate
                    if (oldRef != null) {
                        val oldLookup = storage.lookup(oldRef.relativePath)
                        when (oldLookup) {
                            is MirrorLookupResult.Found -> {
                                // old 还在 final，需 vacate
                                if (!storage.vacateCommitted(oldRef)) {
                                    DiagnosticsLogger.w(
                                        TAG,
                                        "Manifest transaction: vacate failed (resume BACKUP_READY)",
                                    )
                                    if (staged != null) {
                                        storage.delete(
                                            MirrorFileRef(
                                                uri = staged.stagingUri,
                                                relativePath = staged.stagingRelativePath,
                                            ),
                                        )
                                    }
                                    return null
                                }
                            }
                            is MirrorLookupResult.Missing -> {
                                // old 已腾空，无需再 vacate
                            }
                            is MirrorLookupResult.Failed -> {
                                // 查询失败，不能继续
                                DiagnosticsLogger.w(
                                    TAG,
                                    "Manifest transaction: " +
                                        "lookup old failed (resume BACKUP_READY): ${oldLookup.cause?.message}",
                                )
                                if (staged != null) {
                                    storage.delete(
                                        MirrorFileRef(
                                            uri = staged.stagingUri,
                                            relativePath = staged.stagingRelativePath,
                                        ),
                                    )
                                }
                                return null
                            }
                        }
                    }
                    // 写 journal：MANIFEST_OLD_VACATED
                    currentJournal =
                        currentJournal.copy(
                            manifestBackupRef = manifestBackupRef,
                            manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED,
                        )
                    if (!persistPendingJournal(currentJournal)) {
                        if (staged != null) {
                            storage.delete(
                                MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath),
                            )
                        }
                        return null
                    }
                }
            }
            }
            is MirrorLookupResult.Missing -> {
                if (oldRef != null) {
                    // #649 评论 5564820566 问题 3：manifest 也用两步 prepareBackup + vacateCommitted
                    val prepared = storage.prepareBackup(txId, oldRef, MIME_JSON)
                    if (prepared == null) {
                        // backup 失败：删 manifest staging，不动旧 manifest
                        if (staged != null) {
                            storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
                        }
                        return null
                    }
                    manifestBackupRef = prepared.backupRef
                    // 写 journal：MANIFEST_BACKUP_READY
                    currentJournal =
                        currentJournal.copy(
                            manifestBackupRef = manifestBackupRef,
                            manifestSwapState = ManifestTransactionState.MANIFEST_BACKUP_READY,
                        )
                    if (!persistPendingJournal(currentJournal)) {
                        if (staged != null) {
                            storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
                        }
                        return null
                    }
                    // vacate old（如果 prepareBackup 还没 move old）
                    if (!prepared.vacated) {
                        if (!storage.vacateCommitted(oldRef)) {
                            // vacate 失败：删 staging 和 backup
                            if (staged != null) {
                                storage.delete(
                                    MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath),
                                )
                            }
                            return null
                        }
                    }
                    // 写 journal：MANIFEST_OLD_VACATED
                    currentJournal =
                        currentJournal.copy(
                            manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED,
                        )
                    if (!persistPendingJournal(currentJournal)) {
                        // journal 失败：恢复 backup 到最终位置，删 staging
                        storage.restoreBackup(
                            prepared.backupRef,
                            manifestRelativePath,
                            MIME_JSON,
                            currentJournal.manifestOldContentHash,
                        )
                        if (staged != null) {
                            storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
                        }
                        return null
                    }
                }
                // oldRef == null 时什么都不做（首次发布无旧 manifest），直接进入后面的 promote
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(TAG, "manifest backup lookup failed: ${backupResult.cause?.message}")
                // 删 staging
                if (staged != null) {
                    storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
                }
                return null
            }
        }
        // 3. promote 新 manifest（promoteStaged 不删旧 manifest，旧 manifest 已由 backupCommitted 移走）
        //    只有 MANIFEST_OLD_VACATED 状态才允许 promote
        //    #649 评论 5566303837 问题 3 + 5570613481 问题 2：promote 前用完整 when 校验 final
        val desiredNewHash = currentJournal.manifestNewContentHash
        val finalLookup = storage.lookup(manifestRelativePath)
        var newRef: MirrorFileRef? = null
        when (finalLookup) {
            is MirrorLookupResult.Found -> {
                // final 已存在，需要校验身份
                if (desiredNewHash == null) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: final exists but no expected hash, keeping journal")
                    return null
                }
                val hashResult = storage.readTextAndHash(finalLookup.ref)
                if (hashResult == null) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Manifest transaction: readTextAndHash failed, cannot verify final identity, keeping journal",
                    )
                    return null
                }
                val (_, hash) = hashResult
                if (hash == desiredNewHash) {
                    // final 已是新 manifest → promote 已完成，只需 setManifestUri
                    if (!stateStore.setManifestUri(finalLookup.ref.uri)) {
                        DiagnosticsLogger.w(
                            TAG,
                            "Manifest transaction: setManifestUri failed (final already new manifest)",
                        )
                        return null
                    }
                    currentJournal =
                        currentJournal.copy(
                            manifestNewRef = finalLookup.ref,
                            isManifestCommitted = true,
                            manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
                        )
                    if (!persistPendingJournal(currentJournal)) {
                        return null
                    }
                    return ManifestTransactionResult(
                        newRef = finalLookup.ref,
                        manifestOldRef = oldRef,
                        manifestStagedRef = staged ?: journalContext.manifestStagedRef!!,
                        backupOldRef = manifestBackupRef,
                        manifestNewContentHash = manifestNewContentHash,
                        manifestOldContentHash = manifestOldContentHash,
                    )
                } else {
                    DiagnosticsLogger.w(
                        TAG,
                        "Manifest transaction: final hash mismatch, state unknown, keeping journal",
                    )
                    return null
                }
            }
            is MirrorLookupResult.Missing -> {
                // final 不存在，正常 promote
                if (staged != null) {
                    newRef = storage.promoteStaged(staged, manifestRelativePath)
                }
            }
            is MirrorLookupResult.Failed -> {
                DiagnosticsLogger.w(
                    TAG,
                    "Manifest transaction: lookup final failed: ${finalLookup.cause?.message}, keeping journal",
                )
                return null
            }
        }
        if (newRef == null) {
            // promote 失败：恢复 backup（如果有），删 staging
            manifestBackupRef?.let {
                storage.restoreBackup(it, manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            }
            if (staged != null) {
                storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
            }
            return null
        }
        // 写 journal：MANIFEST_PROMOTED
        currentJournal =
            currentJournal.copy(
                manifestNewRef = newRef,
                manifestSwapState = ManifestTransactionState.MANIFEST_PROMOTED,
            )
        if (!persistPendingJournal(currentJournal)) {
            // journal 写失败：删新 manifest，恢复 backup（如果有）
            storage.delete(newRef)
            manifestBackupRef?.let {
                storage.restoreBackup(it, manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            }
            return null
        }
        // 4. setManifestUri
        //    #649 评论 5563333323 缺口 2：setManifestUri 返回 Boolean
        if (!stateStore.setManifestUri(newRef.uri)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed")
            storage.delete(newRef)
            manifestBackupRef?.let {
                storage.restoreBackup(it, manifestRelativePath, MIME_JSON, currentJournal.manifestOldContentHash)
            }
            return null
        }
        // 写 journal：MANIFEST_COMMITTED
        currentJournal =
            currentJournal.copy(
                isManifestCommitted = true,
                manifestSwapState = ManifestTransactionState.MANIFEST_COMMITTED,
            )
        if (!persistPendingJournal(currentJournal)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: MANIFEST_COMMITTED journal write failed")
        }
        return ManifestTransactionResult(
            newRef = newRef,
            manifestOldRef = oldRef,
            manifestStagedRef = staged ?: journalContext.manifestStagedRef!!,
            backupOldRef = manifestBackupRef,
            manifestNewContentHash = manifestNewContentHash,
            manifestOldContentHash = manifestOldContentHash,
        )
    }

    /**
     * 写 manifest 相关的 journal 字段（复用 [journalContext] 的非 manifest 字段）。
     *
     * #649 评论 5562715833 问题 5：manifest 事务中间状态逐步落 journal。
     * #649 评论 5563333323 缺口 2：返回 Boolean。
     */
    private fun writeManifestJournal(
        journalContext: PendingMirrorPublish,
        items: Map<ChapterKey, PendingItem>,
        manifestStagedRef: StagedMirrorRef?,
        manifestNewRef: MirrorFileRef?,
        manifestBackupRef: MirrorFileRef?,
        isManifestCommitted: Boolean,
        manifestSwapState: ManifestTransactionState,
        manifestNewContentHash: String? = null,
        manifestOldContentHash: String? = null,
    ): Boolean {
        return writePendingPublishJournal(
            projectId = journalContext.projectId,
            transactionType = journalContext.transactionType,
            phase = journalContext.phase,
            txId = journalContext.txId,
            backend = journalContext.backend,
            treeUri = journalContext.treeUri,
            oldEntries = journalContext.oldEntries,
            newEntries = journalContext.newEntries,
            stagedRefs = journalContext.stagedRefs,
            items = items,
            removedProjectIds = journalContext.removedProjectIds,
            manifestOldRef = journalContext.manifestOldRef,
            manifestStagedRef = manifestStagedRef,
            manifestNewRef = manifestNewRef,
            manifestBackupRef = manifestBackupRef,
            isManifestCommitted = isManifestCommitted,
            manifestSwapState = manifestSwapState,
            manifestNewContentHash = manifestNewContentHash,
            manifestOldContentHash = manifestOldContentHash,
            journalContext = journalContext,
        )
    }

    private fun logNotLoaded(
        result: BridgeResult<*>,
        op: String,
    ) {
        when (result) {
            is BridgeResult.Error -> DiagnosticsLogger.w(TAG, "$op failed: ${result.fullEnvelope}")
            BridgeResult.NotLoaded -> DiagnosticsLogger.w(TAG, "Native library not loaded, skip $op")
            else -> {}
        }
    }

    private fun manifestToJson(manifest: MirrorManifest): String {
        val sb = StringBuilder()
        sb.appendLine("{")
        sb.appendLine("  \"schemaVersion\": ${manifest.schemaVersion},")
        sb.appendLine("  \"revision\": ${manifest.revision},")
        sb.appendLine("  \"updatedAt\": \"${manifest.updatedAt}\",")
        sb.appendLine("  \"projects\": [")
        for ((i, project) in manifest.projects.withIndex()) {
            sb.append(projectToJson(project, "    "))
            if (i < manifest.projects.lastIndex) sb.appendLine(",") else sb.appendLine()
        }
        sb.appendLine("  ]")
        sb.append("}")
        return sb.toString()
    }

    private fun projectToJson(
        project: MirrorProject,
        indent: String,
    ): String {
        val sb = StringBuilder()
        sb.appendJsonOpen(indent)
        sb.appendLine("$indent  \"id\": \"${project.id}\",")
        sb.appendLine("$indent  \"title\": \"${escapeJson(project.title)}\",")
        sb.appendLine("$indent  \"order\": ${project.order},")
        sb.appendLine("$indent  \"revision\": ${project.revision},")
        sb.appendLine("$indent  \"updatedAt\": \"${project.updatedAt}\",")
        sb.appendLine("$indent  \"volumes\": [")
        for ((i, volume) in project.volumes.withIndex()) {
            sb.append(volumeToJson(volume, "$indent    "))
            if (i < project.volumes.lastIndex) sb.appendLine(",") else sb.appendLine()
        }
        sb.appendLine("$indent  ]")
        sb.appendJsonClose(indent)
        return sb.toString()
    }

    private fun volumeToJson(
        volume: MirrorVolume,
        indent: String,
    ): String {
        val sb = StringBuilder()
        sb.appendJsonOpen(indent)
        sb.appendLine("$indent  \"id\": \"${volume.id}\",")
        sb.appendLine("$indent  \"title\": \"${escapeJson(volume.title)}\",")
        sb.appendLine("$indent  \"order\": ${volume.order},")
        sb.appendLine("$indent  \"revision\": ${volume.revision},")
        sb.appendLine("$indent  \"updatedAt\": \"${volume.updatedAt}\",")
        sb.appendLine("$indent  \"chapters\": [")
        for ((i, chapter) in volume.chapters.withIndex()) {
            sb.append(chapterToJson(chapter, "$indent    "))
            if (i < volume.chapters.lastIndex) sb.appendLine(",") else sb.appendLine()
        }
        sb.appendLine("$indent  ]")
        sb.appendJsonClose(indent)
        return sb.toString()
    }

    private fun chapterToJson(
        chapter: MirrorChapter,
        indent: String,
    ): String {
        val sb = StringBuilder()
        sb.appendJsonOpen(indent)
        sb.appendLine("$indent  \"id\": \"${chapter.id}\",")
        sb.appendLine("$indent  \"title\": \"${escapeJson(chapter.title)}\",")
        sb.appendLine("$indent  \"order\": ${chapter.order},")
        sb.appendLine("$indent  \"revision\": ${chapter.revision},")
        sb.appendLine("$indent  \"updatedAt\": \"${chapter.updatedAt}\",")
        sb.appendLine("$indent  \"contentFile\": \"${escapeJson(chapter.contentFile)}\",")
        sb.appendLine("$indent  \"contentHash\": \"${chapter.contentHash}\"")
        sb.appendJsonClose(indent)
        return sb.toString()
    }

    private fun StringBuilder.appendJsonOpen(indent: String) = appendLine("$indent{")

    private fun StringBuilder.appendJsonClose(indent: String) = append("$indent}")

    private fun escapeJson(s: String): String =
        s.replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("\n", "\\n")
            .replace("\r", "\\r")
            .replace("\t", "\\t")

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
        private const val META_DIR = "_meta"
        private const val MANIFEST_FILE_NAME = "manifest.json"
        private const val MIME_MARKDOWN = "text/markdown"
        private const val MIME_JSON = "application/json"
        private const val MIN_ID_LENGTH = 8
        private const val SKIP_NOT_SUPPORTED = "Mirror publish skipped: storage not supported"
    }
}
