package com.xiwei.sujian.storage.mirror

import android.net.Uri
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.platform.storage.downloads.MediaStoreDownloads
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import com.xiwei.sujian.feature.project.data.model.VolumeWithChapters
import java.time.Instant
import java.time.format.DateTimeFormatter

/**
 * ReadableMirrorPublisher — 异步发布正文到 Download/Sujian 镜像。
 *
 * #649 评论 5560971132 修复 4/6/7：重构发布器。
 *
 * ## 修复 4：用户可读路径
 * 旧路径 `projects/<id>/volumes/<vid>/chapters/<cid>.md` 对用户不可读。
 * 新路径 `作品/<作品名>/<卷名>/<章节名>.md`，标题经 [sanitizeFileName] 净化。
 * 同目录重名时给文件名追加 chapterId 前 8 字符。
 *
 * ## 修复 6：删除旧文件
 * 用 [ReadableMirrorStateStore] 跟踪每个章节对应的 MediaStore URI。发布时：
 * - 章节仍存在：覆盖写旧 URI（或 URI 失效时新建）。
 * - 章节已删除：从 state store 拿旧 URI，调 [MediaStoreDownloads.delete]。
 * - 项目删除：逐个删旧 URI，不查 Core。
 *
 * ## 修复 7：contentHash 用 SHA-256
 * manifest 的 `contentHash` 用 [computeContentHash]（SHA-256）对实际正文计算，
 * 不再用 Core 的 `chapter.hash`（MD5）。恢复时用 [verifyContentHash] 校验。
 *
 * ## 循环依赖
 * 只依赖 [MirrorSnapshotSource]（只读快照），不持有 AppServiceBridge/ProjectBridge，
 * 切断 `AppServiceBridge → MirrorChangeSink → Publisher → AppServiceBridge` 循环。
 *
 * ## 安全约束
 * - 不把 `content://` URI 传给 Rust——只把文本写入 MediaStore。
 * - 所有 I/O 失败只记日志，不阻断业务。
 */
