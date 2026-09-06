package com.xiwei.sujian.storage.mirror

import android.content.Context
import org.json.JSONException
import org.json.JSONObject
import java.io.File
import java.io.IOException

/**
 * 章节在镜像中的唯一定位键。
 *
 * `volumeId/chapterId` 在同一作品内唯一；`projectId` 跨作品隔离。
 */
data class ChapterKey(
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
)

/**
 * ReadableMirrorStateStore — 镜像发布状态的持久化存储。
 *
 * #649 评论 5560971132 修复 2/6：[ReadableMirrorPublisher] 需要在删除项目/章节后
 * 删除旧的 `.md` 文件。但 Publisher 只能通过 [MirrorSnapshotSource] 读 Core 当前
 * 快照，无法知道"上一次发布了哪些 URI"。本类持久化每个章节对应的 MediaStore URI、
 * 相对路径、revision 和 contentHash，让 Publisher 能做集合差删除。
 *
 * ## 存储位置
 * `context.noBackupFilesDir/sujian-mirror/state.json`
 *
 * `noBackupFilesDir` 是应用私有目录，卸载后清除，不被自动备份（避免恢复时残留
 * 指向已不存在 URI 的旧状态）。
 *
 * ## 线程安全
 * 所有公开方法用 [lock] 保护，保证多线程读写原子。文件 I/O 在锁内同步执行
 * （调用方在 IO 调度器上调用）。
 *
 * ## JSON 结构
 * ```json
 * {
 *   "projects": {
 *     "<projectId>": {
 *       "<volumeId>/<chapterId>": {
 *         "uri": "content://media/external/downloads/123",
 *         "relativePath": "作品/作品名/卷名/章节名.md",
 *         "revision": 1694123456789,
 *         "contentHash": "sha256:..."
 *       }
 *     }
 *   }
 * }
 * ```
 *
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，只依赖 Android `Context` 与 `org.json`，
 *   不依赖 Compose/UniFFI/业务 Repository。
 */
