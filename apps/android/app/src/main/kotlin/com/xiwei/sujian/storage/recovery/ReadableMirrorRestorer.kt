package com.xiwei.sujian.storage.recovery

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import com.xiwei.sujian.storage.mirror.MirrorChapter
import com.xiwei.sujian.storage.mirror.MirrorManifest
import com.xiwei.sujian.storage.mirror.MirrorProject
import com.xiwei.sujian.storage.mirror.MirrorVolume
import com.xiwei.sujian.storage.mirror.computeContentHash
import com.xiwei.sujian.storage.mirror.verifyContentHash
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.IOException

/**
 * 镜像恢复结果。
 */
sealed class RestoreResult {
    /** 恢复成功。 */
    object Success : RestoreResult()

    /** 未找到 `_meta/manifest.json`。 */
    object ManifestMissing : RestoreResult()

    /** 恢复过程中失败。 */
    data class RestoreFailed(val reason: String) : RestoreResult()
}

/**
 * ReadableMirrorRestorer — Download/Sujian 镜像恢复器。
 *
 * 处理新的 `Download/Sujian` 镜像恢复：
 * 1. 读取 `_meta/manifest.json`（用 [DocumentTreeReader.readText]，解析 JSON）。
 * 2. 按 manifest 的 ID、标题和顺序通过现有 Core API（[AppServiceBridge]）创建
 *    project / volume / chapter。
 * 3. 逐章读取 `.md` 并调用 Core 的 `saveChapterContent()`。
 * 4. 恢复完成后调用 [RecoveryChangeSink.everythingChanged]。
 *
 * 不把 Download 目录直接复制进 `filesDir/sujian`（镜像格式是给人看的，不是 Core 磁盘格式）。
 * 不把 `content://` URI 传给 Rust——只把 SAF 文档读成内存文本再交给 Core API。
 *
 * #649 评论 5560685734 要求 4：使用共享的 [MirrorManifest] 格式，
 * 包含 schemaVersion、revision、contentHash；恢复时校验正文完整性。
 */
class ReadableMirrorRestorer {
    suspend fun restore(
        context: Context,
        mirrorTreeUri: Uri,
        documentTreeReader: DocumentTreeReader,
        appServiceBridge: AppServiceBridge,
        changeSink: RecoveryChangeSink,
    ): RestoreResult =
        withContext(Dispatchers.IO) {
            // 健壮性检查：根 URI 仍可访问（权限未丢失）
            verifyUriAccessible(context, mirrorTreeUri)?.let { return@withContext it }

            // 1. 读取并解析 manifest
            val manifestUri =
                findDescendant(mirrorTreeUri, MANIFEST_PATH, documentTreeReader)
                    ?: return@withContext RestoreResult.ManifestMissing
            val manifest =
                try {
                    parseManifest(documentTreeReader.readText(manifestUri))
                } catch (e: Exception) {
                    return@withContext RestoreResult.RestoreFailed("Failed to read/parse manifest: ${e.message}")
                }

            // 1.1 校验 manifest 引用的正文文件是否都存在
            val missingFiles = mutableListOf<String>()
            for (project in manifest.projects) {
                for (volume in project.volumes) {
                    for (chapter in volume.chapters) {
                        if (chapter.contentFile.isEmpty()) {
                            missingFiles.add(chapter.contentFile)
                            continue
                        }
                        val contentUri = findDescendant(mirrorTreeUri, chapter.contentFile, documentTreeReader)
                        if (contentUri == null) {
                            missingFiles.add(chapter.contentFile)
                        }
                    }
                }
            }
            if (missingFiles.isNotEmpty()) {
                return@withContext RestoreResult.RestoreFailed("Missing content files: ${missingFiles.joinToString(", ")}")
            }

            // 2-3. 按 manifest 重建 project / volume / chapter + 逐章读取内容
            try {
                for (project in manifest.projects) {
                    restoreProject(project, mirrorTreeUri, documentTreeReader, appServiceBridge)
                }
            } catch (e: Exception) {
                return@withContext RestoreResult.RestoreFailed(e.message ?: "Unknown restore error")
            }

            // 4. 通知所有组件刷新
            changeSink.everythingChanged()
            RestoreResult.Success
        }

