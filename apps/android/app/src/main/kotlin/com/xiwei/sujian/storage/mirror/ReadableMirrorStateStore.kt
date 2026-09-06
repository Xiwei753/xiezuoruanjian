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
 * 单章镜像条目。
 *
 * #649 评论 5561465552 第 1 点：从 [ReadableMirrorStateStore] 的嵌套类提到
 * `storage/mirror` 包顶层，与 [ChapterKey] 并列。
 *
 * 旧代码同时存在两种引用方式：
 * - [ReadableMirrorRestorer] 用 `import com.xiwei.sujian.storage.mirror.ChapterMirrorEntry`
 *   （顶层引用，但顶层并不存在该类，编译靠嵌套类的 import 别名碰巧通过）。
 * - [ReadableMirrorPublisher] 用 `ReadableMirrorStateStore.ChapterMirrorEntry`（嵌套引用）。
 *
 * 提到顶层后两种引用统一为 `ChapterMirrorEntry`，消除歧义。
 *
 * @property uri MediaStore URI（`content://media/external/downloads/<id>`）或
 *   SAF document URI（`content://com.android.providers.../document/...`）。
 * @property relativePath 相对 `Download/Sujian/` 的路径，如 `作品/作品名/卷名/章节名.md`。
 * @property revision Core 的章节 revision（updatedAt 毫秒）。
 * @property contentHash 正文 SHA-256 哈希（`sha256:<hex>`），与 manifest 一致。
 */
data class ChapterMirrorEntry(
    val uri: String,
    val relativePath: String,
    val revision: Long,
    val contentHash: String,
)

/**
 * 镜像存储后端类型。
 *
 * #649 评论 5561465552 第 3 点：SAF/MediaStore URI 体系混用问题。
 * Publisher/Restorer 需要知道当前镜像写到了哪套 URI 体系，后续编辑才能
 * 走对的后端，不再创建第二份。
 *
 * - [MEDIA_STORE]：MediaStore.Downloads（API 29+），URI 形如 `content://media/external/downloads/<id>`。
 * - [DOCUMENT_TREE]：SAF DocumentsProvider（用户选中的 Download/Sujian 树），
 *   URI 形如 `content://com.android.providers.downloads.documents/tree/...`。
 */
