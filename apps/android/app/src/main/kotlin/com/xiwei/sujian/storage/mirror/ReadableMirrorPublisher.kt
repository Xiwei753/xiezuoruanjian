package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import com.xiwei.sujian.feature.project.data.model.VolumeWithChapters
import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant
import java.time.format.DateTimeFormatter

/**
 * ReadableMirrorPublisher — 异步发布正文到 Download/Sujian 镜像。
 *
 * #649 评论 5560971132 修复 4/6/7：重构发布器。
 * #649 评论 5561465552 第 3+4 点：改用 [ReadableMirrorStorage] 接口 + 事务性发布 + journal。
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
 * 构造时注入 [ReadableMirrorStorage]（替代直接用 [com.xiwei.sujian.core.platform.storage.downloads.MediaStoreDownloads]）。
 * 由 [selectMirrorStorage] 根据 stateStore.backend 选择 MediaStore 或 SAF 后端。
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
    private val storage: ReadableMirrorStorage,
    private val stateStore: ReadableMirrorStateStore,
) {
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
            if (!storage.isSupported()) {
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
     *
     * 1. **准备**：读完整快照和所有章节正文到内存，计算 desired state。
     * 2. **写入**：写所有章节正文，任一失败 → 返回，不动 stateStore，不写 manifest。
     * 3. **提交 manifest**：manifest 成功后才批量更新 stateStore。
     * 4. **清理**：删除不再被引用的旧文件。
     *
     * pendingPublish journal 在整个流程中记录进度，成功后清除。
     */
    suspend fun publishProject(projectId: String) {
        try {
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

            // 1. 准备阶段：收集 snapshot 中全部仍存在章节的 key
            val allKeys = mutableSetOf<ChapterKey>()
            for (volumeWithChapters in snapshot.volumes) {
                for (chapter in volumeWithChapters.chapters) {
                    allKeys.add(ChapterKey(projectId, volumeWithChapters.volume.id, chapter.id))
                }
            }

            // 写 pendingPublish journal（记录发布开始）
            writePendingPublishJournal(projectId, phase = "write", writtenKeys = emptyList(), deletedKeys = emptyList())

            // 2. 写入阶段：写所有章节正文到内存计划，再一次性写入
            val usedRelativePaths = mutableSetOf<String>()
            val writePlan = buildWritePlan(projectId, snapshot, oldEntries, usedRelativePaths)
            if (writePlan == null) {
                DiagnosticsLogger.w(TAG, "Publish project $projectId aborted: failed to build write plan")
                stateStore.clearPendingPublish()
                return
            }

            // 执行写入：任一章节失败 → 返回，不动 stateStore，不写 manifest
            val newEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
            val writtenKeys = mutableListOf<String>()
            for (planEntry in writePlan) {
                val ok = executeWritePlanEntry(planEntry, newEntries)
                if (!ok) {
                    DiagnosticsLogger.w(
                        TAG,
                        "Publish project $projectId aborted: write failed for ${planEntry.key.chapterId}",
                    )
                    // 不动 stateStore，不写 manifest；旧镜像保持不变
                    stateStore.clearPendingPublish()
                    return
                }
                writtenKeys.add(planEntry.key.chapterId)
                // 更新 journal 进度
                writePendingPublishJournal(
                    projectId,
                    phase = "write",
                    writtenKeys = writtenKeys,
                    deletedKeys = emptyList(),
                )
            }

            // 3. 提交 manifest：manifest 成功后才批量更新 stateStore
            val manifestOk = publishManifest()
            if (!manifestOk) {
                DiagnosticsLogger.w(TAG, "Publish project $projectId aborted: manifest write failed")
                stateStore.clearPendingPublish()
                return
            }
            // 批量提交新 state
            stateStore.putChapterEntries(newEntries)
            writePendingPublishJournal(
                projectId,
                phase = "cleanup",
                writtenKeys = writtenKeys,
                deletedKeys = emptyList(),
            )

            // 4. 清理阶段：删除 snapshot 中已不存在的旧 key
            val deletedKeys = mutableListOf<String>()
            for ((key, entry) in oldEntries) {
                if (key !in allKeys) {
                    val ref = MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath)
                    storage.delete(ref)
                    stateStore.removeChapterEntry(key.projectId, key.volumeId, key.chapterId)
                    deletedKeys.add(key.chapterId)
                }
            }
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
     */
    suspend fun deleteProject(projectId: String) {
        try {
            if (!storage.isSupported()) {
                DiagnosticsLogger.i(TAG, "Mirror delete skipped: storage not supported")
                return
            }
            // 1. 先从 state store 移除条目（manifest 不再引用该项目）
            val removed = stateStore.removeAllProjectEntries(projectId)
            // 2. 写新 manifest（已不含该项目）
            publishManifest()
            // 3. manifest 成功后再删旧 URI
            for ((_, entry) in removed) {
                try {
                    val ref = MirrorFileRef(uri = entry.uri, relativePath = entry.relativePath)
                    storage.delete(ref)
                } catch (e: Exception) {
                    DiagnosticsLogger.w(TAG, "Failed to delete URI ${entry.uri}", e)
                }
            }
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to delete project: ${e.message}", e)
        }
    }

    /**
     * 全量发布：遍历所有项目。
     */
    suspend fun publishAll() {
        try {
            if (!storage.isSupported()) {
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
            if (storage.replaceText(oldRef, entry.content)) {
                ref = oldRef
            } else {
                // 旧 URI 失效，新建并删旧
                storage.delete(oldRef)
                ref = storage.createText(relativeDir, displayName, MIME_MARKDOWN, entry.content)
            }
        } else if (oldEntry != null && oldEntry.relativePath != entry.relativePath) {
            // 路径变化（作品名/卷名/章节名改动）：先在新路径创建成功
            val newRef = storage.createText(relativeDir, displayName, MIME_MARKDOWN, entry.content)
            if (newRef != null) {
                // 新文件就绪后旧文件先保留（不立即删），由清理阶段处理
                ref = newRef
            } else {
                ref = null
            }
        } else {
            // 无旧条目，直接创建
            ref = storage.createText(relativeDir, displayName, MIME_MARKDOWN, entry.content)
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
     *   "projectId": "<id>",
     *   "phase": "write" | "cleanup",
     *   "writtenKeys": ["<chapterId>", ...],
     *   "deletedKeys": ["<chapterId>", ...],
     *   "updatedAt": "<ISO-8601>"
     * }
     * ```
     */
    private fun writePendingPublishJournal(
        projectId: String,
        phase: String,
        writtenKeys: List<String>,
        deletedKeys: List<String>,
    ) {
        val journal =
            JSONObject().apply {
                put("projectId", projectId)
                put("phase", phase)
                put("writtenKeys", JSONArray(writtenKeys))
                put("deletedKeys", JSONArray(deletedKeys))
                put("updatedAt", DateTimeFormatter.ISO_INSTANT.format(Instant.now()))
            }
        stateStore.writePendingPublish(journal.toString())
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
            if (storage.replaceText(oldRef, content)) {
                ref = oldRef
            } else {
                // 旧 URI 失效，新建并删旧
                storage.delete(oldRef)
                ref = storage.createText(relativeDir, displayName, MIME_MARKDOWN, content)
            }
        } else if (oldEntry != null && oldEntry.relativePath != relativePath) {
            // 路径变化（作品名/卷名/章节名改动）：先在新路径创建成功
            val newRef = storage.createText(relativeDir, displayName, MIME_MARKDOWN, content)
            if (newRef != null) {
                // 新文件就绪后再删旧文件，避免文件停在旧路径
                val oldRef = MirrorFileRef(uri = oldEntry.uri, relativePath = oldEntry.relativePath)
                storage.delete(oldRef)
                ref = newRef
            } else {
                ref = null
            }
        } else {
            // 无旧条目，直接创建
            ref = storage.createText(relativeDir, displayName, MIME_MARKDOWN, content)
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
     * 写 manifest 文件，复用旧 URI 覆盖写。
     *
     * @return 是否写入成功。
     */
    private fun writeManifestFile(json: String): Boolean {
        val oldUri = stateStore.getManifestUri()
        val ref: MirrorFileRef?
        if (oldUri != null) {
            val oldRef = MirrorFileRef(uri = oldUri, relativePath = "$META_DIR/$MANIFEST_FILE_NAME")
            if (storage.replaceText(oldRef, json)) {
                ref = oldRef
            } else {
                storage.delete(oldRef)
                ref = storage.createText(META_DIR, MANIFEST_FILE_NAME, MIME_JSON, json)
            }
        } else {
            ref = storage.createText(META_DIR, MANIFEST_FILE_NAME, MIME_JSON, json)
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

/**
 * 把 ProjectWorkspaceSnapshot 转成 MirrorProject。
 *
 * contentFile 用用户可读的相对路径（`作品/作品名/卷名/章节名.md`），
 * 从 state store 的条目里取（Publisher 写入时已算好重名处理）。
 * contentHash 用 state store 里存的 SHA-256（Publisher 写入时已算好）。
 */
private fun ProjectWorkspaceSnapshot.toMirrorProject(entries: Map<ChapterKey, ChapterMirrorEntry>): MirrorProject {
    return MirrorProject(
        id = this.project.id,
        title = this.project.title,
        order = 0,
        revision = this.project.updatedAt.toEpochMillis(),
        updatedAt = this.project.updatedAt,
        volumes = this.volumes.map { it.toMirrorVolume(this.project.id, entries) },
    )
}

private fun VolumeWithChapters.toMirrorVolume(
    projectId: String,
    entries: Map<ChapterKey, ChapterMirrorEntry>,
): MirrorVolume {
    return MirrorVolume(
        id = this.volume.id,
        title = this.volume.title,
        order = this.volume.order,
        revision = this.volume.updatedAt.toEpochMillis(),
        updatedAt = this.volume.updatedAt,
        chapters =
            this.chapters.map { chapter ->
                val key = ChapterKey(projectId, this.volume.id, chapter.id)
                val entry = entries[key]
                MirrorChapter(
                    id = chapter.id,
                    title = chapter.title,
                    order = chapter.order,
                    revision = chapter.updatedAt.toEpochMillis(),
                    updatedAt = chapter.updatedAt,
                    contentFile = entry?.relativePath ?: "",
                    contentHash = entry?.contentHash ?: "",
                )
            },
    )
}

/**
 * 把 ISO-8601 字符串转成毫秒级时间戳。
 */
private fun String.toEpochMillis(): Long {
    return try {
        Instant.parse(this).toEpochMilli()
    } catch (e: Exception) {
        System.currentTimeMillis()
    }
}
