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
     * 检查并恢复 pending publish（如果存在）。
     *
     * #649 评论 5561974464 问题 3：pendingPublish 没有恢复逻辑。
     * 在应用初始化/worker 开始消费业务事件前调用恢复方法。
     *
     * 恢复策略：
     * - `stage`：staging 未完成，旧镜像完整 → rollback(txId) + clearPendingPublish
     * - `promote`：staging 已写完，promote 部分完成 → 继续 promote 剩余，然后 cleanup
     * - `cleanup`：已 promote 完，stateStore 未更新/清理未完成 → 继续 cleanup
     */
    suspend fun recoverPendingPublishIfNeeded() {
        val journalJson = stateStore.readPendingPublish() ?: return
        val journal = PendingMirrorPublish.fromJson(journalJson) ?: return
        DiagnosticsLogger.i(TAG, "Recovering pending publish: phase=${journal.phase}, projectId=${journal.projectId}, txType=${journal.transactionType}")

        // #649 评论 5562462046 问题 3：恢复时按 journal 记录的 backend/treeUri 构造当时那套 storage，
        // 不能用 router.current() 猜（stateStore 可能已被改写）。
        val storage = router.forBackend(journal.backend, journal.treeUri)
        if (!storage.isSupported()) {
            DiagnosticsLogger.w(TAG, "Storage not supported during recovery, clearing pending publish")
            stateStore.clearPendingPublish()
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
            else -> {
                DiagnosticsLogger.w(TAG, "Unknown phase in pending publish: ${journal.phase}")
                stateStore.clearPendingPublish()
            }
        }
    }

    /**
     * 恢复 promote 阶段。
     *
     * #649 评论 5562462046 问题 3：只继续未完成的 item（state != COMMITTED），
     * 不能把所有 stagedRefs 从头再跑一遍。
     */
    private suspend fun recoverPromotePhase(
        journal: PendingMirrorPublish,
        storage: ReadableMirrorStorage,
    ) {
        val promotedEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        // 把已 COMMITTED 的 item 直接收进 promotedEntries；只对未完成的 item 继续 promote。
        val currentItems = journal.items.toMutableMap()
        for ((key, item) in currentItems.toMap()) {
            if (item.state == PendingItem.STATE_COMMITTED && item.promotedRef != null) {
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
            val result = storage.promote(staged, oldRef, staged.finalRelativePath)
            if (result == null) {
                DiagnosticsLogger.w(TAG, "Recover promote failed for ${key.chapterId}, rolling back")
                storage.rollback(journal.txId)
                // 回滚已 promote 的文件
                for ((_, entry) in promotedEntries) {
                    storage.delete(MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath))
                }
                stateStore.clearPendingPublish()
                return
            }
            promotedEntries[key] =
                ChapterMirrorEntry(
                    uri = result.newRef.uri,
                    relativePath = result.newRef.relativePath,
                    revision = journal.newEntries[key]?.revision ?: 0L,
                    contentHash = journal.newEntries[key]?.contentHash ?: "",
                )
            // 逐项更新 journal（记录该 item 已 PROMOTED）
            currentItems[key] = item.copy(promotedRef = result.newRef, state = PendingItem.STATE_PROMOTED)
            writePendingPublishJournal(
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
        }

        // 写 manifest（走事务性 manifest 写入）
        val snapshotResult = source.getProjectWorkspaceSnapshot(journal.projectId)
        if (snapshotResult !is BridgeResult.Success) {
            DiagnosticsLogger.w(TAG, "Failed to get snapshot for project ${journal.projectId} during recovery")
            storage.rollback(journal.txId)
            stateStore.clearPendingPublish()
            return
        }
        val manifestResult =
            publishManifestWithDesiredTransactional(
                projectId = journal.projectId,
                snapshot = snapshotResult.data,
                desiredEntries = promotedEntries,
                txId = journal.txId,
                existingJournal = journal,
                items = currentItems,
                storage = storage,
            )
        if (manifestResult == null) {
            DiagnosticsLogger.w(TAG, "Failed to write manifest during recovery")
            storage.rollback(journal.txId)
            stateStore.clearPendingPublish()
            return
        }
        // manifest 成功后批量更新 stateStore
        stateStore.putChapterEntries(promotedEntries)
        // 标记所有 item 为 COMMITTED
        val committedItems = currentItems.mapValues { it.value.copy(state = PendingItem.STATE_COMMITTED) }
        // 进入 cleanup 阶段
        recoverCleanupPhase(
            journal.copy(
                phase = PendingMirrorPublish.PHASE_CLEANUP,
                newEntries = promotedEntries,
                items = committedItems,
                manifestNewRef = manifestResult.newRef,
                manifestBackupRef = manifestResult.backupOldRef,
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
                // 删除 snapshot 中已不存在的旧 key
                val allKeys = journal.newEntries.keys
                for ((key, entry) in journal.oldEntries) {
                    if (key !in allKeys) {
                        val ref = MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath)
                        storage.delete(ref)
                        stateStore.removeChapterEntry(key.projectId, key.volumeId, key.chapterId)
                    }
                }
            }
            MirrorTransactionType.DELETE_PROJECT -> {
                // 1. 确保 manifest 已提交成不引用该项目（manifestNewRef 已在 journal 中）
                //    若 manifestNewRef 缺失，尝试重新写一次"不含该项目"的 manifest
                if (journal.manifestNewRef == null) {
                    val manifestOk = publishManifest()
                    if (!manifestOk) {
                        DiagnosticsLogger.w(TAG, "Recover cleanup: manifest rewrite failed for DELETE_PROJECT ${journal.projectId}")
                        // manifest 仍失败，保留 journal 等下次重试，不删旧正文
                        return
                    }
                }
                // 2. 从 stateStore 删除该项目条目（若尚未删）
                stateStore.removeAllProjectEntries(journal.projectId)
                // 3. 删旧正文
                for ((_, entry) in journal.oldEntries) {
                    try {
                        val ref = MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath)
                        storage.delete(ref)
                    } catch (e: Exception) {
                        DiagnosticsLogger.w(TAG, "Recover cleanup: failed to delete URI ${entry.uri}", e)
                    }
                }
                // 4. 删旧 manifest backup（若有）
                journal.manifestBackupRef?.let { storage.delete(it) }
            }
        }
        // 清除 journal
        stateStore.clearPendingPublish()
    }

    /**
     * 发布单章正文。
     *
     * 读章节内容 → 写文件 → 更新 state store → 更新 manifest。
     *
     * 单章发布走简化流程（不写 journal，因为单章失败影响范围小）。
     */
    suspend fun publishChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        try {
            if (!router.current().isSupported()) {
                DiagnosticsLogger.i(TAG, SKIP_NOT_SUPPORTED)
                return
            }
            val snapshotResult = source.getProjectWorkspaceSnapshot(projectId)
            if (snapshotResult !is BridgeResult.Success) {
                logNotLoaded(snapshotResult, "publishChapter")
                return
            }
            val snapshot = snapshotResult.data
            val volumeWithChapters =
                snapshot.volumes.find { it.volume.id == volumeId }
                    ?: run {
                        DiagnosticsLogger.w(TAG, "Volume $volumeId not found in project $projectId")
                        return
                    }
            val chapter =
                volumeWithChapters.chapters.find { it.id == chapterId }
                    ?: run {
                        DiagnosticsLogger.w(TAG, "Chapter $chapterId not found")
                        return
                    }

            val openResult = source.openChapter(projectId, volumeId, chapterId)
            if (openResult !is BridgeResult.Success) {
                logNotLoaded(openResult, "openChapter")
                return
            }
            val content = openResult.data.content
            val relativePath =
                resolveChapterRelativePath(
                    snapshot.project.title,
                    volumeWithChapters.volume.title,
                    chapter,
                    stateStore.getProjectEntries(projectId),
                    ChapterKey(projectId, volumeId, chapterId),
                )
            val ok = writeChapterContent(projectId, volumeId, chapterId, chapter, relativePath, content)
            if (ok) {
                publishManifest()
            }
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish chapter: ${e.message}", e)
        }
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
     */
    suspend fun publishProject(projectId: String) {
        try {
            val storage = router.current()
            if (!storage.isSupported()) {
                DiagnosticsLogger.i(TAG, SKIP_NOT_SUPPORTED)
                return
            }
            val snapshotResult = source.getProjectWorkspaceSnapshot(projectId)
            if (snapshotResult !is BridgeResult.Success) {
                logNotLoaded(snapshotResult, "publishProject")
                return
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
                return
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
                    return
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
            writePendingPublishJournal(
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

            // 3. 提升阶段：promote 所有暂存文件到最终位置（逐项更新 journal）
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
                val result = storage.promote(staged, oldRef, staged.finalRelativePath)
                if (result == null) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Publish project $projectId aborted: promote failed for ${key.chapterId}",
                    )
                    storage.rollback(txId)
                    // 回滚已 promote 的文件（如果有）
                    for ((_, promotedEntry) in promotedEntries) {
                        storage.delete(MirrorFileRef(uri = promotedEntry.uri, relativePath = promotedEntry.relativePath))
                    }
                    stateStore.clearPendingPublish()
                    return
                }
                promotedEntries[key] =
                    ChapterMirrorEntry(
                        uri = result.newRef.uri,
                        relativePath = result.newRef.relativePath,
                        revision = desiredEntries[key]!!.revision,
                        contentHash = desiredEntries[key]!!.contentHash,
                    )
                // 逐项更新 journal（记录该 item 已 PROMOTED）
                items[key] = item.copy(promotedRef = result.newRef, state = PendingItem.STATE_PROMOTED)
                writePendingPublishJournal(
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
            }

            // 4. 提交 manifest：走事务性 manifest 写入（stage → backup old → promote → setManifestUri → 删 backup）
            val manifestResult =
                publishManifestWithDesiredTransactional(
                    projectId = projectId,
                    snapshot = snapshot,
                    desiredEntries = promotedEntries,
                    txId = txId,
                    existingJournal = null,
                    items = items,
                    storage = storage,
                )
            if (manifestResult == null) {
                DiagnosticsLogger.w(TAG, "Publish project $projectId aborted: manifest write failed")
                storage.rollback(txId)
                stateStore.clearPendingPublish()
                return
            }
            // manifest 成功后一次性写 desiredEntries 到 stateStore
            stateStore.putChapterEntries(promotedEntries)

            // 标记所有 item 为 COMMITTED，更新 journal 到 cleanup 阶段
            val committedItems = items.mapValues { it.value.copy(state = PendingItem.STATE_COMMITTED) }
            writePendingPublishJournal(
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

            // 5. 清理阶段：删除 snapshot 中已不存在的旧 key
            for ((key, entry) in oldEntries) {
                if (key !in allKeys) {
                    val ref = MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath)
                    storage.delete(ref)
                    stateStore.removeChapterEntry(key.projectId, key.volumeId, key.chapterId)
                }
            }
            // 删旧 manifest backup（事务已提交）
            manifestResult.backupOldRef?.let { storage.delete(it) }
            // 发布成功，清除 journal
            stateStore.clearPendingPublish()
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish project: ${e.message}", e)
            // 异常时保留 journal，下次启动可继续
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
    suspend fun deleteProject(projectId: String) {
        try {
            val storage = router.current()
            if (!storage.isSupported()) {
                DiagnosticsLogger.i(TAG, "Mirror delete skipped: storage not supported")
                return
            }
            // 1. 获取旧条目
            val removed = stateStore.getProjectEntries(projectId)
            if (removed.isEmpty()) {
                // 没有条目，直接返回
                return
            }
            // 2. 写 pending journal（transactionType=DELETE_PROJECT, phase=CLEANUP）
            val txId = "${System.currentTimeMillis()}-${projectId.take(8)}"
            writePendingPublishJournal(
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
            // 3. 事务提交新 manifest（已不含该项目）
            //    用 desiredEntries=emptyMap 表示该项目不再有任何章节
            val snapshotResult = source.getProjectWorkspaceSnapshot(projectId)
            val snapshot = (snapshotResult as? BridgeResult.Success)?.data
            val manifestResult =
                publishManifestWithDesiredTransactional(
                    projectId = projectId,
                    snapshot = snapshot,
                    desiredEntries = emptyMap(),
                    txId = txId,
                    existingJournal = null,
                    items = emptyMap(),
                    storage = storage,
                )
            if (manifestResult == null) {
                DiagnosticsLogger.w(TAG, "Delete project $projectId aborted: manifest write failed")
                // manifest 失败不清除 journal，下次恢复会重试
                return
            }
            // 4. manifest 成功后再从 state store 删除该项目条目
            stateStore.removeAllProjectEntries(projectId)
            // 5. 删旧 URI
            for ((_, entry) in removed) {
                try {
                    val ref = MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath)
                    storage.delete(ref)
                } catch (e: Exception) {
                    DiagnosticsLogger.w(TAG, "Failed to delete URI ${entry.uri}", e)
                }
            }
            // 6. 删旧 manifest backup（事务已提交）
            manifestResult.backupOldRef?.let { storage.delete(it) }
            // 7. 删除成功，清除 journal
            stateStore.clearPendingPublish()
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to delete project: ${e.message}", e)
            // 异常时保留 journal，下次启动可继续
        }
    }

    /**
     * 全量发布：遍历所有项目。
     */
    suspend fun publishAll() {
        try {
            if (!router.current().isSupported()) {
                DiagnosticsLogger.i(TAG, SKIP_NOT_SUPPORTED)
                return
            }
            val projectsResult = source.listProjects()
            if (projectsResult !is BridgeResult.Success) {
                logNotLoaded(projectsResult, "publishAll")
                return
            }
            // 先清理 state store 中已不存在的项目
            val liveProjectIds = projectsResult.data.map { it.id }.toSet()
            cleanupStaleProjects(liveProjectIds)
            for (project in projectsResult.data) {
                publishProject(project.id)
            }
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish all: ${e.message}", e)
        }
    }

    /**
     * 清理 state store 中已不在 Core 的旧项目镜像。
     * 提取自 publishAll 以控制嵌套深度。
     */
    private suspend fun cleanupStaleProjects(liveProjectIds: Set<String>) {
        for (staleProjectId in stateStore.getAllProjectIds()) {
            if (staleProjectId !in liveProjectIds) {
                deleteProjectMirrorFiles(staleProjectId)
            }
        }
    }

    /** 删除某项目的全部镜像文件（从 state store 移除条目并逐个删 URI）。 */
    private suspend fun deleteProjectMirrorFiles(projectId: String) {
        deleteProject(projectId)
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
     * 执行写入计划单条：写正文到存储，记录新条目到 [newEntries]。
     *
     * @return 是否写入成功。
     */
    private fun executeWritePlanEntry(
        entry: WritePlanEntry,
        newEntries: MutableMap<ChapterKey, ChapterMirrorEntry>,
    ): Boolean {
        val contentHash = computeContentHash(entry.content)
        val relativeDir = entry.relativePath.substringBeforeLast('/', "")
        val displayName = entry.relativePath.substringAfterLast('/')
        val oldEntry = entry.oldEntry

        val ref: MirrorFileRef?
        if (oldEntry != null && oldEntry.relativePath == entry.relativePath) {
            // 同路径：覆盖旧 URI
            val oldRef = MirrorFileRef(uri = oldEntry.uri, relativePath = oldEntry.relativePath)
            if (router.current().replaceText(oldRef, entry.content)) {
                ref = oldRef
            } else {
                // 旧 URI 失效，新建并删旧
                router.current().delete(oldRef)
                ref = router.current().createText(relativeDir, displayName, MIME_MARKDOWN, entry.content)
            }
        } else if (oldEntry != null && oldEntry.relativePath != entry.relativePath) {
            // 路径变化（作品名/卷名/章节名改动）：先在新路径创建成功
            val newRef = router.current().createText(relativeDir, displayName, MIME_MARKDOWN, entry.content)
            if (newRef != null) {
                // 新文件就绪后旧文件先保留（不立即删），由清理阶段处理
                ref = newRef
            } else {
                ref = null
            }
        } else {
            // 无旧条目，直接创建
            ref = router.current().createText(relativeDir, displayName, MIME_MARKDOWN, entry.content)
        }

        return if (ref != null) {
            newEntries[entry.key] =
                ChapterMirrorEntry(
                    uri = ref.uri,
                    relativePath = entry.relativePath,
                    revision = entry.chapter.updatedAt.toEpochMillis(),
                    contentHash = contentHash,
                )
            true
        } else {
            DiagnosticsLogger.w(TAG, "Failed to write chapter content: ${entry.relativePath}")
            false
        }
    }

    /**
     * 写 pendingPublish journal。
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
    ) {
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
            )
        stateStore.writePendingPublish(journal.toJson())
    }

    // ── 内部 ──

    /**
     * 写单章正文到存储，更新 state store。
     *
     * 分支处理：
     * - 旧条目存在且 relativePath 未变：覆盖旧 URI；旧 URI 失效则新建。
     * - 旧条目存在但 relativePath 已变（重命名）：先在新路径创建成功，
     *   stateStore 记录新 URI 后再删旧 URI，避免文件停在旧路径。
     * - 无旧条目：直接 createText。
     *
     * @return 是否写入成功。
     */
    private fun writeChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
        chapter: ChapterMeta,
        relativePath: String,
        content: String,
    ): Boolean {
        val contentHash = computeContentHash(content)
        val oldEntry = stateStore.getProjectEntries(projectId)[ChapterKey(projectId, volumeId, chapterId)]
        val relativeDir = relativePath.substringBeforeLast('/', "")
        val displayName = relativePath.substringAfterLast('/')

        val ref: MirrorFileRef?
        if (oldEntry != null && oldEntry.relativePath == relativePath) {
            // 同路径：覆盖旧 URI
            val oldRef = MirrorFileRef(uri = oldEntry.uri, relativePath = oldEntry.relativePath)
            if (router.current().replaceText(oldRef, content)) {
                ref = oldRef
            } else {
                // 旧 URI 失效，新建并删旧
                router.current().delete(oldRef)
                ref = router.current().createText(relativeDir, displayName, MIME_MARKDOWN, content)
            }
        } else if (oldEntry != null && oldEntry.relativePath != relativePath) {
            // 路径变化（作品名/卷名/章节名改动）：先在新路径创建成功
            val newRef = router.current().createText(relativeDir, displayName, MIME_MARKDOWN, content)
            if (newRef != null) {
                // 新文件就绪后再删旧文件，避免文件停在旧路径
                val oldRef = MirrorFileRef(uri = oldEntry.uri, relativePath = oldEntry.relativePath)
                router.current().delete(oldRef)
                ref = newRef
            } else {
                ref = null
            }
        } else {
            // 无旧条目，直接创建
            ref = router.current().createText(relativeDir, displayName, MIME_MARKDOWN, content)
        }

        return if (ref != null) {
            stateStore.putChapterEntry(
                projectId,
                volumeId,
                chapterId,
                ChapterMirrorEntry(
                    uri = ref.uri,
                    relativePath = relativePath,
                    revision = chapter.updatedAt.toEpochMillis(),
                    contentHash = contentHash,
                ),
            )
            true
        } else {
            DiagnosticsLogger.w(TAG, "Failed to write chapter content: $relativePath")
            false
        }
    }

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
     * 生成并写入 manifest。
     *
     * manifest 的 contentHash 用 [computeContentHash]（SHA-256）对实际正文计算。
     *
     * @return manifest 是否写入成功。
     */
    private suspend fun publishManifest(): Boolean {
        val projectsResult = source.listProjects()
        if (projectsResult !is BridgeResult.Success) return false
        val projects = projectsResult.data

        val mirrorProjects = mutableListOf<MirrorProject>()
        for (project in projects) {
            val snapshotResult = source.getProjectWorkspaceSnapshot(project.id)
            if (snapshotResult is BridgeResult.Success) {
                val snapshot = snapshotResult.data
                val entries = stateStore.getProjectEntries(project.id)
                mirrorProjects.add(snapshot.toMirrorProject(entries))
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

        val json = manifestToJson(manifest)
        return writeManifestFile(json)
    }

    /**
     * 用 desired entries 生成并写入 manifest（事务性发布专用）。
     *
     * #649 评论 5561974464 问题 2：publishManifest() 仍用旧 state。
     * 事务性发布时，manifest 用 desiredEntries 直接构造，不从 stateStore 取旧 entries。
     *
     * 注意：此方法走非事务的 [writeManifestFile]（直接覆盖）。
     * 事务性发布应改用 [publishManifestWithDesiredTransactional]（#649 评论 5562462046 问题 2）。
     *
     * @param projectId 目标项目 ID（只更新这个项目的 manifest）
     * @param snapshot 项目快照
     * @param desiredEntries 期望的条目（promote 后的新条目）
     * @return manifest 是否写入成功。
     */
    private suspend fun publishManifestWithDesired(
        projectId: String,
        snapshot: ProjectWorkspaceSnapshot,
        desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
    ): Boolean {
        val json = buildManifestJsonForDesired(projectId, snapshot, desiredEntries) ?: return false
        return writeManifestFile(json)
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
     * 事务性写入 manifest（stage → backup old → promote → setManifestUri）。
     *
     * #649 评论 5562462046 问题 2：旧 [writeManifestFile] 直接 replaceText 覆盖，
     * SAF openOutputStream 出错时旧 manifest 可能被截断。新实现走和正文同一套事务：
     * 1. stage manifest（用 storage.stageText）
     * 2. 旧 manifest 移到 tx backup（不删除，记入 [ManifestTransactionResult.backupOldRef]）
     * 3. promote 新 manifest（真正 swap，create 成功后才删 old）
     * 4. setManifestUri(newRef)
     * 5. 旧 manifest backup 由调用方在事务完全提交后删（[ReadableMirrorStorage.delete]）
     *
     * @param storage 当前事务的 storage（由调用方传入，避免 router.current() 在事务中途变化）
     * @param existingJournal 恢复场景下已有的 journal（用于保留 manifestOldRef 等）；正常流程传 null
     * @return [ManifestTransactionResult]；任何步骤失败返回 null（不修改 stateStore 的 manifestUri）
     */
    private suspend fun publishManifestWithDesiredTransactional(
        projectId: String,
        snapshot: ProjectWorkspaceSnapshot?,
        desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        txId: String,
        existingJournal: PendingMirrorPublish?,
        items: Map<ChapterKey, PendingItem>,
        storage: ReadableMirrorStorage = router.current(),
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
        // 2. 旧 manifest ref（不删除，作为 backup）
        val oldUri = existingJournal?.manifestNewRef?.uri ?: stateStore.getManifestUri()
        val oldRef = oldUri?.let { MirrorFileRef(uri = it, relativePath = manifestRelativePath) }
        // 3. promote 新 manifest（真正 swap）
        val result = storage.promote(staged, oldRef, manifestRelativePath)
        if (result == null) {
            // promote 失败：删 manifest staging，不动旧 manifest
            storage.delete(MirrorFileRef(uri = staged.stagingUri, relativePath = staged.stagingRelativePath))
            return null
        }
        // 4. setManifestUri
        stateStore.setManifestUri(result.newRef.uri)
        return ManifestTransactionResult(
            newRef = result.newRef,
            manifestOldRef = oldRef,
            manifestStagedRef = staged,
            backupOldRef = oldRef,
        )
    }

    /**
     * 写 manifest 文件，复用旧 URI 覆盖写。
     *
     * 注意：此方法走非事务的覆盖写，供 [publishManifest]（单章发布简化路径）使用。
     * 事务性发布应改用 [publishManifestWithDesiredTransactional]。
     *
     * @return 是否写入成功。
     */
    private fun writeManifestFile(json: String): Boolean {
        val oldUri = stateStore.getManifestUri()
        val ref: MirrorFileRef?
        if (oldUri != null) {
            val oldRef = MirrorFileRef(uri = oldUri, relativePath = "$META_DIR/$MANIFEST_FILE_NAME")
            if (router.current().replaceText(oldRef, json)) {
                ref = oldRef
            } else {
                router.current().delete(oldRef)
                ref = router.current().createText(META_DIR, MANIFEST_FILE_NAME, MIME_JSON, json)
            }
        } else {
            ref = router.current().createText(META_DIR, MANIFEST_FILE_NAME, MIME_JSON, json)
        }
        return if (ref != null) {
            stateStore.setManifestUri(ref.uri)
            true
        } else {
            false
        }
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
