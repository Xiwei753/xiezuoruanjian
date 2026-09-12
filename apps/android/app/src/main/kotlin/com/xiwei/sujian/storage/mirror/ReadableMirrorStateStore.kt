package com.xiwei.sujian.storage.mirror

import android.content.Context
import androidx.core.util.AtomicFile
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import org.json.JSONException
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
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
 * 镜像状态快照 — [readSnapshotStrict] 返回的结构化完整状态。
 *
 * #649 评论 5563333323 缺口 2：用结构化快照替代隐式 emptyMap()，让调用方区分
 * "state.json 不存在/损坏"和"该作品确实没有任何条目"。
 */
data class MirrorStateSnapshot(
    val backend: MirrorBackend,
    val treeUri: String?,
    val manifestUri: String?,
    val projects: Map<String, Map<ChapterKey, ChapterMirrorEntry>>,
)

/**
 * pendingPublish journal 读取结果（#649 评论 5563333323 缺口 2：区分"不存在"和"损坏"）。
 *
 * 损坏的 pending journal 必须阻止启动新事务（调用方应停止本轮操作）。
 */
sealed class PendingPublishResult {
    /** journal 不存在（从未发布过或上次发布已成功清理）。 */
    object NotExists : PendingPublishResult()

    /** 成功读取。 */
    data class Success(val json: String) : PendingPublishResult()

    /** 文件存在但 JSON 损坏或读取失败。调用方应停止本轮操作，不启动新事务。 */
    data class Corrupted(val error: Exception) : PendingPublishResult()
}

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
 * 严格解码后的完整 state（#649 评论 5578472936）。
 *
 * 所有字段已经过类型检查和非空校验，任一处损坏会在 [decodeStateRootStrict] 中抛异常。
 * 三个 strict 入口（[readSnapshotStrict]、[getAllChapterEntriesStrict]、[getCommittedManifestStrict]）
 * 统一从此结构取数据，不再各自重复遍历 JSON。
 */
private data class StrictMirrorState(
    val backend: MirrorBackend,
    val treeUri: String?,
    val manifestUri: String?,
    val publishedProjectIds: Set<String>,
    val entries: Map<ChapterKey, ChapterMirrorEntry>,
    val committedManifestJson: String?,
    val committedManifestHash: String?,
)

/**
 * readRoot 的结果。
 *
 * #649 评论 5563333323 缺口 2：readRoot 不要把 JSON 损坏当成"没有 state"再覆盖成空对象。
 * 读坏了就返回明确错误，让 Publisher 停止本轮镜像操作。
 */
sealed class ReadResult {
    /** state.json 不存在（首次启动或被清空）。 */
    object NotExists : ReadResult()

    /** 成功解析。 */
    data class Parsed(val root: JSONObject) : ReadResult()

    /** 文件存在但 JSON 损坏或读取失败。调用方应停止本轮操作，不覆盖。 */
    data class Corrupted(val error: Exception) : ReadResult()
}

/**
 * 读取已提交 manifest 的三态结果（#649 评论 5576949398 问题 1）。
 *
 * 旧 [ReadableMirrorStateStore.getCommittedManifest] 把损坏当"首次发布"（返回 success(null)），
 * 让 Publisher 误判状态损坏为首次发布，继续写入新 manifest 覆盖损坏状态。
 *
 * 新 [ReadableMirrorStateStore.getCommittedManifestStrict] 返回本密封接口，明确区分：
 * - [NotExists]：state.json 不存在且没有任何旧 mirror state 字段（真正首次发布）
 * - [Found]：committed manifest 存在且校验通过
 * - [Corrupted]：state.json 损坏、committed json/hash 只存在一个、hash 不匹配或解析失败
 * - [NeedsMigration]：旧 state 有 manifestUri/publishedProjectIds/projects 但无 committed baseline，
 *   需要走 [ReadableMirrorStateMigration] 迁移，不能当首次发布继续写
 */
sealed interface CommittedManifestReadResult {
    /** state.json 不存在或全空，真正首次发布。 */
    data object NotExists : CommittedManifestReadResult

    /**
     * 已提交 manifest 存在且校验通过。
     *
     * @property json manifest JSON 字符串
     * @property hash manifest 的 SHA-256 hash
     * @property manifest 解析后的 [MirrorManifest] 对象
     */
    data class Found(
        val json: String,
        val hash: String,
        val manifest: MirrorManifest,
    ) : CommittedManifestReadResult

    /**
     * 状态损坏：state.json 读取失败、committed json/hash 只存在一个、hash 不匹配
     * 或 manifest 解析失败。调用方应停止本轮发布（返回 RetryableFailure），不碰 Download。
     *
     * @property cause 损坏原因
     */
    data class Corrupted(val cause: Throwable) : CommittedManifestReadResult

