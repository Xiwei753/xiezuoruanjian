package com.xiwei.sujian.storage.mirror

import android.content.Context
import androidx.core.util.AtomicFile
import org.json.JSONException
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.io.IOException

/**
 * 镜像 outbox 中每个项目的操作意图类型。
 */
enum class OutboxIntentKind {
    UPSERT,
    DELETE,
}

/**
 * 单个项目在 outbox 中的意图，带代次号用于并发 ACK。
 *
 * @property projectId 项目 ID。
 * @property generation 递增代次号，每次 markDirty/markDeleted 递增。
 * @property kind 操作类型：UPSERT（脏发布）或 DELETE（删除 tombstone）。
 */
data class OutboxProjectIntent(
    val projectId: String,
    val generation: Long,
    val kind: OutboxIntentKind,
)

/**
 * 从磁盘读取的完整 outbox 快照。
 *
 * @property nextGeneration 下一个待分配的代次号。
 * @property projects 所有项目的意图（含 dirty 和 tombstone），key 为 projectId。
 * @property fullDirtyGeneration 全量脏标记的代次号（null 表示无全量标记）。
 */
data class OutboxSnapshot(
    val nextGeneration: Long,
    val projects: Map<String, OutboxProjectIntent>,
    val fullDirtyGeneration: Long?,
    val lastSignalTime: Long = 0L,
)

/**
 * MirrorOutboxStore — 带代次号的持久化 outbox 存储。
 *
 * #649 评论 5575551884 问题 1：旧 outbox 用 `Set<String>` 保存 projectId，
 * markDirty 去重（projectId 已存在时直接 return true，outbox 不发生任何变化），
 * 导致并发保存时 R2 的意图无法持久化，R1 ACK 后 outbox 清空，进程死亡后 R2 丢失。
 *
 * 新实现：
 * - 每个项目的意图带递增 [generation] 号。
 * - [markDirty] / [markDeleted] 每次调用都推进代次。
 * - [ackProject] / [ackFullDirty] 只在"当前磁盘 generation/kind 仍然等于
 *   本轮处理的那一份"时才清除；处理期间来了更晚的保存（generation 已变），
 *   本轮 ACK 什么都不删。
 *
 * ## 存储位置
 * `context.noBackupFilesDir/sujian-mirror/outbox.json`。
 *
 * ## 线程安全
 * 所有公开方法用 [lock] 保护，保证多线程读写原子。
 *
 * ## JSON 结构
 * ```json
 * {
 *   "nextGeneration": 5,
 *   "projects": {
 *     "proj-1": { "generation": 2, "kind": "upsert" },
 *     "proj-3": { "generation": 1, "kind": "delete" }
 *   },
 *   "fullDirtyGeneration": null,
 *   "lastSignalTime": 1694123456789
 * }
 * ```
 *
 * ## 旧格式迁移
 * 首次读取时检测旧格式（`dirtyProjects`/`deleteTombstones`/`fullDirty`）
 * 并自动迁移为新格式。
 */
