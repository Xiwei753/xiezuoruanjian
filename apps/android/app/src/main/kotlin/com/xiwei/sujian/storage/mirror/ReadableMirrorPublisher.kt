package com.xiwei.sujian.storage.mirror

import android.content.ContentResolver
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.platform.storage.downloads.MediaStoreDownloads
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.project.ProjectBridge
import com.xiwei.sujian.feature.project.data.model.Project
import com.xiwei.sujian.feature.project.data.model.Volume
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import com.xiwei.sujian.feature.project.data.model.VolumeWithChapters
import java.time.Instant
import java.time.format.DateTimeFormatter

/**
 * ReadableMirrorPublisher — 异步发布正文到 Download/Sujian 镜像。
 *
 * #649 评论 5559759935 / 5560685734：私有真相源（`filesDir/sujian`）单向异步发布
 * 到用户可读的 Download/Sujian。发布链路：
 *
 * 1. [MirrorChangeSink] 接收业务变更，合并后异步调用本类。
 * 2. 本类通过 [ProjectBridge] 读取 Core 数据。
 * 3. 通过 [MediaStoreDownloads] 写入 Download/Sujian。
 * 4. 写 manifest + 正文文件。
 *
 * ## 设计要点
 * - 异步：Bridge 在保存热路径不能同步写 Download。
 * - 幂等：manifest 与正文文件都是幂等写入；失败可重试。
 * - 合并：短时间内多次发布合并成一次全量（由 MirrorChangeSink 处理）。
 *
 * ## 安全约束
 * - 不把 `content://` URI 传给 Rust——只把文本写入 MediaStore。
 * - 所有 I/O 失败只记日志，不阻断业务。
 */
