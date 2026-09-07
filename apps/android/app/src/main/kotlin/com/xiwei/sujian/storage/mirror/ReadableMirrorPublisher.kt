package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import com.xiwei.sujian.feature.project.data.model.VolumeWithChapters
import com.xiwei.sujian.storage.mirror.toMirrorProject
import com.xiwei.sujian.storage.mirror.toMirrorVolume
import org.json.JSONArray
import org.json.JSONObject
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
    // #649 评论 5564624383 问题 1：不要靠异常判断恢复结果。
    // recoverPendingPublishIfNeeded() 遇到 Corrupted / forBackend 失败 / storage 不支持
    // 只是 return，不抛异常，导致 ensurePendingRecovered 仍返回 true，
    // 下一笔 writePendingPublishJournal 会覆盖旧 journal。
    // 新逻辑：恢复前读一次、恢复后再读一次，只有 journal 不存在才返回 true。
    private suspend fun ensurePendingRecovered(): Boolean {
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
                DiagnosticsLogger.e(TAG, "Pending publish journal is corrupted, cannot start new transaction", pendingResult.error)
                return
            }
            is PendingPublishResult.Success -> {
                val journalJson = pendingResult.json
                val journal = PendingMirrorPublish.fromJson(journalJson) ?: return
                DiagnosticsLogger.i(TAG, "Recovering pending publish: phase=${journal.phase}, projectId=${journal.projectId}, txType=${journal.transactionType}")

                // #649 评论 5564379115 问题 5：用 forBackendResult() 替代已废弃的 forBackend()
                // 恢复时按 journal 记录的 backend/treeUri 构造当时那套 storage
                val storageResult = router.forBackendResult(journal.backend, journal.treeUri)
                if (storageResult.isFailure) {
                    // #649 评论 5564379115 问题 5：storage 不可用时保留 journal，不要 clearPendingPublish()
                    DiagnosticsLogger.w(TAG, "Storage not available during recovery: ${storageResult.exceptionOrNull()?.message}, keeping journal")
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
                        storage.rollback(journal.txId)
                        stateStore.clearPendingPublish()
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
            if (oldRef != null && item.backupOldRef == null) {
                // 检测崩溃窗口：backup 已在备份目录（backupCommitted 已执行但 backupOldRef 未写入 journal）
                val existingBackup = storage.resolveBackup(journal.txId, oldRef.relativePath)
                if (existingBackup != null) {
                    // backup 已存在于备份目录，跳过重复 backup
                    DiagnosticsLogger.i(TAG, "Recover: backup already exists for ${key.chapterId}, using existing backup")
                    currentItems[key] = item.copy(backupOldRef = existingBackup, state = PendingItem.STATE_OLD_BACKED_UP)
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
                        )
                    ) {
                        DiagnosticsLogger.w(TAG, "Recover backup: journal write failed for ${key.chapterId}, keeping journal")
                        return
                    }
                } else {
                    // backup 不存在，正常 backup
                    val backup = storage.backupCommitted(journal.txId, oldRef, MIME_MARKDOWN)
                    if (backup == null) {
                        DiagnosticsLogger.w(TAG, "Recover backup failed for ${key.chapterId}")
                        // #649 评论 5563798095：backup 失败时保留 journal 和 backup，不破坏性清理
                        for ((_, entry) in promotedEntries) {
                            storage.delete(MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath))
                        }
                        return
                    }
                    currentItems[key] = item.copy(backupOldRef = backup, state = PendingItem.STATE_OLD_BACKED_UP)
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
                        )
                    ) {
                        DiagnosticsLogger.w(TAG, "Recover backup: journal write failed for ${key.chapterId}")
                        // #649 评论 5563798095：journal 失败时保留 journal，不破坏性清理
                        for ((_, entry) in promotedEntries) {
                            storage.delete(MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath))
                        }
                        return
                    }
                }
            }
            // 2. promote staged（不删 old）
            val newRef = storage.promoteStaged(staged, staged.finalRelativePath)
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
                // #649 评论 5563333323 缺口 3：调用统一 cleanup 函数，不复制两份逻辑。
                // allLiveKeys = journal.newEntries.keys：只清理已删除章节的旧 ref。
                if (cleanupCommittedTransaction(journal, storage, allLiveKeys = journal.newEntries.keys)) {
                    // 全部清理成功，清除 journal
                    stateStore.clearPendingPublish()
                } else {
                    // 有失败项，保留 journal，下次 recover 继续清
                    DiagnosticsLogger.w(TAG, "Recover cleanup: partial failure for UPSERT_PROJECT ${journal.projectId}, keeping journal")
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
                        DiagnosticsLogger.w(TAG, "Recover cleanup: manifest rewrite failed for DELETE_PROJECT ${journal.projectId}")
                        return
                    }
                }
                // 2. 从 stateStore 删除该项目条目（若尚未删）
                //    #649 评论 5563333323 缺口 2：removeAllProjectEntries 返回 Result
                val removeResult = stateStore.removeAllProjectEntries(journal.projectId)
                if (removeResult.isFailure) {
                    DiagnosticsLogger.w(TAG, "Recover cleanup: removeAllProjectEntries failed for ${journal.projectId}, keeping journal")
                    return
                }
                // 3. 调用统一 cleanup 删旧正文 + manifestBackup + tx staging
                //    allLiveKeys = null：DELETE_PROJECT 时 oldEntries 全部要删
                if (cleanupCommittedTransaction(journal, storage, allLiveKeys = null)) {
                    stateStore.clearPendingPublish()
                } else {
                    DiagnosticsLogger.w(TAG, "Recover cleanup: partial failure for DELETE_PROJECT ${journal.projectId}, keeping journal")
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
        for ((_, item) in journal.items) {
            try {
                item.backupOldRef?.let { ref ->
                    // 先 resolve 确认文件仍存在，不存在视为目标已达到（幂等）
                    val resolved = storage.resolve(ref.relativePath)
                    if (resolved != null) {
                        if (!storage.delete(resolved)) allSuccess = false
                    }
                    // resolved == null：文件已不存在，目标已达到，视为成功
                }
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "cleanup: failed to delete backupOldRef ${item.backupOldRef?.uri}", e)
                allSuccess = false
            }
        }
        // 2. 已删除章节的旧 ref（snapshot 中已不存在的旧 key）
        //    先 resolve 确认文件存在再删，避免 MediaStore 同一条 row 被删两次
        for ((key, entry) in journal.oldEntries) {
            val shouldDelete = allLiveKeys?.let { key !in it } ?: true
            if (shouldDelete) {
                try {
                    val resolved = storage.resolve(entry.relativePath)
                    if (resolved != null) {
                        if (!storage.delete(resolved)) allSuccess = false
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
        try {
            journal.manifestBackupRef?.let { ref ->
                val resolved = storage.resolve(ref.relativePath)
                if (resolved != null) {
                    if (!storage.delete(resolved)) allSuccess = false
                }
            }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to delete manifestBackupRef ${journal.manifestBackupRef?.uri}", e)
            allSuccess = false
        }
        // 4. tx staging/backup 根（事务暂存目录）
        try {
            storage.rollback(journal.txId)
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "cleanup: failed to rollback tx ${journal.txId}", e)
            // rollback 失败不致命，staging 目录残留可下次清
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
            // #649 评论 5564379115 问题 5：用严格接口 currentResult()，不再用已废弃的 current()。
            // 首次安装 state.json 不存在是合法情况，明确初始化为 MEDIA_STORE。
            val storageResult = router.currentResult()
            if (storageResult.isFailure) {
                // state.json 损坏或 DOCUMENT_TREE + treeUri=null
                val error = storageResult.exceptionOrNull()
                DiagnosticsLogger.e(TAG, "Failed to get storage: ${error?.message}")
                return MirrorPublishResult.RetryableFailure
            }
            val storage = storageResult.getOrThrow()
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
                    )
            }

            // 写 pendingPublish journal（记录 staging 完成）
            // #649 评论 5563333323 缺口 2：journal 写入失败则停止本轮镜像操作
            if (!writePendingPublishJournal(
                    projectId = projectId,
                    transactionType = MirrorTransactionType.UPSERT_PROJECT,
                    phase = PendingMirrorPublish.PHASE_PROMOTE,
                    txId = txId,
                    backend = stateStore.getBackend(),
                    treeUri = stateStore.getTreeUri(),
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
            ) {
                DiagnosticsLogger.w(TAG, "Publish project $projectId aborted: journal write failed after stage")
                storage.rollback(txId)
                return MirrorPublishResult.RetryableFailure
            }

            // #649 评论 5564624383 问题 2：journalContext 在 promote 循环前创建，
            // 让 promote 失败时能传给 rollbackWholePublishTransaction 写 rollback journal
            val journalContext =
                PendingMirrorPublish(
                    txId = txId,
                    backend = stateStore.getBackend(),
                    treeUri = stateStore.getTreeUri(),
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
            // #649 评论 5562715833 问题 2：backupCommitted + promoteStaged，不先删 old
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
                // 1. 备份 old（如果有）
                if (oldRef != null) {
                    val backup = storage.backupCommitted(txId, oldRef, MIME_MARKDOWN)
                    if (backup == null) {
                        DiagnosticsLogger.w(
                            TAG,
                            "Publish project $projectId aborted: backup failed for ${key.chapterId}",
                        )
                        rollbackWholePublishTransaction(txId, items, stagedRefs, storage, journalContext)
                        return MirrorPublishResult.RetryableFailure
                    }
                    items[key] = item.copy(backupOldRef = backup, state = PendingItem.STATE_OLD_BACKED_UP)
                    // #649 评论 5563333323 缺口 2：journal 写入失败则停止
                    if (!writePendingPublishJournal(
                            projectId = projectId,
                            transactionType = MirrorTransactionType.UPSERT_PROJECT,
                            phase = PendingMirrorPublish.PHASE_PROMOTE,
                            txId = txId,
                            backend = stateStore.getBackend(),
                            treeUri = stateStore.getTreeUri(),
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
                    ) {
                        DiagnosticsLogger.w(
                            TAG,
                            "Publish project $projectId aborted: journal write failed after backup for ${key.chapterId}",
                        )
                        // #649 评论 5564379115 问题 2：统一事务回滚，不逐 item 回滚
                        rollbackWholePublishTransaction(txId, items, stagedRefs, storage, journalContext)
                        return MirrorPublishResult.RetryableFailure
                    }
                }
                // 2. promote staged（不删 old）
                val newRef = storage.promoteStaged(staged, staged.finalRelativePath)
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
                if (!writePendingPublishJournal(
                        projectId = projectId,
                        transactionType = MirrorTransactionType.UPSERT_PROJECT,
                        phase = PendingMirrorPublish.PHASE_PROMOTE,
                        txId = txId,
                        backend = stateStore.getBackend(),
                        treeUri = stateStore.getTreeUri(),
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
            // manifest 成功后一次性写 desiredEntries 到 stateStore
            // #649 评论 5563333323 缺口 2：putChapterEntries 失败也不清 journal
            if (!stateStore.putChapterEntries(promotedEntries)) {
                DiagnosticsLogger.w(TAG, "Publish project $projectId: putChapterEntries failed, keeping journal for retry")
                return MirrorPublishResult.RetryableFailure
            }

            // 标记所有 item 为 COMMITTED，更新 journal 到 cleanup 阶段
            val committedItems = items.mapValues { it.value.copy(state = PendingItem.STATE_COMMITTED) }
            if (!writePendingPublishJournal(
                    projectId = projectId,
                    transactionType = MirrorTransactionType.UPSERT_PROJECT,
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    txId = txId,
                    backend = stateStore.getBackend(),
                    treeUri = stateStore.getTreeUri(),
                    oldEntries = oldEntries,
                    newEntries = promotedEntries,
                    stagedRefs = emptyMap(),
                    items = committedItems,
                    removedProjectIds = emptySet(),
                    manifestOldRef = manifestResult.manifestOldRef,
                    manifestStagedRef = manifestResult.manifestStagedRef,
                    manifestNewRef = manifestResult.newRef,
                    manifestBackupRef = manifestResult.backupOldRef,
                )
            ) {
                DiagnosticsLogger.w(TAG, "Publish project $projectId: cleanup journal write failed, keeping journal for retry")
                return MirrorPublishResult.RetryableFailure
            }

            // 5. 清理阶段：调用统一 cleanup 函数
            //    #649 评论 5563333323 缺口 3：统一 cleanupCommittedTransaction
            val cleanupJournal =
                PendingMirrorPublish(
                    txId = txId,
                    backend = stateStore.getBackend(),
                    treeUri = stateStore.getTreeUri(),
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
                )
            if (cleanupCommittedTransaction(cleanupJournal, storage, allLiveKeys = allKeys)) {
                // 全部清理成功，清除 journal
                stateStore.clearPendingPublish()
            } else {
                // 有失败项，保留 journal，下次 recover 继续清
                DiagnosticsLogger.w(TAG, "Publish project $projectId: cleanup partial failure, keeping journal for retry")
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
            // #649 评论 5564379115 问题 5：用严格接口
            val storageResult = router.currentResult()
            if (storageResult.isFailure) {
                val error = storageResult.exceptionOrNull()
                DiagnosticsLogger.e(TAG, "Failed to get storage for delete: ${error?.message}")
                return MirrorPublishResult.RetryableFailure
            }
            val storage = storageResult.getOrThrow()
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
            if (!writePendingPublishJournal(
                    projectId = projectId,
                    transactionType = MirrorTransactionType.DELETE_PROJECT,
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    txId = txId,
                    backend = stateStore.getBackend(),
                    treeUri = stateStore.getTreeUri(),
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
            val deleteJournalContext =
                PendingMirrorPublish(
                    txId = txId,
                    backend = stateStore.getBackend(),
                    treeUri = stateStore.getTreeUri(),
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
            if (!writePendingPublishJournal(
                    projectId = projectId,
                    transactionType = MirrorTransactionType.DELETE_PROJECT,
                    phase = PendingMirrorPublish.PHASE_CLEANUP,
                    txId = txId,
                    backend = stateStore.getBackend(),
                    treeUri = stateStore.getTreeUri(),
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
                )
            ) {
                DiagnosticsLogger.w(TAG, "Delete project $projectId: cleanup journal write failed, keeping journal for retry")
                return MirrorPublishResult.RetryableFailure
            }
            // 5. 从 state store 删除该项目条目
            //    #649 评论 5563333323 缺口 2：removeAllProjectEntries 返回 Result
            val removeResult = stateStore.removeAllProjectEntries(projectId)
            if (removeResult.isFailure) {
                DiagnosticsLogger.w(TAG, "Delete project $projectId: removeAllProjectEntries failed, keeping journal for retry")
                return MirrorPublishResult.RetryableFailure
            }
            // 6. 调用统一 cleanup 删旧正文 + manifestBackup + tx staging
            //    #649 评论 5563333323 缺口 3：统一 cleanupCommittedTransaction
            val deleteCleanupJournal =
                PendingMirrorPublish(
                    txId = txId,
                    backend = stateStore.getBackend(),
                    treeUri = stateStore.getTreeUri(),
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
                )
            if (cleanupCommittedTransaction(deleteCleanupJournal, storage, allLiveKeys = null)) {
                stateStore.clearPendingPublish()
            } else {
                DiagnosticsLogger.w(TAG, "Delete project $projectId: cleanup partial failure, keeping journal for retry")
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
     * 回滚整个发布事务：恢复所有 item 的 old backup，再删除 tx staging。
     *
     * #649 评论 5564379115 问题 1/2：统一事务回滚，替代逐 item 回滚。
     * 逐 item 回滚会在第 N 章失败时把前 N-1 章的唯一 backup 一起删掉。
     *
     * #649 评论 5564624383 问题 2：rollback 本身做成 journal 状态。
     * 进程死在回滚中间，下次是继续回滚，不会又转回 forward promote。
     * 统一顺序：
     * 0. 把当前 items 写成 phase=rollback
     * 1. 删除所有 promotedRef
     * 2. 逐个恢复 backupOldRef / resolveBackup() 找到的旧正文，每恢复一个更新 journal
     * 3. 全部恢复成功后：删除 manifest backup → manifest final → rollback(txId) → clearPendingPublish
     *
     * @param txId 事务 ID
     * @param items 当前 items（包含 backupOldRef、promotedRef）
     * @param stagedRefs staged refs（包含 finalRelativePath）
     * @param storage 当前事务的 storage
     * @param journalContext 当前 journal 上下文（用于写 rollback journal）
     * @return true 表示回滚成功；false 表示恢复失败或部分失败（需要后续重试）
     */
    private suspend fun rollbackWholePublishTransaction(
        txId: String,
        items: Map<ChapterKey, PendingItem>,
        stagedRefs: Map<ChapterKey, StagedMirrorRef>,
        storage: ReadableMirrorStorage,
        journalContext: PendingMirrorPublish? = null,
    ): Boolean {
        // 0. 写 rollback journal（让进程死在回滚中间时能继续回滚）
        if (journalContext != null) {
            if (!writePendingPublishJournal(
                    projectId = journalContext.projectId,
                    transactionType = journalContext.transactionType,
                    phase = PendingMirrorPublish.PHASE_ROLLBACK,
                    txId = txId,
                    backend = journalContext.backend,
                    treeUri = journalContext.treeUri,
                    oldEntries = journalContext.oldEntries,
                    newEntries = journalContext.newEntries,
                    stagedRefs = journalContext.stagedRefs,
                    items = items,
                    removedProjectIds = journalContext.removedProjectIds,
                    manifestOldRef = journalContext.manifestOldRef,
                    manifestStagedRef = journalContext.manifestStagedRef,
                    manifestNewRef = journalContext.manifestNewRef,
                    manifestBackupRef = journalContext.manifestBackupRef,
                )
            ) {
                DiagnosticsLogger.w(TAG, "rollback: journal write failed at start")
                return false
            }
        }

        // 1. 删除所有 promotedRef
        for ((_, item) in items) {
            item.promotedRef?.let { storage.delete(it) }
        }

        // 2. 逐个恢复 backupOldRef / resolveBackup() 找到的旧正文到最终路径
        //    每恢复一个更新 journal，让崩溃后能继续
        val currentItems = items.toMutableMap()
        for ((key, item) in currentItems.toMap()) {
            val staged = item.stagedRef ?: stagedRefs[key]
            if (staged == null) continue
            val finalPath = staged.finalRelativePath
            if (storage.resolve(finalPath) != null) {
                // 已在最终位置，标记已恢复
                currentItems[key] = item.copy(backupOldRef = null)
                continue
            }
            val backup = item.backupOldRef
                ?: storage.resolveBackup(txId, finalPath)
            if (backup != null) {
                if (storage.restoreBackup(backup, finalPath, MIME_MARKDOWN) == null) {
                    DiagnosticsLogger.w(TAG, "rollback: failed to restore backup for ${key.chapterId}")
                    // 标记已尝试恢复但失败，保留 backupOldRef 供下次重试
                    if (journalContext != null) {
                        writePendingPublishJournal(
                            projectId = journalContext.projectId,
                            transactionType = journalContext.transactionType,
                            phase = PendingMirrorPublish.PHASE_ROLLBACK,
                            txId = txId,
                            backend = journalContext.backend,
                            treeUri = journalContext.treeUri,
                            oldEntries = journalContext.oldEntries,
                            newEntries = journalContext.newEntries,
                            stagedRefs = journalContext.stagedRefs,
                            items = currentItems,
                            removedProjectIds = journalContext.removedProjectIds,
                            manifestOldRef = journalContext.manifestOldRef,
                            manifestStagedRef = journalContext.manifestStagedRef,
                            manifestNewRef = journalContext.manifestNewRef,
                            manifestBackupRef = journalContext.manifestBackupRef,
                        )
                    }
                    return false
                }
                // 恢复成功，清除 backupOldRef 表示已恢复
                currentItems[key] = item.copy(backupOldRef = null)
            }
            // backup 为 null 说明从未 backup（新建章节），跳过恢复

            // 更新 journal
            if (journalContext != null) {
                if (!writePendingPublishJournal(
                        projectId = journalContext.projectId,
                        transactionType = journalContext.transactionType,
                        phase = PendingMirrorPublish.PHASE_ROLLBACK,
                        txId = txId,
                        backend = journalContext.backend,
                        treeUri = journalContext.treeUri,
                        oldEntries = journalContext.oldEntries,
                        newEntries = journalContext.newEntries,
                        stagedRefs = journalContext.stagedRefs,
                        items = currentItems,
                        removedProjectIds = journalContext.removedProjectIds,
                        manifestOldRef = journalContext.manifestOldRef,
                        manifestStagedRef = journalContext.manifestStagedRef,
                        manifestNewRef = journalContext.manifestNewRef,
                        manifestBackupRef = journalContext.manifestBackupRef,
                    )
                ) {
                    DiagnosticsLogger.w(TAG, "rollback: journal write failed after restoring ${key.chapterId}")
                    return false
                }
            }
        }

        // 3. 全部旧正文恢复成功
        //    a. 恢复 manifest backup（如果存在）→ 删 manifest final
        //    b. rollback(txId)（删 staging）
        //    c. clearPendingPublish
        journalContext?.manifestBackupRef?.let { backup ->
            val manifestRelativePath = "$META_DIR/$MANIFEST_FILE_NAME"
            // 恢复 manifest backup 到最终位置
            storage.restoreBackup(backup, manifestRelativePath, MIME_JSON)
            // 删除 manifest final（已被 promote 覆盖的旧 final）
            storage.resolve(manifestRelativePath)?.let { storage.delete(it) }
            stateStore.setManifestUri(backup.uri)
        }
        // rollback(txId) 删 staging（backup 已不在 staging 内）
        storage.rollback(txId)
        stateStore.clearPendingPublish()
        return true
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
        val currentItems = journal.items.toMutableMap()
        var allRestored = true
        for ((key, item) in currentItems.toMap()) {
            if (item.backupOldRef == null) {
                // 已恢复（backupOldRef 已在上次 rollback 中清除）
                continue
            }
            val staged = item.stagedRef ?: journal.stagedRefs[key]
            if (staged == null) continue
            val finalPath = staged.finalRelativePath
            if (storage.resolve(finalPath) != null) {
                // 已在最终位置，标记已恢复
                currentItems[key] = item.copy(backupOldRef = null)
                continue
            }
            val backup = item.backupOldRef
                ?: storage.resolveBackup(journal.txId, finalPath)
            if (backup != null) {
                if (storage.restoreBackup(backup, finalPath, MIME_MARKDOWN) == null) {
                    DiagnosticsLogger.w(TAG, "Recover rollback: failed to restore backup for ${key.chapterId}")
                    allRestored = false
                    continue
                }
                currentItems[key] = item.copy(backupOldRef = null)
            }
        }
        if (allRestored) {
            // 全部恢复成功：恢复 manifest、删 staging、清 journal
            journal.manifestBackupRef?.let { backup ->
                val manifestRelativePath = "$META_DIR/$MANIFEST_FILE_NAME"
                storage.restoreBackup(backup, manifestRelativePath, MIME_JSON)
                storage.resolve(manifestRelativePath)?.let { storage.delete(it) }
                stateStore.setManifestUri(backup.uri)
            }
            storage.rollback(journal.txId)
            stateStore.clearPendingPublish()
        } else {
            // 部分恢复失败，更新 journal 保留进度
            writePendingPublishJournal(
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
            )
            DiagnosticsLogger.w(TAG, "Recover rollback: partial failure, keeping journal for retry")
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
    ): Boolean {
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
            )
        return stateStore.writePendingPublish(journal.toJson())
    }

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
     */
    private data class ManifestTransactionResult(
        val newRef: MirrorFileRef,
        val manifestOldRef: MirrorFileRef?,
        val manifestStagedRef: StagedMirrorRef,
        val backupOldRef: MirrorFileRef?,
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
        val json = buildManifestJsonForDesired(projectId, snapshot, desiredEntries) ?: return null
        val manifestRelativePath = "$META_DIR/$MANIFEST_FILE_NAME"
        // 1. stage manifest
        val staged =
            storage.stageText(
                txId = txId,
                relativePath = manifestRelativePath,
                mimeType = MIME_JSON,
                text = json,
            ) ?: return null
        // 写 journal：记录 manifestStagedRef
        // #649 评论 5563333323 缺口 2：journal 写入失败则停止
        if (!writeManifestJournal(
                journalContext = journalContext,
                items = items,
                manifestStagedRef = staged,
                manifestNewRef = journalContext.manifestNewRef,
                manifestBackupRef = journalContext.manifestBackupRef,
                isManifestCommitted = false,
            )
        ) {
            storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
            return null
        }
        // 2. 备份旧 manifest（如果存在）
        //    #649 评论 5564379115 问题 6：manifest 也要走同一套 backupCommitted/restoreBackup swap，
        //    且处理崩溃窗口。
        //    #649 评论 5564624383 问题 4：backup 和 final 同时存在是"旧 manifest 已进 backup，
        //    新 manifest 已 promote，但 journal 还没写完"的典型崩溃窗口。
        //    此时应该：保留 backup → 把现有 final 当成已经 promote 的 new manifest →
        //    journal 写 manifestNewRef=finalRef + manifestBackupRef=backup → 继续 setManifestUri/commit。
        //    不能删 backup，也不能再 promote 一份同名 manifest（MediaStore 会出现两条同路径记录）。
        val oldUri = journalContext.manifestNewRef?.uri ?: stateStore.getManifestUri()
        val oldRef = oldUri?.let { MirrorFileRef(uri = it, relativePath = manifestRelativePath) }
        var manifestBackupRef: MirrorFileRef? = null

        // #649 评论 5564379115 问题 6 + #649 评论 5564624383 问题 4：
        // 检查崩溃窗口 — backup 已存在于备份目录
        val existingBackup = storage.resolveBackup(txId, manifestRelativePath)
        if (existingBackup != null) {
            // backup 已存在（backupCommitted 已执行但 journal 未更新）
            manifestBackupRef = existingBackup
            // 检查 final 是否也存在（崩溃在 promote 成功 + journal 未写完之间）
            val existingFinal = storage.resolve(manifestRelativePath)
            if (existingFinal != null) {
                // final 和 backup 同时存在：旧 manifest 已进 backup，新 manifest 已 promote。
                // 保留 backup 用于 rollback，把 final 当作已 promote 的 new manifest。
                // 跳过 promote 步骤，直接继续 setManifestUri。
                if (!writeManifestJournal(
                        journalContext = journalContext,
                        items = items,
                        manifestStagedRef = staged,
                        manifestNewRef = existingFinal,
                        manifestBackupRef = manifestBackupRef,
                        isManifestCommitted = false,
                    )
                ) {
                    storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
                    return null
                }
                // 跳到 setManifestUri（步骤 4），不走 promote
                if (!stateStore.setManifestUri(existingFinal.uri)) {
                    DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed (crash window recovery)")
                    return null
                }
                // 写 journal：isManifestCommitted = true
                writeManifestJournal(
                    journalContext = journalContext,
                    items = items,
                    manifestStagedRef = staged,
                    manifestNewRef = existingFinal,
                    manifestBackupRef = manifestBackupRef,
                    isManifestCommitted = true,
                )
                return ManifestTransactionResult(
                    newRef = existingFinal,
                    manifestOldRef = oldRef,
                    manifestStagedRef = staged,
                    backupOldRef = manifestBackupRef,
                )
            }
            // final 不存在 + backup 存在 → backup 已完成，可以继续 promote
            // 写 journal：记录已检测到的 manifestBackupRef
            if (!writeManifestJournal(
                    journalContext = journalContext,
                    items = items,
                    manifestStagedRef = staged,
                    manifestNewRef = null,
                    manifestBackupRef = manifestBackupRef,
                    isManifestCommitted = false,
                )
            ) {
                storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
                return null
            }
        } else if (oldRef != null) {
            // 正常路径：backup 不存在，执行 backupCommitted
            val backup = storage.backupCommitted(txId, oldRef, MIME_JSON)
            if (backup == null) {
                // backup 失败：删 manifest staging，不动旧 manifest
                storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
                return null
            }
            manifestBackupRef = backup
            // 写 journal：记录 manifestBackupRef
            if (!writeManifestJournal(
                    journalContext = journalContext,
                    items = items,
                    manifestStagedRef = staged,
                    manifestNewRef = null,
                    manifestBackupRef = manifestBackupRef,
                    isManifestCommitted = false,
                )
            ) {
                // journal 失败：恢复 backup 到最终位置，删 staging
                storage.restoreBackup(backup, manifestRelativePath, MIME_JSON)
                storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
                return null
            }
        }
        // 3. promote 新 manifest（promoteStaged 不删旧 manifest，旧 manifest 已由 backupCommitted 移走）
        val newRef = storage.promoteStaged(staged, manifestRelativePath)
        if (newRef == null) {
            // promote 失败：恢复 backup（如果有），删 staging
            manifestBackupRef?.let { storage.restoreBackup(it, manifestRelativePath, MIME_JSON) }
            storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
            return null
        }
        // 写 journal：记录 manifestNewRef, manifestBackupRef
        if (!writeManifestJournal(
                journalContext = journalContext,
                items = items,
                manifestStagedRef = staged,
                manifestNewRef = newRef,
                manifestBackupRef = manifestBackupRef,
                isManifestCommitted = false,
            )
        ) {
            // journal 写失败：删新 manifest，恢复 backup（如果有）
            storage.delete(newRef)
            manifestBackupRef?.let { storage.restoreBackup(it, manifestRelativePath, MIME_JSON) }
            return null
        }
        // 4. setManifestUri
        //    #649 评论 5563333323 缺口 2：setManifestUri 返回 Boolean
        if (!stateStore.setManifestUri(newRef.uri)) {
            DiagnosticsLogger.w(TAG, "Manifest transaction: setManifestUri failed")
            storage.delete(newRef)
            manifestBackupRef?.let { storage.restoreBackup(it, manifestRelativePath, MIME_JSON) }
            return null
        }
        // 写 journal：isManifestCommitted = true
        if (!writeManifestJournal(
                journalContext = journalContext,
                items = items,
                manifestStagedRef = staged,
                manifestNewRef = newRef,
                manifestBackupRef = manifestBackupRef,
                isManifestCommitted = true,
            )
        ) {
            // journal 写失败：manifest 已 setManifestUri，但 isManifestCommitted 未落盘。
            // 返回成功让调用方继续；下次 recover 会发现 isManifestCommitted=false 重做。
            DiagnosticsLogger.w(TAG, "Manifest transaction: isManifestCommitted journal write failed")
        }
        return ManifestTransactionResult(
            newRef = newRef,
            manifestOldRef = oldRef,
            manifestStagedRef = staged,
            backupOldRef = manifestBackupRef,
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