class MirrorOutboxStore(
    private val context: Context,
) {
    private val lock = Any()

    private val outboxFile: File by lazy {
        File(File(context.noBackupFilesDir, DIR_NAME), OUTBOX_FILE_NAME).also { file ->
            file.parentFile?.mkdirs()
        }
    }

    private val outboxAtomicFile: AtomicFile by lazy { AtomicFile(outboxFile) }

    /**
     * 标记项目脏（需要重新发布），推进代次。
     *
     * @return 新的 [OutboxProjectIntent]（含递增代次）；null 表示持久化失败。
     */
    fun markDirty(projectId: String): OutboxProjectIntent? {
        synchronized(lock) {
            val snapshot = readSnapshotForUpdate() ?: return null
            // tombstone 优先：如果该项目已是 DELETE 意图，不添加 dirty
            val existing = snapshot.projects[projectId]
            if (existing != null && existing.kind == OutboxIntentKind.DELETE) {
                return existing
            }
            val generation = snapshot.nextGeneration
            val intent = OutboxProjectIntent(projectId, generation, OutboxIntentKind.UPSERT)
            val projects = snapshot.projects.toMutableMap()
            projects[projectId] = intent
            val newSnapshot = OutboxSnapshot(
                nextGeneration = generation + 1,
                projects = projects,
                fullDirtyGeneration = snapshot.fullDirtyGeneration,
                lastSignalTime = snapshot.lastSignalTime,
            )
            return if (writeSnapshot(newSnapshot)) intent else null
        }
    }

    /**
     * 标记全量脏（所有项目都需要重新发布），推进代次。
     *
     * @return 新的全量代次号；null 表示持久化失败。
     */
    fun markDirtyAll(): Long? {
        synchronized(lock) {
            val snapshot = readSnapshotForUpdate() ?: return null
            val generation = snapshot.nextGeneration
            val newSnapshot = OutboxSnapshot(
                nextGeneration = generation + 1,
                projects = snapshot.projects,
                fullDirtyGeneration = generation,
                lastSignalTime = snapshot.lastSignalTime,
            )
            return if (writeSnapshot(newSnapshot)) generation else null
        }
    }

    /**
     * 标记项目删除（tombstone 优先），推进代次。
     *
     * @return 新的 [OutboxProjectIntent]（含递增代次）；null 表示持久化失败。
     */
    fun markDeleted(projectId: String): OutboxProjectIntent? {
        synchronized(lock) {
            val snapshot = readSnapshotForUpdate() ?: return null
            val generation = snapshot.nextGeneration
            val intent = OutboxProjectIntent(projectId, generation, OutboxIntentKind.DELETE)
            val projects = snapshot.projects.toMutableMap()
            projects[projectId] = intent
            val newSnapshot = OutboxSnapshot(
                nextGeneration = generation + 1,
                projects = projects,
                fullDirtyGeneration = snapshot.fullDirtyGeneration,
                lastSignalTime = snapshot.lastSignalTime,
            )
            return if (writeSnapshot(newSnapshot)) intent else null
        }
    }

    /**
     * ACK 单个项目：只有当前磁盘的 generation 和 kind 仍等于本轮处理的那一份时，
     * 才清除该条意图。处理期间来了更晚的保存（generation 已变），本轮 ACK 什么都不删。
     *
     * @return true 表示 ACK 成功（条目已清除）；false 表示 generation 不匹配或持久化失败。
     */
    fun ackProject(projectId: String, generation: Long, kind: OutboxIntentKind): Boolean {
        synchronized(lock) {
            val snapshot = readSnapshotForUpdate() ?: return false
            val current = snapshot.projects[projectId]
            if (current == null || current.generation != generation || current.kind != kind) {
                return false
            }
            val projects = snapshot.projects.toMutableMap()
            projects.remove(projectId)
            val newSnapshot = OutboxSnapshot(
                nextGeneration = snapshot.nextGeneration,
                projects = projects,
                fullDirtyGeneration = snapshot.fullDirtyGeneration,
                lastSignalTime = snapshot.lastSignalTime,
            )
            return writeSnapshot(newSnapshot)
        }
    }

    /**
     * ACK 全量脏标记：只有当前磁盘的 fullDirtyGeneration 仍等于本轮处理的那一份时，
     * 才清除全量标记。
     *
     * #649 评论 5575950895 问题 1：旧实现无条件 `projects = emptyMap()`，
     * 会删掉发布期间新到的 `intent.generation > generation` 的更晚事件。
     * 新实现只保留 generation 严格大于本轮 ACK generation 的项目，
     * 既清掉本轮全量覆盖的旧意图，又不误删更晚的新事件。
     *
     * 如果发布期间又来了新的 fullDirty，`fullDirtyGeneration` 已经变化，
     * 则 `snapshot.fullDirtyGeneration != generation` 校验失败，ACK 直接返回 false，
     * 保留新一代全量标记等待下一轮处理。
     *
     * @return true 表示 ACK 成功；false 表示 generation 不匹配或持久化失败。
     */
    fun ackFullDirty(generation: Long): Boolean {
        synchronized(lock) {
            val snapshot = readSnapshotForUpdate() ?: return false
            if (snapshot.fullDirtyGeneration != generation) {
                return false
            }
            // #649 评论 5575950895 问题 1：保留 generation > 本轮 generation 的更晚新事件，
            // 不再无条件清空 projects。
            val remainingProjects = snapshot.projects.filterValues { it.generation > generation }
            val newSnapshot = OutboxSnapshot(
                nextGeneration = snapshot.nextGeneration,
                projects = remainingProjects,
                fullDirtyGeneration = null,
                lastSignalTime = snapshot.lastSignalTime,
            )
            return writeSnapshot(newSnapshot)
        }
    }

    /**
     * 读取当前 outbox 快照（仅读，不修改）。
     *
     * @return 完整快照；失败时返回 null。
     */
    fun readSnapshot(): OutboxSnapshot? {
        synchronized(lock) {
            return readSnapshotForRead()
        }
    }

    /**
     * 是否为空（无 projects、无 fullDirty）。
     */
    fun isEmpty(): Boolean {
        synchronized(lock) {
            val snapshot = readSnapshotForRead() ?: return false
            return snapshot.projects.isEmpty() && snapshot.fullDirtyGeneration == null
        }
    }

    /**
     * 清空所有 outbox。
     */
    fun clearAll(): Boolean {
        synchronized(lock) {
            return writeSnapshot(OutboxSnapshot(1, emptyMap(), null))
        }
    }

    /**
     * 记录上次发送信号的时间（用于调试）。
     */
    fun recordSignalTime(): Boolean {
        synchronized(lock) {
            val root = readRootOrNull(JSONObject()) ?: return false
            root.put(LAST_SIGNAL_TIME_KEY, System.currentTimeMillis())
            return writeRoot(root)
        }
    }

    /**
     * 获取上次发送信号的时间（用于调试）。
     */
    fun getLastSignalTime(): Long {
        synchronized(lock) {
            val root = readRootOrNull() ?: return 0L
            return root.optLong(LAST_SIGNAL_TIME_KEY, 0L)
        }
    }

    // ── 内部方法 ──

    private fun readSnapshotForUpdate(): OutboxSnapshot? {
        return when (val result = readRoot()) {
            is ReadResult.NotExists -> OutboxSnapshot(1, emptyMap(), null)
            is ReadResult.Parsed -> parseSnapshot(result.root)
            is ReadResult.Corrupted -> null
        }
    }

    private fun readSnapshotForRead(): OutboxSnapshot? {
        return when (val result = readRoot()) {
            is ReadResult.NotExists -> null
            is ReadResult.Parsed -> parseSnapshot(result.root)
            is ReadResult.Corrupted -> null
        }
    }

    /**
     * 从 JSON root 解析快照，自动迁移旧格式。
     */
    private fun parseSnapshot(root: JSONObject): OutboxSnapshot {
        if (root.has(NEXT_GENERATION_KEY)) {
            return parseNewFormat(root)
        }
        // 旧格式迁移
        return migrateLegacy(root)
    }

    private fun parseNewFormat(root: JSONObject): OutboxSnapshot {
        val nextGeneration = root.optLong(NEXT_GENERATION_KEY, 1L)
        val fullDirtyGeneration = if (root.has(FULL_DIRTY_GENERATION_KEY)) {
            root.optLong(FULL_DIRTY_GENERATION_KEY, 0L).takeIf { it > 0 }
        } else {
            null
        }
        val projectsObj = root.optJSONObject(PROJECTS_KEY)
        val projects = mutableMapOf<String, OutboxProjectIntent>()
        if (projectsObj != null) {
            val keys = projectsObj.keys()
            while (keys.hasNext()) {
                val pid = keys.next()
                val intentObj = projectsObj.optJSONObject(pid) ?: continue
                val gen = intentObj.optLong("generation", 1L)
                val kindStr = intentObj.optString("kind", "upsert")
                val kind = when (kindStr) {
                    "delete" -> OutboxIntentKind.DELETE
                    else -> OutboxIntentKind.UPSERT
                }
                projects[pid] = OutboxProjectIntent(pid, gen, kind)
            }
        }
        val signalTime = root.optLong(LAST_SIGNAL_TIME_KEY, 0L)
        return OutboxSnapshot(nextGeneration, projects, fullDirtyGeneration, signalTime)
    }

    /**
     * 旧格式迁移：`dirtyProjects: [...], deleteTombstones: [...], fullDirty: bool`
     * → 新格式：`nextGeneration: Long, projects: {...}, fullDirtyGeneration: Long?`
     *
     * 旧 dirtyProjects 的项给 generation=1 UPSERT，
     * 旧 deleteTombstones 的项给 generation=1 DELETE，
     * 旧 fullDirty=true 给 fullDirtyGeneration=1。
     */
    private fun migrateLegacy(root: JSONObject): OutboxSnapshot {
        val projects = mutableMapOf<String, OutboxProjectIntent>()
        var gen: Long = 1

        val dirtyArray = root.optJSONArray(LEGACY_DIRTY_PROJECTS_KEY)
        if (dirtyArray != null) {
            for (i in 0 until dirtyArray.length()) {
                val pid = dirtyArray.optString(i)
                if (pid.isNotEmpty()) {
                    projects[pid] = OutboxProjectIntent(pid, gen++, OutboxIntentKind.UPSERT)
                }
            }
        }

        val tombstoneArray = root.optJSONArray(LEGACY_DELETE_TOMBSTONES_KEY)
        if (tombstoneArray != null) {
            for (i in 0 until tombstoneArray.length()) {
                val pid = tombstoneArray.optString(i)
                if (pid.isNotEmpty()) {
                    // tombstone 优先：覆盖 dirty
                    projects[pid] = OutboxProjectIntent(pid, gen++, OutboxIntentKind.DELETE)
                }
            }
        }

        val fullDirty = root.optBoolean(LEGACY_FULL_DIRTY_KEY, false)
        val signalTime = root.optLong(LAST_SIGNAL_TIME_KEY, 0L)

        return OutboxSnapshot(gen, projects, if (fullDirty) 1L else null, signalTime)
    }

    private fun writeSnapshot(snapshot: OutboxSnapshot): Boolean {
        val root = JSONObject()
        root.put(NEXT_GENERATION_KEY, snapshot.nextGeneration)
        if (snapshot.fullDirtyGeneration != null) {
            root.put(FULL_DIRTY_GENERATION_KEY, snapshot.fullDirtyGeneration)
        }
        val projectsObj = JSONObject()
        for ((pid, intent) in snapshot.projects) {
            val intentObj = JSONObject()
            intentObj.put("generation", intent.generation)
            intentObj.put("kind", when (intent.kind) {
                OutboxIntentKind.UPSERT -> "upsert"
                OutboxIntentKind.DELETE -> "delete"
            })
            projectsObj.put(pid, intentObj)
        }
        root.put(PROJECTS_KEY, projectsObj)
        // 保留 lastSignalTime
        if (snapshot.lastSignalTime > 0) {
            root.put(LAST_SIGNAL_TIME_KEY, snapshot.lastSignalTime)
        }
        return writeRoot(root)
    }

    private fun readRoot(): ReadResult {
        if (!outboxFile.exists()) return ReadResult.NotExists
        return try {
            ReadResult.Parsed(JSONObject(outboxAtomicFile.readFully().toString(Charsets.UTF_8)))
        } catch (e: IOException) {
            ReadResult.Corrupted(e)
        } catch (e: JSONException) {
            ReadResult.Corrupted(e)
        }
    }

    private fun readRootOrNull(notExistsDefault: JSONObject? = null): JSONObject? {
        return when (val result = readRoot()) {
            is ReadResult.NotExists -> notExistsDefault
            is ReadResult.Parsed -> result.root
            is ReadResult.Corrupted -> null
        }
    }

    private fun writeRoot(root: JSONObject): Boolean {
        return try {
            outboxFile.parentFile?.mkdirs()
            val os = outboxAtomicFile.startWrite() as FileOutputStream
            try {
                os.write(root.toString().toByteArray(Charsets.UTF_8))
                outboxAtomicFile.finishWrite(os)
                true
            } catch (e: IOException) {
                outboxAtomicFile.failWrite(os)
                false
            }
        } catch (e: IOException) {
            false
        }
    }

    private sealed class ReadResult {
        object NotExists : ReadResult()
        data class Parsed(val root: JSONObject) : ReadResult()
        data class Corrupted(val error: Exception) : ReadResult()
    }

    companion object {
        private const val DIR_NAME = "sujian-mirror"
        private const val OUTBOX_FILE_NAME = "outbox.json"
        private const val NEXT_GENERATION_KEY = "nextGeneration"
        private const val PROJECTS_KEY = "projects"
        private const val FULL_DIRTY_GENERATION_KEY = "fullDirtyGeneration"
        private const val LAST_SIGNAL_TIME_KEY = "lastSignalTime"

        // 旧格式 key（用于迁移）
        private const val LEGACY_DIRTY_PROJECTS_KEY = "dirtyProjects"
        private const val LEGACY_DELETE_TOMBSTONES_KEY = "deleteTombstones"
        private const val LEGACY_FULL_DIRTY_KEY = "fullDirty"
    }
}