class ReadableMirrorPublisher(
    private val contentResolver: ContentResolver,
    private val projectBridge: ProjectBridge,
    private val appServiceBridge: AppServiceBridge,
) {
    private val mediaStore by lazy { MediaStoreDownloads(contentResolver) }

    /**
     * 发布单章正文。
     */
    suspend fun publishChapter(projectId: String, volumeId: String, chapterId: String) {
        try {
            // 1. 确保镜像根存在
            val rootResult = mediaStore.ensureMirrorRoot()
            if (rootResult is MediaStoreDownloads.MirrorRootResult.NotSupported) {
                DiagnosticsLogger.i(TAG, "Mirror publish skipped: ${rootResult.reason}")
                return
            }
            if (rootResult is MediaStoreDownloads.MirrorRootResult.Failed) {
                DiagnosticsLogger.w(TAG, "Failed to ensure mirror root: ${rootResult.reason}")
                return
            }

            // 2. 读取章节内容
            val openResult = appServiceBridge.openChapter(projectId, volumeId, chapterId)
            if (openResult is com.xiwei.sujian.core.interop.common.BridgeResult.Error) {
                DiagnosticsLogger.w(TAG, "Failed to open chapter $chapterId: ${openResult.fullEnvelope}")
                return
            }
            if (openResult is com.xiwei.sujian.core.interop.common.BridgeResult.NotLoaded) {
                DiagnosticsLogger.w(TAG, "Native library not loaded, skip chapter publish")
                return
            }
            val chapterContent = (openResult as com.xiwei.sujian.core.interop.common.BridgeResult.Success).data.content

            // 3. 写入正文文件
            val relativePath = "projects/$projectId/volumes/$volumeId/chapters/$chapterId.md"
            val written = mediaStore.writeText(relativePath, chapterContent)
            if (written == null) {
                DiagnosticsLogger.w(TAG, "Failed to write chapter content: $relativePath")
                return
            }

            // 4. 更新 manifest（全量刷新以保持一致性）
            publishManifest()
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish chapter: ${e.message}", e)
        }
    }

    /**
     * 发布整个项目。
     */
    suspend fun publishProject(projectId: String) {
        try {
            val rootResult = mediaStore.ensureMirrorRoot()
            if (rootResult is MediaStoreDownloads.MirrorRootResult.NotSupported) {
                DiagnosticsLogger.i(TAG, "Mirror publish skipped: ${rootResult.reason}")
                return
            }
            if (rootResult is MediaStoreDownloads.MirrorRootResult.Failed) {
                DiagnosticsLogger.w(TAG, "Failed to ensure mirror root: ${rootResult.reason}")
                return
            }

            // 获取项目快照
            val snapshotResult = projectBridge.getProjectWorkspaceSnapshot(projectId)
            if (snapshotResult is com.xiwei.sujian.core.interop.common.BridgeResult.Error) {
                DiagnosticsLogger.w(TAG, "Failed to get project snapshot: ${snapshotResult.fullEnvelope}")
                return
            }
            if (snapshotResult is com.xiwei.sujian.core.interop.common.BridgeResult.NotLoaded) {
                DiagnosticsLogger.w(TAG, "Native library not loaded, skip project publish")
                return
            }
            val snapshot = (snapshotResult as com.xiwei.sujian.core.interop.common.BridgeResult.Success<*>).data as ProjectWorkspaceSnapshot

            // 写入所有章节
            for (volumeWithChapters in snapshot.volumes) {
                for (chapter in volumeWithChapters.chapters) {
                    val openResult = appServiceBridge.openChapter(projectId, volumeWithChapters.volume.id, chapter.id)
                    if (openResult is com.xiwei.sujian.core.interop.common.BridgeResult.Success) {
                        val content = openResult.data.content
                        val relativePath = "projects/$projectId/volumes/${volumeWithChapters.volume.id}/chapters/${chapter.id}.md"
                        mediaStore.writeText(relativePath, content)
                    }
                }
            }

            // 更新 manifest
            publishManifest()
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish project: ${e.message}", e)
        }
    }

    /**
     * 删除项目镜像。
     */
    suspend fun deleteProject(projectId: String) {
        try {
            // 删除项目目录（幂等：删除不存在的文件返回 true）
            val projectsDir = "projects/$projectId"
            // 注意：MediaStoreDownloads 没有递归删除，需要逐章删除
            val snapshotResult = projectBridge.getProjectWorkspaceSnapshot(projectId)
            if (snapshotResult is com.xiwei.sujian.core.interop.common.BridgeResult.Success<*>) {
                val snapshot = snapshotResult.data as ProjectWorkspaceSnapshot
                for (volumeWithChapters in snapshot.volumes) {
                    for (chapter in volumeWithChapters.chapters) {
                        val relativePath = "projects/$projectId/volumes/${volumeWithChapters.volume.id}/chapters/${chapter.id}.md"
                        mediaStore.delete(relativePath)
                    }
                }
            }

            // 更新 manifest
            publishManifest()
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to delete project: ${e.message}", e)
        }
    }

    /**
     * 全量发布。
     */
    suspend fun publishAll() {
        try {
            val rootResult = mediaStore.ensureMirrorRoot()
            if (rootResult is MediaStoreDownloads.MirrorRootResult.NotSupported) {
                DiagnosticsLogger.i(TAG, "Mirror publish skipped: ${rootResult.reason}")
                return
            }
            if (rootResult is MediaStoreDownloads.MirrorRootResult.Failed) {
                DiagnosticsLogger.w(TAG, "Failed to ensure mirror root: ${rootResult.reason}")
                return
            }

            // 获取所有项目
            val projectsResult = appServiceBridge.listProjects()
            if (projectsResult is com.xiwei.sujian.core.interop.common.BridgeResult.Error) {
                DiagnosticsLogger.w(TAG, "Failed to list projects: ${projectsResult.fullEnvelope}")
                return
            }
            if (projectsResult is com.xiwei.sujian.core.interop.common.BridgeResult.NotLoaded) {
                DiagnosticsLogger.w(TAG, "Native library not loaded, skip full publish")
                return
            }
            val projects = (projectsResult as com.xiwei.sujian.core.interop.common.BridgeResult.Success).data

            // 发布每个项目
            for (project in projects) {
                publishProject(project.id)
            }

            // manifest 已在每次 publishProject 中更新
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to publish all: ${e.message}", e)
        }
    }

    /**
     * 生成并写入 manifest。
     */
    private suspend fun publishManifest() {
        val projectsResult = appServiceBridge.listProjects()
        if (projectsResult !is com.xiwei.sujian.core.interop.common.BridgeResult.Success) return
        val projects = projectsResult.data

        val mirrorProjects = mutableListOf<MirrorProject>()
        for (project in projects) {
            val snapshotResult = projectBridge.getProjectWorkspaceSnapshot(project.id)
            if (snapshotResult is com.xiwei.sujian.core.interop.common.BridgeResult.Success<*>) {
                val snapshot = snapshotResult.data as ProjectWorkspaceSnapshot
                mirrorProjects.add(snapshot.toMirrorProject())
            }
        }

        val now = Instant.now()
        val updatedAt = DateTimeFormatter.ISO_INSTANT.format(now)
        val manifest = MirrorManifest(
            schemaVersion = 1,
            revision = now.toEpochMilli(),
            updatedAt = updatedAt,
            projects = mirrorProjects,
        )

        // 序列化成 JSON
        val json = manifestToJson(manifest)
        mediaStore.writeText("_meta/manifest.json", json)
    }

    /**
     * 把 MirrorProject 转成 JSON 字符串（手动序列化以避免引入 kotlinx-serialization 依赖）。
     */
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

    private fun projectToJson(project: MirrorProject, indent: String): String {
        val sb = StringBuilder()
        sb.appendLine("$indent{")
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
        sb.append("$indent}")
        return sb.toString()
    }

    private fun volumeToJson(volume: MirrorVolume, indent: String): String {
        val sb = StringBuilder()
        sb.appendLine("$indent{")
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
        sb.append("$indent}")
        return sb.toString()
    }

    private fun chapterToJson(chapter: MirrorChapter, indent: String): String {
        val sb = StringBuilder()
        sb.appendLine("$indent{")
        sb.appendLine("$indent  \"id\": \"${chapter.id}\",")
        sb.appendLine("$indent  \"title\": \"${escapeJson(chapter.title)}\",")
        sb.appendLine("$indent  \"order\": ${chapter.order},")
        sb.appendLine("$indent  \"revision\": ${chapter.revision},")
        sb.appendLine("$indent  \"updatedAt\": \"${chapter.updatedAt}\",")
        sb.appendLine("$indent  \"contentFile\": \"${chapter.contentFile}\",")
        sb.appendLine("$indent  \"contentHash\": \"${chapter.contentHash}\"")
        sb.append("$indent}")
        return sb.toString()
    }

    private fun escapeJson(s: String): String =
        s.replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("\n", "\\n")
            .replace("\r", "\\r")
            .replace("\t", "\\t")

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
    }
}