    /**
     * 旧 state 有 manifestUri/publishedProjectIds/projects 但无 committed baseline。
     *
     * 需要走 [ReadableMirrorStateMigration] 迁移：从 manifestUri 读取 manifest，
     * 严格校验后一次性写入 committed baseline。迁移完成前不能当首次发布继续写，
     * 否则会把多作品 manifest 退化成单作品。
     */
    data object NeedsMigration : CommittedManifestReadResult
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
 * `context.filesDir/sujian/mirror/state.json`（通过 [AndroidPrivateDataRoot.mirror]）
 *
 * Issue #667：从旧位置 `noBackupFilesDir/sujian-mirror/` 迁移到应用私有目录
 * `filesDir/sujian/mirror/`，与 [MirrorTransactionWorkspace] 共用同一目录。
 * 升级后首次访问时自动从旧位置迁移 state.json 和 pending-publish.json。
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
@Suppress("LargeClass")
class ReadableMirrorStateStore(
    private val context: Context,
) {
    private val lock = Any()

    private val stateFile: File by lazy {
        val newFile = File(AndroidPrivateDataRoot.mirror(context), STATE_FILE_NAME)
        // Issue #667：从旧位置 noBackupFilesDir/sujian-mirror/ 迁移到新位置
        if (!newFile.exists()) {
            val legacyFile = File(File(context.noBackupFilesDir, LEGACY_STATE_DIR_NAME), STATE_FILE_NAME)
            if (legacyFile.exists()) {
                legacyFile.copyTo(newFile, overwrite = false)
            }
        }
        newFile.parentFile?.mkdirs()
        newFile
    }

    /** #649 评论 5563333323 缺口 2：用 AtomicFile 做原子写入。 */
    private val stateAtomicFile: AtomicFile by lazy { AtomicFile(stateFile) }

    /** pendingPublish journal 文件（#649 评论 5561465552 第 4 点）。 */
    private val pendingPublishFile: File by lazy {
        val newFile = File(AndroidPrivateDataRoot.mirror(context), PENDING_PUBLISH_FILE_NAME)
        // Issue #667：从旧位置迁移
        if (!newFile.exists()) {
            val legacyFile = File(File(context.noBackupFilesDir, LEGACY_STATE_DIR_NAME), PENDING_PUBLISH_FILE_NAME)
            if (legacyFile.exists()) {
                legacyFile.copyTo(newFile, overwrite = false)
            }
        }
        newFile.parentFile?.mkdirs()
        newFile
    }

    /** #649 评论 5563333323 缺口 2：journal 也用 AtomicFile 做原子写入。 */
    private val pendingPublishAtomicFile: AtomicFile by lazy { AtomicFile(pendingPublishFile) }

    /** 获取某作品下全部章节条目。 */
    fun getProjectEntries(projectId: String): Map<ChapterKey, ChapterMirrorEntry> {
        synchronized(lock) {
            val root = readRootForRead() ?: return emptyMap()
            val projectObj = root.optJSONObject(PROJECTS_KEY)?.optJSONObject(projectId) ?: return emptyMap()
            return decodeProjectEntries(projectId, projectObj)
        }
    }

    /**
     * 严格读取完整状态快照（#649 评论 5563333323 缺口 2）。
     *
     * 与 [getProjectEntries] 不同：
     * - [getProjectEntries] 在 state.json 损坏时返回 `emptyMap()`，调用方不知道是损坏还是真的空
     * - 本方法在损坏时返回 [Result.failure]，让调用方区分"不存在/损坏"与"该作品确实无条目"
     *
     * @return [Result.success] 包含完整快照；[Result.failure] 包含 [ReadResult.NotExists]
     *   或 [ReadResult.Corrupted] 异常
     */
    fun readSnapshotStrict(): Result<MirrorStateSnapshot> {
        synchronized(lock) {
            return when (val result = readRoot()) {
                // #649 评论 5564624383 问题 3：首次安装 state.json 不存在不是损坏，
                // 是合法初始状态，默认 MEDIA_STORE 后端。
                is ReadResult.NotExists ->
                    Result.success(
                        MirrorStateSnapshot(
                            backend = MirrorBackend.MEDIA_STORE,
                            treeUri = null,
                            manifestUri = null,
                            projects = emptyMap(),
                        ),
                    )
                is ReadResult.Corrupted -> Result.failure(result.error)
                is ReadResult.Parsed ->
                    runCatching {
                        // #649 评论 5578472936：统一走 decodeStateRootStrict，
                        // 不再用 optString/optJSONObject/continue 静默裁掉坏 state。
                        val s = decodeStateRootStrict(result.root)
                        MirrorStateSnapshot(
                            backend = s.backend,
                            treeUri = s.treeUri,
                            manifestUri = s.manifestUri,
                            projects =
                                s.entries.entries
                                    .groupBy { it.key.projectId }
                                    .mapValues { (_, values) -> values.associate { it.toPair() } },
                        )
                    }
            }
        }
    }

    /** 写入/覆盖单章条目。返回 true 表示持久化成功；false 表示失败（调用方应停止本轮操作）。 */
    fun putChapterEntry(
        projectId: String,
        volumeId: String,
        chapterId: String,
        entry: ChapterMirrorEntry,
    ): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            val projects = root.optJSONObject(PROJECTS_KEY) ?: JSONObject().also { root.put(PROJECTS_KEY, it) }
            val projectObj = projects.optJSONObject(projectId) ?: JSONObject().also { projects.put(projectId, it) }
            projectObj.put(chapterKey(volumeId, chapterId), encodeEntry(entry))
            return writeRoot(root)
        }
    }

    /**
     * 批量写入多个章节条目（#649 评论 5561465552 第 4 点：事务性发布提交阶段使用）。
     *
     * 在锁内一次性写完所有条目，避免半提交状态。任一条目写入失败不影响其他条目
     * （JSONObject.put 不抛异常）。
     *
     * manifest 提交成功后（进入 PHASE_CLEANUP 之前），把本事务最终的 manifest JSON/hash
     * 幂等写进 private state（#649 评论 5576464076 问题 2）。
     *
     * @param entries 章节条目映射
     * @param committedManifestJson 本次事务最终的 manifest JSON 字符串（可选，提交成功后写入）
     * @param committedManifestHash 本次事务最终的 manifest 的 SHA-256 hash（可选，提交成功后写入）
     * @return true 表示持久化成功；false 表示失败（调用方应停止本轮操作，不清 journal）。
     */
    fun putChapterEntries(
        entries: Map<ChapterKey, ChapterMirrorEntry>,
        committedManifestJson: String? = null,
        committedManifestHash: String? = null,
    ): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            val projects = root.optJSONObject(PROJECTS_KEY) ?: JSONObject().also { root.put(PROJECTS_KEY, it) }
            for ((key, entry) in entries) {
                val projectObj =
                    projects.optJSONObject(key.projectId)
                        ?: JSONObject().also { projects.put(key.projectId, it) }
                projectObj.put(chapterKey(key.volumeId, key.chapterId), encodeEntry(entry))
            }
            // manifest 提交成功后，幂等写入 committed manifest 信息
            if (committedManifestJson != null && committedManifestHash != null) {
                root.put(COMMITTED_MANIFEST_JSON_KEY, committedManifestJson)
                root.put(COMMITTED_MANIFEST_HASH_KEY, committedManifestHash)
            }
            return writeRoot(root)
        }
    }

    /**
     * 删除单章条目（幂等）。返回 true 表示持久化成功；false 表示失败。
     */
    fun removeChapterEntry(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            val projects = root.optJSONObject(PROJECTS_KEY) ?: return true
            val projectObj = projects.optJSONObject(projectId) ?: return true
            projectObj.remove(chapterKey(volumeId, chapterId))
            if (projectObj.length() == 0) {
                projects.remove(projectId)
            }
            if (projects.length() == 0) {
                root.remove(PROJECTS_KEY)
            }
            return writeRoot(root)
        }
    }

    /**
     * 删除某作品的全部条目，返回被删除的条目（供 Publisher 逐个删 MediaStore URI）。
     *
     * #649 评论 5563333323 缺口 2：返回 Result，写失败时调用方不清 journal。
     */
    fun removeAllProjectEntries(projectId: String): Result<Map<ChapterKey, ChapterMirrorEntry>> {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return Result.failure(IOException("state read failed or corrupted"))
            val projects = root.optJSONObject(PROJECTS_KEY) ?: return Result.success(emptyMap())
            val projectObj = projects.optJSONObject(projectId) ?: return Result.success(emptyMap())
            val removed = decodeProjectEntries(projectId, projectObj)
            projects.remove(projectId)
            if (projects.length() == 0) {
                root.remove(PROJECTS_KEY)
            }
            if (!writeRoot(root)) {
                return Result.failure(IOException("state write failed"))
            }
            return Result.success(removed)
        }
    }

    /** 列出所有有镜像条目的作品 ID。 */
    fun getAllProjectIds(): Set<String> {
        synchronized(lock) {
            val root = readRootForRead() ?: return emptySet()
            val ids = mutableSetOf<String>()
            // #649 评论 5564820566 问题 5：零章节作品也需要独立 project 状态。
            // publishedProjectIds 独立于章节条目保存，即使 chapters={} 也保留。
            val publishedObj = root.optJSONObject(PUBLISHED_PROJECTS_KEY)
            if (publishedObj != null) {
                val keys = publishedObj.keys()
                while (keys.hasNext()) {
                    ids.add(keys.next())
                }
            }
            val projects = root.optJSONObject(PROJECTS_KEY)
            if (projects != null) {
                val keys = projects.keys()
                while (keys.hasNext()) {
                    ids.add(keys.next())
                }
            }
            return ids
        }
    }

    /**
     * 严格读取全量 published IDs + chapter entries 的 snapshot。
     *
     * #649 评论 5577831998 问题 2：迁移需要一个严格读取全量 published IDs + chapter entries 的 snapshot 接口，
     * 不要继续用会在损坏时返回 `emptyMap()` 的宽松 getter 做迁移判断。
     * 本方法在 state.json 损坏时返回 Result.failure，让调用方区分"不存在/损坏"与"确实没有条目"。
     *
     * @return Result.success 包含完整 published ID 集合和 chapter entries 映射；
     *   Result.failure 包含读取失败或损坏的异常
     */
    fun getAllChapterEntriesStrict(): Result<Pair<Set<String>, Map<ChapterKey, ChapterMirrorEntry>>> {
        synchronized(lock) {
            return when (val result = readRoot()) {
                is ReadResult.NotExists ->
                    Result.success(emptySet<String>() to emptyMap())
                is ReadResult.Corrupted -> Result.failure(result.error)
                is ReadResult.Parsed ->
                    runCatching {
                        // #649 评论 5578472936：直接从同一份 StrictMirrorState 返回
                        // publishedProjectIds + entries，不再重复遍历 JSON。
                        val s = decodeStateRootStrict(result.root)
                        val ids = s.publishedProjectIds.toMutableSet()
                        for (key in s.entries.keys) {
                            ids.add(key.projectId)
                        }
                        ids to s.entries
                    }
            }
        }
    }

    /**
     * 严格解码 chapter entry，任一字段缺失/类型错误/空值都抛异常。
     *
     * #649 评论 5578053805 问题 2：供 [getAllChapterEntriesStrict] 使用，
     * 不能用 optXxx / null fallback / continue 跳过损坏数据。
     * 迁移判断必须 fail-closed：坏 state 被静默裁掉会导致
     * verifyManifestAgainstState 误判 manifest 与 state 一致。
     */
    private fun decodeEntryStrict(
        projectId: String,
        chapterKey: String,
        obj: JSONObject,
    ): Pair<ChapterKey, ChapterMirrorEntry> {
        val parts = chapterKey.split("/", limit = 2)
        require(parts.size == 2 && parts[0].isNotEmpty() && parts[1].isNotEmpty()) {
            "State corruption: invalid chapter key '$chapterKey' in project $projectId"
        }

        val uriRaw = obj.get(URI_KEY)
        require(uriRaw is String) {
            "State corruption: uri must be String for chapter key '$chapterKey'" +
                " in project $projectId, got ${uriRaw?.javaClass?.simpleName}"
        }
        val uri = uriRaw

        val relativePathRaw = obj.get(RELATIVE_PATH_KEY)
        require(relativePathRaw is String) {
            "State corruption: relativePath must be String for chapter key '$chapterKey'" +
                " in project $projectId, got ${relativePathRaw?.javaClass?.simpleName}"
        }
        val relativePath = relativePathRaw

        val revisionRaw = obj.get(REVISION_KEY)
        require(revisionRaw is Number) {
            "State corruption: revision must be Number for chapter key '$chapterKey'" +
                " in project $projectId, got ${revisionRaw?.javaClass?.simpleName}"
        }
        val revision = revisionRaw.toLong()

        val contentHashRaw = obj.get(CONTENT_HASH_KEY)
        require(contentHashRaw is String) {
            "State corruption: contentHash must be String for chapter key '$chapterKey'" +
                " in project $projectId, got ${contentHashRaw?.javaClass?.simpleName}"
        }
        val contentHash = contentHashRaw

        require(uri.isNotEmpty()) {
            "State corruption: empty uri for chapter key '$chapterKey' in project $projectId"
        }
        require(relativePath.isNotEmpty()) {
            "State corruption: empty relativePath for chapter key '$chapterKey' in project $projectId"
        }
        require(contentHash.isNotEmpty()) {
            "State corruption: empty contentHash for chapter key '$chapterKey' in project $projectId"
        }

        return ChapterKey(projectId, parts[0], parts[1]) to
            ChapterMirrorEntry(uri, relativePath, revision, contentHash)
    }

    /**
     * 从已解析的 state root 严格解码完整状态（#649 评论 5578472936）。
     *
     * 不再维护三套"半严格解析"，统一在此处做一次真正的严格校验。
     * 任一处异常直接抛出，让上层转换为 `Result.failure` / `Corrupted`，
     * 绝不能 `continue`、`emptyMap()` 或默认值降级。
     *
     * Android `org.json` 的字符串 getter 存在类型 coercion 语义，
     * 所有字段用 `obj.get(KEY)` + `require(value is T)` 做真正的类型检查。
     */
    private fun decodeStateRootStrict(root: JSONObject): StrictMirrorState {
        // #649 评论 5578666118：用 parseBackendStrict 区分"字段不存在"和"字段存在但空字符串"。
        val backend = parseBackendStrict(root)

        // #649 评论 5578666118：treeUri 存在时必须是非空字符串，空字符串是损坏状态。
        val treeUri =
            root.opt(TREE_URI_KEY)?.let { raw ->
                require(raw is String && raw.isNotEmpty()) {
                    "State corruption: treeUri must be a non-empty String"
                }
                raw
            }

        // #649 评论 5578666118：backend=document_tree 必须有非空 treeUri。
        if (backend == MirrorBackend.DOCUMENT_TREE) {
            require(treeUri != null) {
                "State corruption: document_tree backend requires treeUri"
            }
        }

        // #649 评论 5578666118：manifestUri 存在时也必须是非空字符串；
        // "没有 manifest"用字段不存在表达，不用空字符串。
        val manifestUri =
            root.opt(MANIFEST_URI_KEY)?.let { raw ->
                require(raw is String && raw.isNotEmpty()) {
                    "State corruption: manifestUri must be a non-empty String"
                }
                raw
            }

        root.opt(PUBLISHED_PROJECTS_KEY)?.let {
            require(
                it is JSONObject,
            ) { "State corruption: publishedProjectIds must be JSONObject, got ${it.javaClass.simpleName}" }
        }
        val projectsRaw = root.opt(PROJECTS_KEY)
        if (projectsRaw != null) {
            require(projectsRaw is JSONObject) {
                "State corruption: projects must be JSONObject, got ${projectsRaw.javaClass.simpleName}"
            }
        }
        val entries = decodeEntriesStrict(projectsRaw)
        val committedJson =
            root.opt(COMMITTED_MANIFEST_JSON_KEY)?.let {
                require(
                    it is String,
                ) { "State corruption: committedManifestJson must be String, got ${it.javaClass.simpleName}" }
                it
            }
        val committedHash =
            root.opt(COMMITTED_MANIFEST_HASH_KEY)?.let {
                require(
                    it is String,
                ) { "State corruption: committedManifestHash must be String, got ${it.javaClass.simpleName}" }
                it
            }
        return StrictMirrorState(
            backend = backend,
            treeUri = treeUri,
            manifestUri = manifestUri,
            publishedProjectIds =
                root.opt(PUBLISHED_PROJECTS_KEY)?.let {
                    (it as JSONObject).keys().asSequence().toSet()
                } ?: emptySet(),
            entries = entries,
            committedManifestJson = committedJson,
            committedManifestHash = committedHash,
        )
    }

    /** 严格解码 projects 节点为 chapter entries 映射（#651 评论 5592465805：拆分降低 decodeStateRootStrict 复杂度）。 */
    private fun decodeEntriesStrict(projectsRaw: Any?): MutableMap<ChapterKey, ChapterMirrorEntry> {
        val entries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        if (projectsRaw is JSONObject) {
            val projectIds = projectsRaw.keys()
            while (projectIds.hasNext()) {
                val projectId = projectIds.next()
                val projectObj = projectsRaw.get(projectId)
                require(projectObj is JSONObject) {
                    "State corruption: project '$projectId' must be JSONObject," +
                        " got ${projectObj?.javaClass?.simpleName}"
                }
                decodeProjectEntriesStrict(projectId, projectObj, entries)
            }
        }
        return entries
    }

    /** 严格解码单个 project 的所有 chapter entries。 */
    private fun decodeProjectEntriesStrict(
        projectId: String,
        projectObj: JSONObject,
        entries: MutableMap<ChapterKey, ChapterMirrorEntry>,
    ) {
        val chapterKeys = projectObj.keys()
        while (chapterKeys.hasNext()) {
            val chapterKeyStr = chapterKeys.next()
            val entryObj = projectObj.get(chapterKeyStr)
            require(entryObj is JSONObject) {
                "State corruption: chapter entry '$chapterKeyStr' in project '$projectId' must be JSONObject," +
                    " got ${entryObj?.javaClass?.simpleName}"
            }
            val (key, entry) = decodeEntryStrict(projectId, chapterKeyStr, entryObj)
            entries[key] = entry
        }
    }

    /**
     * 标记作品已发布到镜像（manifest 提交成功后调用）。
     * 零章节作品在 putChapterEntries(emptyMap()) 后不会留下 projectId，
     * 但 publishedProjectIds 能保留该信息，让 cleanupStaleProjects 正确清理。
     *
     * @return true 表示持久化成功。
     */
    fun addPublishedProjectId(projectId: String): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            val published =
                root.optJSONObject(PUBLISHED_PROJECTS_KEY)
                    ?: JSONObject().also { root.put(PUBLISHED_PROJECTS_KEY, it) }
            published.put(projectId, true)
            return writeRoot(root)
        }
    }

    /**
     * 移除作品的已发布标记（delete 成功后调用）。
     *
     * @return true 表示持久化成功。
     */
    fun removePublishedProjectId(projectId: String): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            val published = root.optJSONObject(PUBLISHED_PROJECTS_KEY) ?: return true
            published.remove(projectId)
            if (published.length() == 0) {
                root.remove(PUBLISHED_PROJECTS_KEY)
            }
            return writeRoot(root)
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
            val root = readRootForRead() ?: return MirrorBackend.MEDIA_STORE
            val name = root.optString(BACKEND_KEY).takeIf { it.isNotEmpty() }
            return when (name) {
                BACKEND_VALUE_DOCUMENT_TREE -> MirrorBackend.DOCUMENT_TREE
                BACKEND_VALUE_MEDIA_STORE -> MirrorBackend.MEDIA_STORE
                else -> MirrorBackend.MEDIA_STORE
            }
        }
    }

    /** 设置当前镜像存储后端。返回 true 表示持久化成功。 */
    fun setBackend(backend: MirrorBackend): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            root.put(
                BACKEND_KEY,
                when (backend) {
                    MirrorBackend.MEDIA_STORE -> BACKEND_VALUE_MEDIA_STORE
                    MirrorBackend.DOCUMENT_TREE -> BACKEND_VALUE_DOCUMENT_TREE
                },
            )
            return writeRoot(root)
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
            val root = readRootForRead() ?: return null
            return root.optString(TREE_URI_KEY).takeIf { it.isNotEmpty() }
        }
    }

    /** 设置 SAF document tree URI。返回 true 表示持久化成功。 */
    fun setTreeUri(uri: String): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            root.put(TREE_URI_KEY, uri)
            return writeRoot(root)
        }
    }

    // ── committed manifest 存取（#649 评论 5576464076 问题 2）──

    /**
     * 获取上一次已提交的 manifest。
     *
     * @return [Result.success] 包含 manifest JSON 字符串（从未提交过时为 null）；
     *   [Result.failure] 包含读取失败或 JSON 损坏的异常
     */
    fun getCommittedManifest(): Result<String?> {
        synchronized(lock) {
            val root = readRootForRead() ?: return Result.success(null)
            val json = root.optString(COMMITTED_MANIFEST_JSON_KEY).takeIf { it.isNotEmpty() }
            return if (json != null) {
                Result.success(json)
            } else {
                // 字段不存在，向后兼容：旧 state.json 没有该字段
                Result.success(null)
            }
        }
    }

    /**
     * 严格读取已提交 manifest，返回三态结果（#649 评论 5576949398 问题 1）。
     *
     * 旧 [getCommittedManifest] 把损坏当"首次发布"（返回 success(null)），让 Publisher
     * 误判状态损坏为首次发布继续写。本方法用 [readRoot]（不把损坏当不存在），
     * 明确区分四种情况：
     *
     * 1. state.json 不存在，且没有任何旧 mirror state 字段 → [CommittedManifestReadResult.NotExists]
     * 2. committedManifestJson/hash 都存在且校验通过 → [CommittedManifestReadResult.Found]
     * 3. 旧 state 有 manifestUri/publishedProjectIds/projects 但无 committed baseline
     *    → [CommittedManifestReadResult.NeedsMigration]（需走迁移，不能当首次发布）
     * 4. state.json 损坏、json/hash 只存在一个、hash 不匹配或解析失败
     *    → [CommittedManifestReadResult.Corrupted]
     *
     * @return 三态结果，调用方据此决定首次发布 / 复用基线 / 停止 / 触发迁移
     */
    @Suppress("CyclomaticComplexMethod", "CognitiveComplexMethod")
    fun getCommittedManifestStrict(): CommittedManifestReadResult {
        synchronized(lock) {
            return when (val result = readRoot()) {
                is ReadResult.NotExists -> CommittedManifestReadResult.NotExists
                is ReadResult.Corrupted -> CommittedManifestReadResult.Corrupted(result.error)
                is ReadResult.Parsed -> {
                    // #649 评论 5578472936：统一走 decodeStateRootStrict。
                    // "baseline 正常但 projects 损坏"的 state 也会先整体判 Corrupted，
                    // 不会被 Router 放行。
                    try {
                        val s = decodeStateRootStrict(result.root)
                        val json = s.committedManifestJson
                        val hash = s.committedManifestHash
                        when {
                            // json/hash 都存在：校验 hash + 严格解析
                            json != null && hash != null -> {
                                val computedHash = computeContentHash(json)
                                if (computedHash != hash) {
                                    CommittedManifestReadResult.Corrupted(
                                        IllegalArgumentException(
                                            "Committed manifest hash mismatch: stored=$hash, computed=$computedHash",
                                        ),
                                    )
                                } else {
                                    try {
                                        val manifest = mirrorManifestFromJsonStrict(json)
                                        CommittedManifestReadResult.Found(json, hash, manifest)
                                    } catch (e: Exception) {
                                        CommittedManifestReadResult.Corrupted(e)
                                    }
                                }
                            }
                            // json/hash 只存在一个：状态不完整，视为损坏
                            json != null || hash != null ->
                                CommittedManifestReadResult.Corrupted(
                                    IllegalArgumentException(
                                        "Committed manifest partial state: json=${json != null}, hash=${hash != null}",
                                    ),
                                )
                            // json/hash 都不存在：检查是否有旧 mirror state 字段
                            else -> {
                                val hasManifestUri = s.manifestUri?.isNotEmpty() == true
                                val hasPublishedProjects = s.publishedProjectIds.isNotEmpty()
                                val hasProjects = s.entries.isNotEmpty()
                                if (hasManifestUri || hasPublishedProjects || hasProjects) {
                                    CommittedManifestReadResult.NeedsMigration
                                } else {
                                    CommittedManifestReadResult.NotExists
                                }
                            }
                        }
                    } catch (e: JSONException) {
                        CommittedManifestReadResult.Corrupted(e)
                    } catch (e: IllegalArgumentException) {
                        CommittedManifestReadResult.Corrupted(e)
                    }
                }
            }
        }
    }

    /**
     * 记录上一次已提交的 manifest（幂等写入）。
     *
     * @param json 本次事务最终的 manifest JSON 字符串
     * @param hash 本次事务最终的 manifest 的 SHA-256 hash
     * @return true 表示持久化成功；false 表示失败
     */
    fun setCommittedManifest(
        json: String,
        hash: String,
    ): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            root.put(COMMITTED_MANIFEST_JSON_KEY, json)
            root.put(COMMITTED_MANIFEST_HASH_KEY, hash)
            return writeRoot(root)
        }
    }

    // ── manifest URI 存取 ──

    /** 获取 manifest 文件的 MediaStore URI（供 Publisher 覆盖写入）。 */
    fun getManifestUri(): String? {
        synchronized(lock) {
            val root = readRootForRead() ?: return null
            return root.optString(MANIFEST_URI_KEY).takeIf { it.isNotEmpty() }
        }
    }

    /** 计入 manifest 文件 URI。返回 true 表示持久化成功。 */
    fun setManifestUri(uri: String): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            root.put(MANIFEST_URI_KEY, uri)
            return writeRoot(root)
        }
    }

    /**
     * 清除 manifest 文件 URI（#649 评论 5572554935 问题 4）。
     *
     * 首次发布 manifest（无旧 manifest）的事务回滚时调用：
     * 正确的回滚结果是 stateStore 不再持有 manifestUri（恢复成"没有 manifest"）。
     * 旧实现只有 getManifestUri/setManifestUri，无法清除 manifestUri，
     * 导致 no-old manifest rollback 直接 return true 时 manifestUri 仍指向新 manifest。
     *
     * @return true 表示持久化成功（或字段本就不存在）；false 表示失败（state 损坏或写入失败）
     */
    fun clearManifestUri(): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            root.remove(MANIFEST_URI_KEY)
            return writeRoot(root)
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
     * #649 评论 5563333323 缺口 2：state.json 损坏时返回 false，不再静默覆盖成空对象。
     * 调用方应报告错误并提示用户。
     *
     * #649 评论 5565067997 修复 6：增加 publishedProjectIds 参数。
     * 旧实现从 chapterEntries 推导 published ID，零章节作品没有 chapterEntries 所以不进 publishedProjectIds。
     * 新实现让 ReadableMirrorRestorer 直接传 manifest.projects.map { it.id }.toSet()，
     * 和章节条目一起在同一次 AtomicFile state 写入里保存。
     *
     * #649 评论 5576949398 问题 3：增加 committedManifestJson / committedManifestHash 参数。
     * SAF 恢复后第一次编辑会把多作品 manifest 退化成单作品，根因是恢复后 state 没有
     * committed baseline，Publisher 误判为首次发布。新实现让 Restorer 在恢复时就把
     * normalized manifest JSON 和 hash 写入 committed baseline，和 backend/treeUri/projects
     * 一起在同一次 AtomicFile 写入里保存，不能先写 state 再单独 setCommittedManifest。
     *
     * @param manifestUri manifest 文件的 URI（MediaStore 或 SAF document URI）
     * @param chapterEntries 所有章节的条目（包含 URI、相对路径、revision、contentHash）
     * @param backend 本次恢复使用的存储后端（默认 [MirrorBackend.DOCUMENT_TREE]，
     *   因为恢复入口是 SAF OpenDocumentTree）
     * @param treeUri SAF document tree URI（document_tree 后端时必传）
     * @param publishedProjectIds 已发布作品的 ID 集合（含零章节作品），
     *   默认空集（向后兼容，旧调用方不传时从 chapterEntries 推导）
     * @param committedManifestJson 已提交 manifest 的 JSON 字符串（#649 评论 5576949398 问题 3，
     *   恢复时必传，写入 committed baseline 避免首次编辑退化）
     * @param committedManifestHash 已提交 manifest 的 SHA-256 hash（恢复时必传）
     * @return true 表示持久化成功；false 表示失败（state.json 损坏或写入失败）
     */
    @Suppress("CyclomaticComplexMethod", "CognitiveComplexMethod", "LongParameterList")
    fun saveRestoredState(
        manifestUri: String,
        chapterEntries: Map<ChapterKey, ChapterMirrorEntry>,
        backend: MirrorBackend = MirrorBackend.DOCUMENT_TREE,
        treeUri: String? = null,
        publishedProjectIds: Set<String> = emptySet(),
        committedManifestJson: String = "",
        committedManifestHash: String = "",
    ): Boolean {
        synchronized(lock) {
            // #649 评论 5563333323 缺口 2：用 readRoot() 区分"不存在"和"损坏"
            // #649 评论 5578472936：ReadResult.Parsed 时先跑 strict decoder，
            // 旧 state 结构损坏就直接 false，不用 optJSONObject() 把坏字段覆盖掉。
            val root =
                when (val result = readRoot()) {
                    is ReadResult.NotExists -> JSONObject()
                    is ReadResult.Corrupted -> return false
                    is ReadResult.Parsed -> {
                        try {
                            decodeStateRootStrict(result.root)
                        } catch (_: Exception) {
                            return false
                        }
                        result.root
                    }
                }
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
            // #649 评论 5564820566 问题 5：恢复时也写 publishedProjectIds
            val published =
                root.optJSONObject(PUBLISHED_PROJECTS_KEY)
                    ?: JSONObject().also { root.put(PUBLISHED_PROJECTS_KEY, it) }
            // #649 评论 5565067997 修复 6：优先用传入的 publishedProjectIds，
            // 同时也从 chapterEntries 推导（向后兼容），取并集。
            val projectIds = mutableSetOf<String>()
            for ((key, entry) in chapterEntries) {
                val projectObj =
                    projects.optJSONObject(key.projectId)
                        ?: JSONObject().also { projects.put(key.projectId, it) }
                projectObj.put(chapterKey(key.volumeId, key.chapterId), encodeEntry(entry))
                projectIds.add(key.projectId)
            }
            // 合并传入的 publishedProjectIds（含零章节作品）
            projectIds.addAll(publishedProjectIds)
            for (pid in projectIds) {
                published.put(pid, true)
            }
            // #649 评论 5576949398 问题 3：在同一次 AtomicFile 写入里保存 committed baseline，
            // 避免先写 state 再单独 setCommittedManifest 的非原子窗口。
            // committedManifestJson/hash 非空时才写（向后兼容旧调用方不传时跳过）。
            if (committedManifestJson.isNotEmpty() && committedManifestHash.isNotEmpty()) {
                root.put(COMMITTED_MANIFEST_JSON_KEY, committedManifestJson)
                root.put(COMMITTED_MANIFEST_HASH_KEY, committedManifestHash)
            }
            return writeRoot(root)
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
     * journal 文件路径：`filesDir/sujian/mirror/pending-publish.json`（[AndroidPrivateDataRoot.mirror]）。
     *
     * #649 评论 5563333323 缺口 2：返回 [PendingPublishResult] 区分"不存在"和"损坏"。
     * 损坏的 pending journal 必须阻止启动新事务。
     *
     * @return [PendingPublishResult.NotExists] 文件不存在；
     *   [PendingPublishResult.Success] 成功读取；
     *   [PendingPublishResult.Corrupted] 文件存在但 JSON 损坏或读取失败
     */
    fun readPendingPublish(): PendingPublishResult {
        synchronized(lock) {
            if (!pendingPublishFile.exists()) return PendingPublishResult.NotExists
            return try {
                val json = pendingPublishFile.readText(Charsets.UTF_8)
                // #649 评论 5564379115 问题 5：立即验证 JSON 可解析性，
                // 不让坏 JSON 被当成 "Success" 后在恢复时走 fromJson() 失败
                // 然后 ensurePendingRecovered 仍然设 pendingRecovered=true
                PendingMirrorPublish.fromJson(json)
                    ?: return PendingPublishResult.Corrupted(
                        IllegalArgumentException("Pending publish JSON is invalid"),
                    )
                PendingPublishResult.Success(json)
            } catch (e: IOException) {
                PendingPublishResult.Corrupted(e)
            }
        }
    }

    /**
     * 写入/覆盖 pendingPublish journal。
     *
     * #649 评论 5563333323 缺口 2：用 AtomicFile 原子写入，返回 Boolean。
     * Publisher 规则：下一步会改变外部镜像之前，上一步 journal 必须确认持久化成功；
     * 失败则停止本轮镜像操作（不继续移动文件）。
     *
     * @param journalJson 完整的 journal JSON（调用方负责组装）。
     * @return true 表示持久化成功；false 表示失败（调用方应停止本轮操作）。
     */
    fun writePendingPublish(journalJson: String): Boolean {
        synchronized(lock) {
            return try {
                pendingPublishFile.parentFile?.mkdirs()
                val os = pendingPublishAtomicFile.startWrite() as FileOutputStream
                try {
                    os.write(journalJson.toByteArray(Charsets.UTF_8))
                    pendingPublishAtomicFile.finishWrite(os)
                    true
                } catch (e: IOException) {
                    pendingPublishAtomicFile.failWrite(os)
                    false
                }
            } catch (e: IOException) {
                false
            }
        }
    }

    /**
     * 删除 pendingPublish journal（发布成功后调用）。
     *
     * @return true 表示删除成功或文件本就不存在；false 表示删除失败。
     */
    fun clearPendingPublish(): Boolean {
        synchronized(lock) {
            return try {
                // AtomicFile.delete() 删除 .new 临时文件和正式文件
                pendingPublishAtomicFile.delete()
                true
            } catch (_: Exception) {
                false
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

    /**
     * 从 root 对象严格解析 backend（#649 评论 5578666118）。
     *
     * 与旧 `parseBackend()` 的区别：
     * - 字段不存在 → 向后兼容为 [MirrorBackend.MEDIA_STORE]（旧版 state 无该字段）。
     * - 字段存在但值为空字符串 / 非字符串 / 未知值 → 抛 [IllegalArgumentException]。
     *   旧代码把 `backend=""` 当 null 再回退到 MEDIA_STORE，掩盖损坏状态，
     *   导致新事务写进 MediaStore 而旧镜像在用户选中的 DocumentTree。
     */
    private fun parseBackendStrict(root: JSONObject): MirrorBackend {
        if (!root.has(BACKEND_KEY)) {
            return MirrorBackend.MEDIA_STORE
        }

        val raw = root.get(BACKEND_KEY)
        require(raw is String && raw.isNotEmpty()) {
            "State corruption: backend must be a non-empty String," +
                " got ${if (raw is String) "empty" else raw?.javaClass?.simpleName}"
        }

        return when (raw) {
            BACKEND_VALUE_MEDIA_STORE -> MirrorBackend.MEDIA_STORE
            BACKEND_VALUE_DOCUMENT_TREE -> MirrorBackend.DOCUMENT_TREE
            else -> throw IllegalArgumentException("Unknown backend value: $raw")
        }
    }

    /**
     * 读取 state root。
     *
     * #649 评论 5563333323 缺口 2：返回 [ReadResult]，不把 JSON 损坏当成"没有 state"。
     * 调用方遇到 [ReadResult.Corrupted] 时应停止操作，不覆盖成空对象。
     */
    private fun readRoot(): ReadResult {
        if (!stateFile.exists()) return ReadResult.NotExists
        return try {
            ReadResult.Parsed(JSONObject(stateAtomicFile.readFully().toString(Charsets.UTF_8)))
        } catch (e: IOException) {
            ReadResult.Corrupted(e)
        } catch (e: JSONException) {
            ReadResult.Corrupted(e)
        }
    }

    /**
     * 读 root 用于更新；损坏时返回 null（调用方应停止本轮操作，不覆盖）。
     * 文件不存在时返回空 [JSONObject]（首次启动）。
     */
    private fun readRootForUpdate(): JSONObject? {
        return when (val result = readRoot()) {
            ReadResult.NotExists -> JSONObject()
            is ReadResult.Parsed -> result.root
            is ReadResult.Corrupted -> null
        }
    }

    /**
     * 读 root 用于只读查询；损坏时返回 null（调用方返回默认值，不抛异常）。
     * 与 [readRootForUpdate] 区别：损坏时不停止操作（只读查询返回默认值更安全）。
     */
    private fun readRootForRead(): JSONObject? {
        return when (val result = readRoot()) {
            ReadResult.NotExists -> null
            is ReadResult.Parsed -> result.root
            is ReadResult.Corrupted -> null
        }
    }

    /**
     * 用 AtomicFile 原子写入 state root。
     *
     * #649 评论 5563333323 缺口 2：startWrite → finishWrite，失败 failWrite。
     * @return true 表示持久化成功；false 表示失败。
     */
    private fun writeRoot(root: JSONObject): Boolean {
        return try {
            stateFile.parentFile?.mkdirs()
            val os = stateAtomicFile.startWrite() as FileOutputStream
            try {
                os.write(root.toString().toByteArray(Charsets.UTF_8))
                stateAtomicFile.finishWrite(os)
                true
            } catch (e: IOException) {
                stateAtomicFile.failWrite(os)
                false
            }
        } catch (e: IOException) {
            false
        }
    }

    companion object {
        /** Issue #667：旧存储目录名，仅用于迁移。 */
        private const val LEGACY_STATE_DIR_NAME = "sujian-mirror"
        private const val STATE_FILE_NAME = "state.json"
        private const val PENDING_PUBLISH_FILE_NAME = "pending-publish.json"
        private const val PROJECTS_KEY = "projects"

        // #649 评论 5564820566 问题 5：零章节作品独立 project 状态
        private const val PUBLISHED_PROJECTS_KEY = "publishedProjectIds"
        private const val MANIFEST_URI_KEY = "manifestUri"
        private const val BACKEND_KEY = "backend"
        private const val TREE_URI_KEY = "treeUri"
        private const val BACKEND_VALUE_MEDIA_STORE = "media_store"
        private const val BACKEND_VALUE_DOCUMENT_TREE = "document_tree"
        private const val URI_KEY = "uri"
        private const val RELATIVE_PATH_KEY = "relativePath"
        private const val REVISION_KEY = "revision"
        private const val CONTENT_HASH_KEY = "contentHash"
        private const val COMMITTED_MANIFEST_JSON_KEY = "committedManifestJson"
        private const val COMMITTED_MANIFEST_HASH_KEY = "committedManifestHash"
    }
}