    /** 验证根 URI 仍可查询；返回 null 表示通过，否则返回失败结果。 */
    private fun verifyUriAccessible(
        context: Context,
        uri: Uri,
    ): RestoreResult? {
        val cursor =
            try {
                context.contentResolver.query(
                    uri,
                    arrayOf(DocumentsContract.Document.COLUMN_MIME_TYPE),
                    null,
                    null,
                    null,
                )
            } catch (e: Exception) {
                return RestoreResult.RestoreFailed("Failed to verify mirror tree URI: ${e.message}")
            }
        if (cursor == null) {
            return RestoreResult.RestoreFailed("Mirror tree URI not queryable")
        }
        return cursor.use { c ->
            if (!c.moveToFirst()) {
                RestoreResult.RestoreFailed("Mirror tree URI no longer accessible")
            } else {
                null
            }
        }
    }

    private fun restoreProject(
        project: MirrorProject,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
        bridge: AppServiceBridge,
    ) {
        val newProject = bridge.createProject(project.title).unwrapOrThrow()
        val sortedVolumes = project.volumes.sortedBy { it.order }
        val newVolumeIds = mutableListOf<String>()
        for (volume in sortedVolumes) {
            val newVolume = bridge.createVolume(newProject.id, volume.title).unwrapOrThrow()
            newVolumeIds.add(newVolume.id)
            restoreVolumeChapters(newProject.id, newVolume.id, volume, mirrorTreeUri, reader, bridge)
        }
        if (newVolumeIds.size > 1) {
            bridge.reorderVolumes(newProject.id, newVolumeIds).unwrapOrThrow()
        }
    }

    private fun restoreVolumeChapters(
        projectId: String,
        volumeId: String,
        volume: MirrorVolume,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
        bridge: AppServiceBridge,
    ) {
        val sortedChapters = volume.chapters.sortedBy { it.order }
        val newChapterIds = mutableListOf<String>()
        for (chapter in sortedChapters) {
            val newChapter = bridge.createChapter(projectId, volumeId, chapter.title).unwrapOrThrow()
            newChapterIds.add(newChapter.id)
            val content = readChapterContent(chapter, mirrorTreeUri, reader)
            if (content != null) {
                bridge.saveChapterContent(projectId, volumeId, newChapter.id, content).unwrapOrThrow()
            }
        }
        if (newChapterIds.size > 1) {
            bridge.reorderChapters(projectId, volumeId, newChapterIds).unwrapOrThrow()
        }
    }

    /**
     * 读取章节内容；返回 null 表示读取失败（阻断恢复，因为 manifest 已声明该文件存在）。
     * 空正文返回空字符串（正常恢复）。
     */
    private fun readChapterContent(
        chapter: MirrorChapter,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
    ): String? {
        if (chapter.contentFile.isEmpty()) return ""
        val contentUri = findDescendant(mirrorTreeUri, chapter.contentFile, reader) ?: return null
        return runCatching { reader.readText(contentUri) }
            .getOrNull()
            ?.let { content ->
                // 校验 contentHash
                if (chapter.contentHash.isNotEmpty() && !verifyContentHash(content, chapter.contentHash)) {
                    null // 哈希不匹配
                } else {
                    content
                }
            }
    }

    /**
     * 按 [relativePath]（以 `/` 分隔）逐级在 [tree] 下定位子文档。
     * 返回 null 表示路径不存在。
     */
    private fun findDescendant(
        tree: Uri,
        relativePath: String,
        reader: DocumentTreeReader,
    ): Uri? {
        val parts = relativePath.split("/").filter { it.isNotEmpty() }
        var current = tree
        for (part in parts) {
            val children = reader.listChildren(current)
            val match = children.find { it.name == part } ?: return null
            current = match.uri
        }
        return current
    }