class ReadableMirrorPublisher(
    private val source: MirrorSnapshotSource,
    private val mediaStore: MediaStoreDownloads,
    private val stateStore: ReadableMirrorStateStore,
) {
    /**
     * 发布单章正文。
     *
     * 读章节内容 → 写文件 → 更新 state store → 更新 manifest。
     */
    suspend fun publishChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        try {
            if (!mediaStore.isSupported()) {
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
     * 发布整个项目：写当前所有章节，删除已不存在的旧章节 URI，更新 manifest。
     *
     * 完整结果语义：先从 snapshot 得到全部仍存在章节的 key；
     * 任一章节读取或写入失败时，本次发布返回失败，保留旧镜像和旧 manifest，不执行删除。
     * 只有所有当前章节都成功后，才删除 snapshot 中已不存在的旧 key，再写新 manifest。
     */
    suspend fun publishProject(projectId: String) {
        try {
            if (!mediaStore.isSupported()) {
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

            // 先收集 snapshot 中全部仍存在章节的 key，删除判断只看这个集合
            val allKeys = mutableSetOf<ChapterKey>()
            for (volumeWithChapters in snapshot.volumes) {
                for (chapter in volumeWithChapters.chapters) {
                    allKeys.add(ChapterKey(projectId, volumeWithChapters.volume.id, chapter.id))
                }
            }

            val usedRelativePaths = mutableSetOf<String>()
            val newKeys = publishProjectChapters(projectId, snapshot, oldEntries, usedRelativePaths)

            // 任一章节失败：保留旧镜像和旧 manifest，不执行删除
            if (newKeys == null) {
                DiagnosticsLogger.w(TAG, "Publish project $projectId aborted: some chapters failed")
                return
            }

            // 所有章节都成功后，删除 snapshot 中已不存在的旧 key
            for ((key, entry) in oldEntries) {
                if (key !in allKeys) {
                    mediaStore.delete(Uri.parse(entry.uri))
                    stateStore.removeChapterEntry(key.projectId, key.volumeId, key.chapterId)
                }
            }
            publishManifest()
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish project: ${e.message}", e)
        }
    }

    /**
     * 遍历项目所有卷/章节，写正文并收集成功写入的 key。
     *
     * @return 成功写入的 key 集合；任一章节读取或写入失败时返回 null。
     */
    private suspend fun publishProjectChapters(
        projectId: String,
        snapshot: ProjectWorkspaceSnapshot,
        oldEntries: Map<ChapterKey, ReadableMirrorStateStore.ChapterMirrorEntry>,
        usedRelativePaths: MutableSet<String>,
    ): Set<ChapterKey>? {
        val newKeys = mutableSetOf<ChapterKey>()
        for (volumeWithChapters in snapshot.volumes) {
            for (chapter in volumeWithChapters.chapters) {
                val openResult = source.openChapter(projectId, volumeWithChapters.volume.id, chapter.id)
                if (openResult !is BridgeResult.Success) {
                    DiagnosticsLogger.w(TAG, "Failed to open chapter ${chapter.id}, skip")
                    return null
                }
                val content = openResult.data.content
                val relativePath =
                    resolveChapterRelativePath(
                        snapshot.project.title,
                        volumeWithChapters.volume.title,
                        chapter,
                        oldEntries,
                        ChapterKey(projectId, volumeWithChapters.volume.id, chapter.id),
                        usedRelativePaths,
                    )
                usedRelativePaths.add(relativePath)
                val ok = writeChapterContent(
                    projectId,
                    volumeWithChapters.volume.id,
                    chapter.id,
                    chapter,
                    relativePath,
                    content,
                )
                if (!ok) {
                    DiagnosticsLogger.w(TAG, "Failed to write chapter ${chapter.id}, skip")
                    return null
                }
                newKeys.add(ChapterKey(projectId, volumeWithChapters.volume.id, chapter.id))
            }
        }
        return newKeys
    }

    /**
     * 删除项目镜像：从 state store 拿旧条目逐个删 URI，不查 Core。
     */
    suspend fun deleteProject(projectId: String) {
        try {
            if (!mediaStore.isSupported()) {
                DiagnosticsLogger.i(TAG, "Mirror delete skipped: MediaStore.Downloads not supported")
                return
            }
            val removed = stateStore.removeAllProjectEntries(projectId)
            for ((_, entry) in removed) {
                try {
                    mediaStore.delete(Uri.parse(entry.uri))
                } catch (e: Exception) {
                    DiagnosticsLogger.w(TAG, "Failed to delete URI ${entry.uri}", e)
                }
            }
            publishManifest()
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to delete project: ${e.message}", e)
        }
    }

    /**
     * 全量发布：遍历所有项目。
     */
    suspend fun publishAll() {
        try {
            if (!mediaStore.isSupported()) {
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
    private fun cleanupStaleProjects(liveProjectIds: Set<String>) {
        for (staleProjectId in stateStore.getAllProjectIds()) {
            if (staleProjectId !in liveProjectIds) {
                deleteProjectMirrorFiles(staleProjectId)
            }
        }
    }

    /** 删除某项目的全部镜像文件（从 state store 移除条目并逐个删 URI）。 */
    private fun deleteProjectMirrorFiles(projectId: String) {
        val removed = stateStore.removeAllProjectEntries(projectId)
        for ((_, entry) in removed) {
            try {
                mediaStore.delete(Uri.parse(entry.uri))
            } catch (_: Exception) {
            }
        }
    }

    // ── 内部 ──

    /**
     * 写单章正文到 MediaStore，更新 state store。
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

        val uri: String?
        if (oldEntry != null && oldEntry.relativePath == relativePath) {
            // 同路径：覆盖旧 URI
            val oldUri = Uri.parse(oldEntry.uri)
            if (mediaStore.replaceText(oldUri, content)) {
                uri = oldEntry.uri
            } else {
                // 旧 URI 失效，新建并删旧
                mediaStore.delete(oldUri)
                uri = mediaStore.createText(relativeDir, displayName, MIME_MARKDOWN, content)?.toString()
            }
        } else if (oldEntry != null && oldEntry.relativePath != relativePath) {
            // 路径变化（作品名/卷名/章节名改动）：先在新路径创建成功
            val newUri = mediaStore.createText(relativeDir, displayName, MIME_MARKDOWN, content)
            if (newUri != null) {
                // 新文件就绪后再删旧文件，避免文件停在旧路径
                mediaStore.delete(Uri.parse(oldEntry.uri))
                uri = newUri.toString()
            } else {
                uri = null
            }
        } else {
            // 无旧条目，直接创建
            uri = mediaStore.createText(relativeDir, displayName, MIME_MARKDOWN, content)?.toString()
        }

        return if (uri != null) {
            stateStore.putChapterEntry(
                projectId,
                volumeId,
                chapterId,
                ReadableMirrorStateStore.ChapterMirrorEntry(
                    uri = uri,
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
        oldEntries: Map<ChapterKey, ReadableMirrorStateStore.ChapterMirrorEntry>,
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
     */
    private suspend fun publishManifest() {
        val projectsResult = source.listProjects()
        if (projectsResult !is BridgeResult.Success) return
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
        writeManifestFile(json)
    }

    /**
     * 写 manifest 文件，复用旧 URI 覆盖写。
     */
    private fun writeManifestFile(json: String) {
        val oldUri = stateStore.getManifestUri()
        val uri =
            if (oldUri != null) {
                val parsed = Uri.parse(oldUri)
                if (mediaStore.replaceText(parsed, json)) {
                    oldUri
                } else {
                    mediaStore.delete(parsed)
                    mediaStore.createText(META_DIR, MANIFEST_FILE_NAME, MIME_JSON, json)?.toString()
                }
            } else {
                mediaStore.createText(META_DIR, MANIFEST_FILE_NAME, MIME_JSON, json)?.toString()
            }
        if (uri != null) {
            stateStore.setManifestUri(uri)
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
        private const val SKIP_NOT_SUPPORTED = "Mirror publish skipped: MediaStore.Downloads not supported"
    }
}

/**
 * 把 ProjectWorkspaceSnapshot 转成 MirrorProject。
 *
 * contentFile 用用户可读的相对路径（`作品/作品名/卷名/章节名.md`），
 * 从 state store 的条目里取（Publisher 写入时已算好重名处理）。
 * contentHash 用 state store 里存的 SHA-256（Publisher 写入时已算好）。
 */
private fun ProjectWorkspaceSnapshot.toMirrorProject(
    entries: Map<ChapterKey, ReadableMirrorStateStore.ChapterMirrorEntry>,
): MirrorProject {
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
    entries: Map<ChapterKey, ReadableMirrorStateStore.ChapterMirrorEntry>,
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
