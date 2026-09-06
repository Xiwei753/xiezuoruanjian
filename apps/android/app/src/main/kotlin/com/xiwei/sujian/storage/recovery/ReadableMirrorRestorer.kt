package com.xiwei.sujian.storage.recovery

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import com.xiwei.sujian.storage.mirror.ChapterKey
import com.xiwei.sujian.storage.mirror.ChapterMirrorEntry
import com.xiwei.sujian.storage.mirror.MirrorBackend
import com.xiwei.sujian.storage.mirror.MirrorChapter
import com.xiwei.sujian.storage.mirror.MirrorManifest
import com.xiwei.sujian.storage.mirror.MirrorProject
import com.xiwei.sujian.storage.mirror.MirrorVolume
import com.xiwei.sujian.storage.mirror.ReadableMirrorStateStore
import com.xiwei.sujian.storage.mirror.verifyContentHash
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import uniffi.writer_core.RestoreChapterInputDto
import uniffi.writer_core.RestoreProjectInputDto
import uniffi.writer_core.RestoreVolumeInputDto
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
 * 2. 预读所有章节正文到内存，预检查 hash。
 * 3. 把 manifest + 预读正文组装成 [RestoreProjectInputDto]，对每个 project 调一次
 *    [AppServiceBridge.restoreProjectTree]（#649 评论 5561465552 第 2 点）。
 * 4. 恢复完成后调用 [RecoveryChangeSink.everythingChanged]。
 *
 * #649 评论 5561465552 第 2 点：不再逐层调 `recoveryCreateProject`/`recoveryCreateVolume`/
 * `recoveryCreateChapter`（丢弃 manifest 里的 ID，生成新 ID）。新实现一次跨 FFI 传入完整
 * 作品树，Core 负责校验 ID、校验目标不冲突、原子发布到 `projects/<projectId>`。
 *
 * #649 评论 5561465552 第 3 点：恢复成功后保存 `backend=document_tree` 和 `treeUri` 到
 * [ReadableMirrorStateStore]，后续 Publisher 通过 [com.xiwei.sujian.storage.mirror.selectMirrorStorage]
 * 选择 [com.xiwei.sujian.storage.mirror.DocumentTreeMirrorStorage]，直接更新用户刚选中的那棵
 * Download/Sujian，不再创建第二份。
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
     * 恢复时使用 [AppServiceBridge.restoreProjectTree]（不触发镜像发布）。
     * 恢复完成后保存状态到 [ReadableMirrorStateStore]，供后续 Publisher 做集合差删除。
     *
     * #649 评论 5561465552 第 3 点：保存 `backend=document_tree` 和 `treeUri`。
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

            // 1.1 预检查 + 预读所有章节正文到内存。
            // #649 评论 5560971132 修复 8：任一章节读取失败或 hash 不匹配立即返回 RestoreFailed，
            // 不创建半成品 Core 项目。
            // #649 评论 5561465552 第 2 点：预读正文到内存，组装 DTO 时直接用，不再逐章调 Core。
            val preloadedContents = mutableMapOf<String, String>() // key: "projectId/volumeId/chapterId"
            val precheckFailure =
                precheckAndPreloadAllChapters(
                    manifest,
                    mirrorTreeUri,
                    documentTreeReader,
                    preloadedContents,
                )
            if (precheckFailure != null) {
                return@withContext precheckFailure
            }

            // 2. 对每个 project 组装 RestoreProjectInputDto 并调一次 restoreProjectTree。
            // #649 评论 5561465552 第 2 点：保留 manifest 里的 project.id/volume.id/chapter.id，
            // 不再生成新 ID。
            val allChapterEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
            try {
                for (project in manifest.projects) {
                    val dto = buildRestoreProjectInputDto(project, preloadedContents)
                    val result = appServiceBridge.restoreProjectTree(dto).unwrapOrThrow()
                    // restore_project_tree 保留 manifest ID，所以这里用 manifest 的 ID 组装条目
                    val projectEntries = buildChapterEntries(project, mirrorTreeUri, documentTreeReader)
                    allChapterEntries.putAll(projectEntries)
                }
            } catch (e: Exception) {
                return@withContext RestoreResult.RestoreFailed(e.message ?: "Unknown restore error")
            }

            // 3. 保存恢复后的状态到 ReadableMirrorStateStore。
            // #649 评论 5561465552 第 3 点：backend=document_tree，treeUri=mirrorTreeUri。
            val manifestUriString = manifestUri.toString()
            stateStore.saveRestoredState(
                manifestUri = manifestUriString,
                chapterEntries = allChapterEntries,
                backend = MirrorBackend.DOCUMENT_TREE,
                treeUri = mirrorTreeUri.toString(),
            )

            // 4. 通知所有组件刷新
            changeSink.everythingChanged()
            RestoreResult.Success
        }

    /**
     * 预检查所有章节 + 预读正文到 [preloadedContents]。
     *
     * 返回 null 表示全部通过；否则返回 [RestoreResult.RestoreFailed]。
     * 在创建任何 Core 项目之前调用，避免半成品。
     *
     * @param preloadedContents 输出参数：预读的正文，key 为 `projectId/volumeId/chapterId`。
     */
    private fun precheckAndPreloadAllChapters(
        manifest: MirrorManifest,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
        preloadedContents: MutableMap<String, String>,
    ): RestoreResult? {
        for (project in manifest.projects) {
            for (volume in project.volumes) {
                val failure =
                    precheckAndPreloadVolumeChapters(
                        project.id,
                        volume,
                        mirrorTreeUri,
                        reader,
                        preloadedContents,
                    )
                if (failure != null) return failure
            }
        }
        return null
    }

    /**
     * 预检查 + 预读单卷所有章节。提取自 precheckAndPreloadAllChapters 以控制嵌套深度。
     */
    private fun precheckAndPreloadVolumeChapters(
        projectId: String,
        volume: MirrorVolume,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
        preloadedContents: MutableMap<String, String>,
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
            // 预读到内存，组装 DTO 时直接用
            preloadedContents["$projectId/${volume.id}/${chapter.id}"] = content
        }
        return null
    }

    /**
     * 把 manifest 的 [MirrorProject] + 预读正文组装成 [RestoreProjectInputDto]。
     *
     * #649 评论 5561465552 第 2 点：manifest 里的 id → DTO 的 id，order 直接传，
     * content 用预读的正文。
     */
    private fun buildRestoreProjectInputDto(
        project: MirrorProject,
        preloadedContents: Map<String, String>,
    ): RestoreProjectInputDto {
        val volumeDtos =
            project.volumes.sortedBy { it.order }.map { volume ->
                val chapterDtos =
                    volume.chapters.sortedBy { it.order }.map { chapter ->
                        val contentKey = "${project.id}/${volume.id}/${chapter.id}"
                        val content = preloadedContents[contentKey] ?: ""
                        RestoreChapterInputDto(
                            chapterId = chapter.id,
                            title = chapter.title,
                            order = chapter.order,
                            content = content,
                        )
                    }
                RestoreVolumeInputDto(
                    volumeId = volume.id,
                    title = volume.title,
                    order = volume.order,
                    chapters = chapterDtos,
                )
            }
        return RestoreProjectInputDto(
            projectId = project.id,
            title = project.title,
            order = project.order,
            volumes = volumeDtos,
        )
    }

    /**
     * 为已恢复的 [MirrorProject] 构建章节条目映射（用于保存到 stateStore）。
     *
     * #649 评论 5561465552 第 2 点：restore_project_tree 保留 manifest ID，
     * 所以这里用 manifest 的 project.id/volume.id/chapter.id 组装 ChapterKey
     * （不再用新生成的 ID）。
     */
    private fun buildChapterEntries(
        project: MirrorProject,
        mirrorTreeUri: Uri,
        reader: DocumentTreeReader,
    ): Map<ChapterKey, ChapterMirrorEntry> {
        val entries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        for (volume in project.volumes) {
            for (chapter in volume.chapters) {
                val contentUri = findDescendant(mirrorTreeUri, chapter.contentFile, reader)
                if (contentUri != null) {
                    entries[ChapterKey(project.id, volume.id, chapter.id)] =
                        ChapterMirrorEntry(
                            uri = contentUri.toString(),
                            relativePath = chapter.contentFile,
                            revision = chapter.revision,
                            contentHash = chapter.contentHash,
                        )
                }
            }
        }
        return entries
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