class ReadableMirrorStateStore(
    private val context: Context,
) {
    /** 单章镜像条目。 */
    data class ChapterMirrorEntry(
        /** MediaStore URI（`content://media/external/downloads/<id>`）。 */
        val uri: String,
        /** 相对 `Download/Sujian/` 的路径，如 `作品/作品名/卷名/章节名.md`。 */
        val relativePath: String,
        /** Core 的章节 revision（updatedAt 毫秒）。 */
        val revision: Long,
        /** 正文 SHA-256 哈希（`sha256:<hex>`），与 manifest 一致。 */
        val contentHash: String,
    )

    private val lock = Any()

    private val stateFile: File by lazy {
        File(File(context.noBackupFilesDir, STATE_DIR_NAME), STATE_FILE_NAME).also { file ->
            file.parentFile?.mkdirs()
        }
    }

    /** 获取某作品下全部章节条目。 */
    fun getProjectEntries(projectId: String): Map<ChapterKey, ChapterMirrorEntry> {
        synchronized(lock) {
            val root = readRoot() ?: return emptyMap()
            val projectObj = root.optJSONObject(PROJECTS_KEY)?.optJSONObject(projectId) ?: return emptyMap()
            return decodeProjectEntries(projectId, projectObj)
        }
    }

    /** 写入/覆盖单章条目。 */
    fun putChapterEntry(
        projectId: String,
        volumeId: String,
        chapterId: String,
        entry: ChapterMirrorEntry,
    ) {
        synchronized(lock) {
            val root = readRoot() ?: JSONObject()
            val projects = root.optJSONObject(PROJECTS_KEY) ?: JSONObject().also { root.put(PROJECTS_KEY, it) }
            val projectObj = projects.optJSONObject(projectId) ?: JSONObject().also { projects.put(projectId, it) }
            projectObj.put(chapterKey(volumeId, chapterId), encodeEntry(entry))
            writeRoot(root)
        }
    }

    /** 删除单章条目（幂等）。 */
    fun removeChapterEntry(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        synchronized(lock) {
            val root = readRoot() ?: return
            val projects = root.optJSONObject(PROJECTS_KEY) ?: return
            val projectObj = projects.optJSONObject(projectId) ?: return
            projectObj.remove(chapterKey(volumeId, chapterId))
            if (projectObj.length() == 0) {
                projects.remove(projectId)
            }
            if (projects.length() == 0) {
                root.remove(PROJECTS_KEY)
            }
            writeRoot(root)
        }
    }

    /**
     * 删除某作品的全部条目，返回被删除的条目（供 Publisher 逐个删 MediaStore URI）。
     */
    fun removeAllProjectEntries(projectId: String): Map<ChapterKey, ChapterMirrorEntry> {
        synchronized(lock) {
            val root = readRoot() ?: return emptyMap()
            val projects = root.optJSONObject(PROJECTS_KEY) ?: return emptyMap()
            val projectObj = projects.optJSONObject(projectId) ?: return emptyMap()
            val removed = decodeProjectEntries(projectId, projectObj)
            projects.remove(projectId)
            if (projects.length() == 0) {
                root.remove(PROJECTS_KEY)
            }
            writeRoot(root)
            return removed
        }
    }

    /** 列出所有有镜像条目的作品 ID。 */
    fun getAllProjectIds(): Set<String> {
        synchronized(lock) {
            val root = readRoot() ?: return emptySet()
            val projects = root.optJSONObject(PROJECTS_KEY) ?: return emptySet()
            val ids = mutableSetOf<String>()
            val keys = projects.keys()
            while (keys.hasNext()) {
                ids.add(keys.next())
            }
            return ids
        }
    }

    // ── manifest URI 存取 ──

    /** 获取 manifest 文件的 MediaStore URI（供 Publisher 覆盖写入）。 */
    fun getManifestUri(): String? {
        synchronized(lock) {
            val root = readRoot() ?: return null
            return root.optString(MANIFEST_URI_KEY).takeIf { it.isNotEmpty() }
        }
    }

    /** 记入 manifest 文件 URI。 */
    fun setManifestUri(uri: String) {
        synchronized(lock) {
            val root = readRoot() ?: JSONObject()
            root.put(MANIFEST_URI_KEY, uri)
            writeRoot(root)
        }
    }

    /**
     * #649 评论 5561286861 第 4 点：恢复成功后保存完整状态。
     *
     * 用用户刚选中的 tree URI 把：
     * - `_meta/manifest.json` 的 URI
     * - 每个 `contentFile` 对应的现有文档 URI
     * 写进 [ReadableMirrorStateStore]，供后续 Publisher 做集合差删除。
     *
     * @param manifestUri manifest 文件的 MediaStore URI
     * @param chapterEntries 所有章节的条目（包含 URI、相对路径、revision、contentHash）
     */
    fun saveRestoredState(
        manifestUri: String,
        chapterEntries: Map<ChapterKey, ChapterMirrorEntry>,
    ) {
        synchronized(lock) {
            val root = readRoot() ?: JSONObject()
            // 写入 manifest URI
            root.put(MANIFEST_URI_KEY, manifestUri)
            // 写入所有章节条目
            val projects = root.optJSONObject(PROJECTS_KEY) ?: JSONObject().also { root.put(PROJECTS_KEY, it) }
            for ((key, entry) in chapterEntries) {
                val projectObj = projects.optJSONObject(key.projectId) ?: JSONObject().also { projects.put(key.projectId, it) }
                projectObj.put(chapterKey(key.volumeId, key.chapterId), encodeEntry(entry))
            }
            writeRoot(root)
        }
    }

    // ── 内部 ──

    private fun chapterKey(
        volumeId: String,
        chapterId: String,
    ): String = "$volumeId/$chapterId"

    private fun decodeProjectEntries(
        projectId: String,
        projectObj: JSONObject,
    ): Map<ChapterKey, ChapterMirrorEntry> {
        val result = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        val keys = projectObj.keys()
        while (keys.hasNext()) {
            val key = keys.next()
            val parts = key.split("/", limit = 2)
            if (parts.size != 2) continue
            val entryObj = projectObj.optJSONObject(key) ?: continue
            val entry = decodeEntry(entryObj) ?: continue
            result[ChapterKey(projectId, parts[0], parts[1])] = entry
        }
        return result
    }

    private fun decodeEntry(obj: JSONObject): ChapterMirrorEntry? {
        val uri = obj.optString(URI_KEY)
        if (uri.isEmpty()) return null
        return ChapterMirrorEntry(
            uri = uri,
            relativePath = obj.optString(RELATIVE_PATH_KEY),
            revision = obj.optLong(REVISION_KEY),
            contentHash = obj.optString(CONTENT_HASH_KEY),
        )
    }

    private fun encodeEntry(entry: ChapterMirrorEntry): JSONObject =
        JSONObject().apply {
            put(URI_KEY, entry.uri)
            put(RELATIVE_PATH_KEY, entry.relativePath)
            put(REVISION_KEY, entry.revision)
            put(CONTENT_HASH_KEY, entry.contentHash)
        }

    private fun readRoot(): JSONObject? {
        if (!stateFile.exists()) return null
        return try {
            JSONObject(stateFile.readText(Charsets.UTF_8))
        } catch (e: IOException) {
            null
        } catch (e: JSONException) {
            null
        }
    }

    private fun writeRoot(root: JSONObject) {
        try {
            stateFile.parentFile?.mkdirs()
            stateFile.writeText(root.toString(), Charsets.UTF_8)
        } catch (e: IOException) {
            // 状态写入失败不阻断业务；下次发布会重写。
        }
    }

    companion object {
        private const val STATE_DIR_NAME = "sujian-mirror"
        private const val STATE_FILE_NAME = "state.json"
        private const val PROJECTS_KEY = "projects"
        private const val MANIFEST_URI_KEY = "manifestUri"
        private const val URI_KEY = "uri"
        private const val RELATIVE_PATH_KEY = "relativePath"
        private const val REVISION_KEY = "revision"
        private const val CONTENT_HASH_KEY = "contentHash"
    }
}