enum class MirrorBackend {
    MEDIA_STORE,
    DOCUMENT_TREE,
}

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
 *   "backend": "document_tree",
 *   "treeUri": "content://.../tree/primary%2FDownload%2FSujian",
 *   "manifestUri": "content://.../document/...",
 *   "projects": {
 *     "<projectId>": {
 *       "<volumeId>/<chapterId>": {
 *         "uri": "content://...",
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
    private val lock = Any()

    private val stateFile: File by lazy {
        File(File(context.noBackupFilesDir, STATE_DIR_NAME), STATE_FILE_NAME).also { file ->
            file.parentFile?.mkdirs()
        }
    }

    /** pendingPublish journal 文件（#649 评论 5561465552 第 4 点）。 */
    private val pendingPublishFile: File by lazy {
        File(File(context.noBackupFilesDir, STATE_DIR_NAME), PENDING_PUBLISH_FILE_NAME).also { file ->
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

    /**
     * 批量写入多个章节条目（#649 评论 5561465552 第 4 点：事务性发布提交阶段使用）。
     *
     * 在锁内一次性写完所有条目，避免半提交状态。任一条目写入失败不影响其他条目
     * （JSONObject.put 不抛异常）。
     */
    fun putChapterEntries(entries: Map<ChapterKey, ChapterMirrorEntry>) {
        synchronized(lock) {
            val root = readRoot() ?: JSONObject()
            val projects = root.optJSONObject(PROJECTS_KEY) ?: JSONObject().also { root.put(PROJECTS_KEY, it) }
            for ((key, entry) in entries) {
                val projectObj =
                    projects.optJSONObject(key.projectId)
                        ?: JSONObject().also { projects.put(key.projectId, it) }
                projectObj.put(chapterKey(key.volumeId, key.chapterId), encodeEntry(entry))
            }
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

    // ── backend / treeUri 存取（#649 评论 5561465552 第 3 点）──

    /**
     * 当前镜像存储后端。
     *
     * 旧 state.json 没有该字段时返回 [MirrorBackend.MEDIA_STORE]（向后兼容：
     * 旧 Publisher 只用 MediaStore.Downloads）。
     */
    fun getBackend(): MirrorBackend {
        synchronized(lock) {
            val root = readRoot() ?: return MirrorBackend.MEDIA_STORE
            val name = root.optString(BACKEND_KEY).takeIf { it.isNotEmpty() }
            return when (name) {
                BACKEND_VALUE_DOCUMENT_TREE -> MirrorBackend.DOCUMENT_TREE
                BACKEND_VALUE_MEDIA_STORE -> MirrorBackend.MEDIA_STORE
                else -> MirrorBackend.MEDIA_STORE
            }
        }
    }

    /** 设置当前镜像存储后端。 */
    fun setBackend(backend: MirrorBackend) {
        synchronized(lock) {
            val root = readRoot() ?: JSONObject()
            root.put(
                BACKEND_KEY,
                when (backend) {
                    MirrorBackend.MEDIA_STORE -> BACKEND_VALUE_MEDIA_STORE
                    MirrorBackend.DOCUMENT_TREE -> BACKEND_VALUE_DOCUMENT_TREE
                },
            )
            writeRoot(root)
        }
    }

    /**
     * SAF document tree URI（[MirrorBackend.DOCUMENT_TREE] 后端时保存用户选中的 tree URI）。
     *
     * 后续 Publisher 用此 URI 通过 [DocumentTreeMirrorStorage] 写同一棵树，
     * 不再创建第二份镜像。
     */
    fun getTreeUri(): String? {
        synchronized(lock) {
            val root = readRoot() ?: return null
            return root.optString(TREE_URI_KEY).takeIf { it.isNotEmpty() }
        }
    }

    /** 设置 SAF document tree URI。 */
    fun setTreeUri(uri: String) {
        synchronized(lock) {
            val root = readRoot() ?: JSONObject()
            root.put(TREE_URI_KEY, uri)
            writeRoot(root)
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
     * #649 评论 5561465552 第 3 点：增加 backend 和 treeUri 参数。
     * 用用户刚选中的 tree URI 把：
     * - `_meta/manifest.json` 的 URI
     * - 每个 `contentFile` 对应的现有文档 URI
     * 写进 [ReadableMirrorStateStore]，供后续 Publisher 做集合差删除。
     *
     * @param manifestUri manifest 文件的 URI（MediaStore 或 SAF document URI）
     * @param chapterEntries 所有章节的条目（包含 URI、相对路径、revision、contentHash）
     * @param backend 本次恢复使用的存储后端（默认 [MirrorBackend.DOCUMENT_TREE]，
     *   因为恢复入口是 SAF OpenDocumentTree）
     * @param treeUri SAF document tree URI（document_tree 后端时必传）
     */
    fun saveRestoredState(
        manifestUri: String,
        chapterEntries: Map<ChapterKey, ChapterMirrorEntry>,
        backend: MirrorBackend = MirrorBackend.DOCUMENT_TREE,
        treeUri: String? = null,
    ) {
        synchronized(lock) {
            val root = readRoot() ?: JSONObject()
            // 写入 backend
            root.put(
                BACKEND_KEY,
                when (backend) {
                    MirrorBackend.MEDIA_STORE -> BACKEND_VALUE_MEDIA_STORE
                    MirrorBackend.DOCUMENT_TREE -> BACKEND_VALUE_DOCUMENT_TREE
                },
            )
            // 写入 treeUri（document_tree 后端时）
            if (treeUri != null) {
                root.put(TREE_URI_KEY, treeUri)
            }
            // 写入 manifest URI
            root.put(MANIFEST_URI_KEY, manifestUri)
            // 写入所有章节条目
            val projects = root.optJSONObject(PROJECTS_KEY) ?: JSONObject().also { root.put(PROJECTS_KEY, it) }
            for ((key, entry) in chapterEntries) {
                val projectObj =
                    projects.optJSONObject(key.projectId)
                        ?: JSONObject().also { projects.put(key.projectId, it) }
                projectObj.put(chapterKey(key.volumeId, key.chapterId), encodeEntry(entry))
            }
            writeRoot(root)
        }
    }

    // ── pendingPublish journal（#649 评论 5561465552 第 4 点：事务性发布）──

    /**
     * pendingPublish journal — 记录正在进行的发布。
     *
     * 发布开始时写 journal，每步更新，成功后删除 journal。
     * 下次启动/下一次 worker 如果发现 pendingPublish journal，继续完成这次发布
     * （重新写未完成的文件、删旧文件），不猜旧状态。
     *
     * journal 文件路径：`noBackupFilesDir/sujian-mirror/pending-publish.json`。
     *
     * @return journal JSON 内容；不存在或读取失败返回 null。
     */
    fun readPendingPublish(): String? {
        synchronized(lock) {
            if (!pendingPublishFile.exists()) return null
            return try {
                pendingPublishFile.readText(Charsets.UTF_8)
            } catch (e: IOException) {
                null
            }
        }
    }

    /**
     * 写入/覆盖 pendingPublish journal。
     *
     * @param journalJson 完整的 journal JSON（调用方负责组装）。
     */
    fun writePendingPublish(journalJson: String) {
        synchronized(lock) {
            try {
                pendingPublishFile.parentFile?.mkdirs()
                pendingPublishFile.writeText(journalJson, Charsets.UTF_8)
            } catch (e: IOException) {
                // journal 写入失败不阻断发布；最坏情况是中断后无法恢复未完成步骤。
            }
        }
    }

    /** 删除 pendingPublish journal（发布成功后调用）。 */
    fun clearPendingPublish() {
        synchronized(lock) {
            try {
                if (pendingPublishFile.exists()) {
                    pendingPublishFile.delete()
                }
            } catch (_: Exception) {
            }
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
        private const val PENDING_PUBLISH_FILE_NAME = "pending-publish.json"
        private const val PROJECTS_KEY = "projects"
        private const val MANIFEST_URI_KEY = "manifestUri"
        private const val BACKEND_KEY = "backend"
        private const val TREE_URI_KEY = "treeUri"
        private const val BACKEND_VALUE_MEDIA_STORE = "media_store"
        private const val BACKEND_VALUE_DOCUMENT_TREE = "document_tree"
        private const val URI_KEY = "uri"
        private const val RELATIVE_PATH_KEY = "relativePath"
        private const val REVISION_KEY = "revision"
        private const val CONTENT_HASH_KEY = "contentHash"
    }
}
