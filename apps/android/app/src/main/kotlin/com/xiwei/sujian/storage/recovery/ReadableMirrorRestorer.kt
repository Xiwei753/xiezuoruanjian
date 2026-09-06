package com.xiwei.sujian.storage.recovery

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import com.xiwei.sujian.storage.mirror.ChapterKey
import com.xiwei.sujian.storage.mirror.ChapterMirrorEntry
import com.xiwei.sujian.storage.mirror.MirrorChapter
import com.xiwei.sujian.storage.mirror.MirrorManifest
import com.xiwei.sujian.storage.mirror.MirrorProject
import com.xiwei.sujian.storage.mirror.MirrorVolume
import com.xiwei.sujian.storage.mirror.ReadableMirrorStateStore
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
    /**
     * #649 评论 5561286861 第 4 点：恢复专用写入接口（不带 MirrorChangeSink）。
     *
     * 恢复时使用 AppServiceBridge 的 recovery* 方法，避免触发镜像发布。
     * 恢复完成后保存状态到 [ReadableMirrorStateStore]，供后续 Publisher 做集合差删除。
     */
    suspend fun restore(
        context: Context,
        mirrorTreeUri: Uri,
        documentTreeReader: DocumentTreeReader,
        appServiceBridge: AppServiceBridge,
        stateStore: ReadableMirrorStateStore,
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

            // 1.1 预检查：创建 Core 项目前，对所有章节做"存在 + 可读 + hash"预检。
            // #649 评论 5560971132 修复 8：任一章节读取失败或 hash 不匹配立即返回 RestoreFailed，
            // 不创建半成品 Core 项目。扩展现有 missingFiles 检查，同时预读验证 hash。
            val precheckFailure = precheckAllChapters(manifest, mirrorTreeUri, documentTreeReader)
            if (precheckFailure != null) {
                return@withContext precheckFailure
            }

            // 2-3. 按 manifest 重建 project / volume / chapter + 逐章读取内容
            // #649 评论 5561286861：使用恢复专用写入接口，不触发 MirrorChangeSink
            val allChapterEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
            try {
                for (project in manifest.projects) {
                    val projectEntries = restoreProject(project, mirrorTreeUri, documentTreeReader, appServiceBridge)
                    allChapterEntries.putAll(projectEntries)
                }
            } catch (e: Exception) {
                return@withContext RestoreResult.RestoreFailed(e.message ?: "Unknown restore error")
            }

            // 4. 保存恢复后的状态到 ReadableMirrorStateStore
            val manifestUriString = manifestUri.toString()
            stateStore.saveRestoredState(manifestUriString, allChapterEntries)

            // 5. 通知所有组件刷新
            changeSink.everythingChanged()
            RestoreResult.Success
        }

    /**
     * 预检查所有章节：存在 + 可读 + hash 匹配。
     *
     * 返回 null 表示全部通过；否则返回 [RestoreResult.RestoreFailed]。
     * 在创建任何 Core 项目之前调用，避免半成品。
     */
    private fun precheckAllChapters(
        manifest: MirrorManifest,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
    ): RestoreResult? {
        for (project in manifest.projects) {
            for (volume in project.volumes) {
                val failure = precheckVolumeChapters(volume, mirrorTreeUri, reader)
                if (failure != null) return failure
            }
        }
        return null
    }

    /**
     * 预检查单卷所有章节。提取自 precheckAllChapters 以控制嵌套深度与认知复杂度。
     */
    private fun precheckVolumeChapters(
        volume: MirrorVolume,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
    ): RestoreResult? {
        for (chapter in volume.chapters) {
            if (chapter.contentFile.isEmpty()) {
                return RestoreResult.RestoreFailed(
                    "Chapter ${chapter.id} has empty contentFile in manifest",
                )
            }
            val contentUri =
                findDescendant(mirrorTreeUri, chapter.contentFile, reader)
                    ?: return RestoreResult.RestoreFailed(
                        "Missing content file: ${chapter.contentFile}",
                    )
            val content =
                try {
                    reader.readText(contentUri)
                } catch (e: Exception) {
                    return RestoreResult.RestoreFailed(
                        "Failed to read ${chapter.contentFile}: ${e.message}",
                    )
                }
            // hash 预检：manifest 声明了 hash 时必须匹配
            if (chapter.contentHash.isNotEmpty() && !verifyContentHash(content, chapter.contentHash)) {
                return RestoreResult.RestoreFailed(
                    "Content hash mismatch for ${chapter.contentFile}",
                )
            }
        }
        return null
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

    /**
     * 恢复单个项目，返回该项目所有章节的条目映射。
     *
     * #649 评论 5561286861：使用恢复专用写入接口，不触发 MirrorChangeSink。
     */
    private fun restoreProject(
        project: MirrorProject,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
        bridge: AppServiceBridge,
    ): Map<ChapterKey, ChapterMirrorEntry> {
        val newProject = bridge.recoveryCreateProject(project.title).unwrapOrThrow()
        val sortedVolumes = project.volumes.sortedBy { it.order }
        val newVolumeIds = mutableListOf<String>()
        val allChapterEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        for (volume in sortedVolumes) {
            val newVolume = bridge.recoveryCreateVolume(newProject.id, volume.title).unwrapOrThrow()
            newVolumeIds.add(newVolume.id)
            val volumeEntries = restoreVolumeChapters(newProject.id, newVolume.id, volume, mirrorTreeUri, reader, bridge)
            allChapterEntries.putAll(volumeEntries)
        }
        if (newVolumeIds.size > 1) {
            bridge.recoveryReorderVolumes(newProject.id, newVolumeIds).unwrapOrThrow()
        }
        return allChapterEntries
    }

    /**
     * 恢复单个卷的所有章节，返回该卷所有章节的条目映射。
     *
     * #649 评论 5561286861：使用恢复专用写入接口，不触发 MirrorChangeSink。
     */
    private fun restoreVolumeChapters(
        projectId: String,
        volumeId: String,
        volume: MirrorVolume,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
        bridge: AppServiceBridge,
    ): Map<ChapterKey, ChapterMirrorEntry> {
        val sortedChapters = volume.chapters.sortedBy { it.order }
        val newChapterIds = mutableListOf<String>()
        val chapterEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        for (chapter in sortedChapters) {
            val newChapter = bridge.recoveryCreateChapter(projectId, volumeId, chapter.title).unwrapOrThrow()
            newChapterIds.add(newChapter.id)
            // #649 修复 8：失败直接 throw IOException，不返回 null。
            // 预检查已保证可读 + hash 匹配，此处再读一次做双重保险。
            val content = readVerifiedChapterContent(chapter, mirrorTreeUri, reader)
            bridge.recoverySaveChapterContent(projectId, volumeId, newChapter.id, content).unwrapOrThrow()
            // 记录章节条目（用于保存到 stateStore）
            val contentUri = findDescendant(mirrorTreeUri, chapter.contentFile, reader)
            if (contentUri != null) {
                chapterEntries[ChapterKey(projectId, volumeId, newChapter.id)] = ChapterMirrorEntry(
                    uri = contentUri.toString(),
                    relativePath = chapter.contentFile,
                    revision = chapter.revision,
                    contentHash = chapter.contentHash,
                )
            }
        }
        if (newChapterIds.size > 1) {
            bridge.recoveryReorderChapters(projectId, volumeId, newChapterIds).unwrapOrThrow()
        }
        return chapterEntries
    }

    /**
     * 读取并验证章节内容；失败 throw [IOException]。
     *
     * #649 评论 5560971132 修复 8：旧 [readChapterContent] 返回 null 时调用方静默跳过，
     * 导致恢复出空章节。新实现失败直接 throw，让外层 try-catch 转成 RestoreFailed。
     *
     * - contentFile 为空 → throw（manifest 声明了该文件存在）。
     * - 文件不存在 → throw。
     * - 读取失败 → throw。
     * - hash 不匹配 → throw。
     * - 空正文返回空字符串（正常恢复）。
     */
    private fun readVerifiedChapterContent(
        chapter: MirrorChapter,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
    ): String {
        if (chapter.contentFile.isEmpty()) {
            throw IOException("Chapter ${chapter.id} has empty contentFile")
        }
        val contentUri =
            findDescendant(mirrorTreeUri, chapter.contentFile, reader)
                ?: throw IOException("Content file not found: ${chapter.contentFile}")
        val content =
            try {
                reader.readText(contentUri)
            } catch (e: Exception) {
                throw IOException("Failed to read ${chapter.contentFile}: ${e.message}", e)
            }
        if (chapter.contentHash.isNotEmpty() && !verifyContentHash(content, chapter.contentHash)) {
            throw IOException("Content hash mismatch for ${chapter.contentFile}")
        }
        return content
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
     * #649 评论 5560971132 修复 8：schemaVersion 严格校验。
     * - 用 [JSONObject.getInt]（不是 optInt）：字段缺失时 throw JSONException → RestoreFailed。
     * - 值 != 1 时 throw IOException → RestoreFailed（不支持的未来版本）。
     */
    private fun parseManifest(json: String): MirrorManifest {
        val root = JSONObject(json)
        val schemaVersion = root.getInt(SCHEMA_VERSION_KEY)
        if (schemaVersion != 1) {
            throw IOException("Unsupported manifest schemaVersion: $schemaVersion (expected 1)")
        }
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