    /**
     * 解析 manifest JSON。
     *
     * #649 评论 5560685734：使用共享的 [MirrorManifest] 格式，
     * 包含 schemaVersion、revision、contentHash。
     */
    private fun parseManifest(json: String): MirrorManifest {
        val root = JSONObject(json)
        val schemaVersion = root.optInt(SCHEMA_VERSION_KEY, 1)
        val revision = root.optLong(REVISION_KEY)
        val updatedAt = root.optString(UPDATED_AT_KEY)
        val projects = mutableListOf<MirrorProject>()
        val projectsArr = root.optJSONArray(PROJECTS_KEY)
        if (projectsArr != null) {
            for (i in 0 until projectsArr.length()) {
                projects.add(parseProject(projectsArr.getJSONObject(i)))
            }
        }
        return MirrorManifest(
            schemaVersion = schemaVersion,
            revision = revision,
            updatedAt = updatedAt,
            projects = projects,
        )
    }

    private fun parseProject(obj: JSONObject): MirrorProject {
        val volumes = mutableListOf<MirrorVolume>()
        val volumesArr = obj.optJSONArray(VOLUMES_KEY)
        if (volumesArr != null) {
            for (i in 0 until volumesArr.length()) {
                volumes.add(parseVolume(volumesArr.getJSONObject(i)))
            }
        }
        return MirrorProject(
            id = obj.optString(ID_KEY),
            title = obj.optString(TITLE_KEY),
            order = obj.optInt(ORDER_KEY, 0),
            revision = obj.optLong(REVISION_KEY),
            updatedAt = obj.optString(UPDATED_AT_KEY),
            volumes = volumes,
        )
    }

    private fun parseVolume(obj: JSONObject): MirrorVolume {
        val chapters = mutableListOf<MirrorChapter>()
        val chaptersArr = obj.optJSONArray(CHAPTERS_KEY)
        if (chaptersArr != null) {
            for (i in 0 until chaptersArr.length()) {
                chapters.add(parseChapter(chaptersArr.getJSONObject(i)))
            }
        }
        return MirrorVolume(
            id = obj.optString(ID_KEY),
            title = obj.optString(TITLE_KEY),
            order = obj.optInt(ORDER_KEY, 0),
            revision = obj.optLong(REVISION_KEY),
            updatedAt = obj.optString(UPDATED_AT_KEY),
            chapters = chapters,
        )
    }

    private fun parseChapter(obj: JSONObject): MirrorChapter =
        MirrorChapter(
            id = obj.optString(ID_KEY),
            title = obj.optString(TITLE_KEY),
            order = obj.optInt(ORDER_KEY, 0),
            revision = obj.optLong(REVISION_KEY),
            updatedAt = obj.optString(UPDATED_AT_KEY),
            contentFile = obj.optString(CONTENT_FILE_KEY),
            contentHash = obj.optString(CONTENT_HASH_KEY),
        )

    private fun <T> BridgeResult<T>.unwrapOrThrow(): T =
        when (this) {
            is BridgeResult.Success -> data
            is BridgeResult.Error -> throw IOException(
                "Bridge error: ${envelope.errorCode} - ${envelope.rawError}",
            )
            BridgeResult.NotLoaded -> throw IOException("Native library not loaded")
        }

    companion object {
        private const val MANIFEST_PATH = "_meta/manifest.json"
        private const val SCHEMA_VERSION_KEY = "schemaVersion"
        private const val REVISION_KEY = "revision"
        private const val UPDATED_AT_KEY = "updatedAt"
        private const val PROJECTS_KEY = "projects"
        private const val VOLUMES_KEY = "volumes"
        private const val CHAPTERS_KEY = "chapters"
        private const val ID_KEY = "id"
        private const val TITLE_KEY = "title"
        private const val ORDER_KEY = "order"
        private const val CONTENT_FILE_KEY = "contentFile"
        private const val CONTENT_HASH_KEY = "contentHash"
    }
}
