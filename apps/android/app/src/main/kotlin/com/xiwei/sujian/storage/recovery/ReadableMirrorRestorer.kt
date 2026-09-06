package com.xiwei.sujian.storage.recovery

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.IOException

/**
 * 镜像 manifest schema — Download/Sujian 镜像的恢复清单。
 *
 * 由 [ReadableMirrorRestorer] 解析 `_meta/manifest.json` 得到，按 ID、标题和顺序
 * 通过 Core API 重建 project / volume / chapter。manifest 里的旧 ID 仅用于定位
 * contentFile 路径，不传给 Core（Core 自动生成新 ID）。
 */
data class MirrorManifest(val projects: List<MirrorProject>)

data class MirrorProject(
    val id: String,
    val title: String,
    val volumes: List<MirrorVolume>,
)

data class MirrorVolume(
    val id: String,
    val title: String,
    val order: Int,
    val chapters: List<MirrorChapter>,
)

data class MirrorChapter(
    val id: String,
    val title: String,
    val order: Int,
    /** 内容文件相对路径，如 `projects/<id>/volumes/<vid>/chapters/<cid>.md`。 */
    val contentFile: String,
)

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
 * 4. 恢复完成后调用 [MirrorChangeSink.everythingChanged]。
 *
 * 不把 Download 目录直接复制进 `filesDir/sujian`（镜像格式是给人看的，不是 Core 磁盘格式）。
 * 不把 `content://` URI 传给 Rust——只把 SAF 文档读成内存文本再交给 Core API。
 */
class ReadableMirrorRestorer {
    suspend fun restore(
        context: Context,
        mirrorTreeUri: Uri,
        documentTreeReader: DocumentTreeReader,
        appServiceBridge: AppServiceBridge,
        changeSink: MirrorChangeSink,
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

    /** 读取章节内容；返回 null 表示无内容或读取失败（跳过，不阻断恢复）。 */
    private fun readChapterContent(
        chapter: MirrorChapter,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
    ): String? {
        if (chapter.contentFile.isEmpty()) return null
        val contentUri = findDescendant(mirrorTreeUri, chapter.contentFile, reader) ?: return null
        return runCatching { reader.readText(contentUri) }
            .getOrNull()
            ?.takeIf { it.isNotEmpty() }
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

    private fun parseManifest(json: String): MirrorManifest {
        val root = JSONObject(json)
        val projects = mutableListOf<MirrorProject>()
        val projectsArr = root.optJSONArray(PROJECTS_KEY)
        if (projectsArr != null) {
            for (i in 0 until projectsArr.length()) {
                projects.add(parseProject(projectsArr.getJSONObject(i)))
            }
        }
        return MirrorManifest(projects)
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
            chapters = chapters,
        )
    }

    private fun parseChapter(obj: JSONObject): MirrorChapter =
        MirrorChapter(
            id = obj.optString(ID_KEY),
            title = obj.optString(TITLE_KEY),
            order = obj.optInt(ORDER_KEY, 0),
            contentFile = obj.optString(CONTENT_FILE_KEY),
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
        private const val PROJECTS_KEY = "projects"
        private const val VOLUMES_KEY = "volumes"
        private const val CHAPTERS_KEY = "chapters"
        private const val ID_KEY = "id"
        private const val TITLE_KEY = "title"
        private const val ORDER_KEY = "order"
        private const val CONTENT_FILE_KEY = "contentFile"
    }
}