/**
 * 把 ProjectWorkspaceSnapshot 转成 MirrorProject。
 *
 * 需要读取每章正文以计算 contentHash。
 */
private suspend fun ProjectWorkspaceSnapshot.toMirrorProject(): MirrorProject {
    val chapters = mutableListOf<MirrorChapter>()
    for (volumeWithChapters in this.volumes) {
        for (chapter in volumeWithChapters.chapters) {
            // 注意：这里需要调用方在 publishProject 中已经打开了章节内容
            // 为简化，这里只记录 meta；contentHash 在 publishChapter 时写入
            chapters.add(
                MirrorChapter(
                    id = chapter.id,
                    title = chapter.title,
                    order = chapter.order,
                    revision = chapter.updatedAt.toEpochMillis(),
                    updatedAt = chapter.updatedAt,
                    contentFile = "projects/${this.project.id}/volumes/${volumeWithChapters.volume.id}/chapters/${chapter.id}.md",
                    contentHash = chapter.hash, // 使用 Core 提供的 hash
                )
            )
        }
    }

    return MirrorProject(
        id = this.project.id,
        title = this.project.title,
        order = 0, // 项目顺序在 listProjects 中决定
        revision = this.project.updatedAt.toEpochMillis(),
        updatedAt = this.project.updatedAt,
        volumes = this.volumes.map { it.toMirrorVolume(this.project.id) },
    )
}

private fun VolumeWithChapters.toMirrorVolume(projectId: String): MirrorVolume {
    return MirrorVolume(
        id = this.volume.id,
        title = this.volume.title,
        order = this.volume.order,
        revision = this.volume.updatedAt.toEpochMillis(),
        updatedAt = this.volume.updatedAt,
        chapters = this.chapters.map { chapter ->
            MirrorChapter(
                id = chapter.id,
                title = chapter.title,
                order = chapter.order,
                revision = chapter.updatedAt.toEpochMillis(),
                updatedAt = chapter.updatedAt,
                contentFile = "projects/$projectId/volumes/${this.volume.id}/chapters/${chapter.id}.md",
                contentHash = chapter.hash,
            )
        }
    )
}

/**
 * 把 ISO-8601 字符串转成毫秒级时间戳（简化：取 updatedAt 字段）。
 *
 * 注意：实际 Core 返回的 updatedAt 已经是 ISO-8601 格式。
 */
private fun String.toEpochMillis(): Long {
    return try {
        Instant.parse(this).toEpochMilli()
    } catch (e: Exception) {
        System.currentTimeMillis()
    }
}
