package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import java.time.Instant
import java.time.format.DateTimeFormatter

/**
 * 写入计划条目。
 */
internal data class WritePlanEntry(
    val key: ChapterKey,
    val chapter: ChapterMeta,
    val relativePath: String,
    val content: String,
    val oldEntry: ChapterMirrorEntry?,
)

/**
 * Mirror 发布计划构建器。
 *
 * 从 ReadableMirrorPublisher 提取的计划构建逻辑。
 * #649 评论 5560971132 重构：拆分 LargeClass。
 */
internal class MirrorPublishPlanner(
    private val source: MirrorSnapshotSource,
    private val stateStore: ReadableMirrorStateStore,
    private val codec: MirrorManifestCodec,
) {
    /**
     * 准备阶段：读完整快照和所有章节正文到内存，构建写入计划。
     *
     * @return 写入计划列表；任一章节读取失败返回 null。
     */
    suspend fun buildWritePlan(
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
     * 计算章节正文的相对路径（相对 `Download/Sujian/`）。
     *
     * 同目录重名处理：若 [usedRelativePaths] 或 [oldEntries] 中已有相同 relativePath
     * 但属于不同 chapterId，给文件名追加 `_<chapterId 前 8 字符>`。
     */
    fun resolveChapterRelativePath(
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
    suspend fun buildManifestJsonForDesired(
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
                if (snapshot != null) {
                    mirrorProjects.add(snapshot.toMirrorProject(desiredEntries))
                }
            } else {
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
        return codec.manifestToJson(manifest)
    }

    companion object {
        private const val TAG = "MirrorPublishPlanner"
        private const val MIN_ID_LENGTH = 8
    }
}
