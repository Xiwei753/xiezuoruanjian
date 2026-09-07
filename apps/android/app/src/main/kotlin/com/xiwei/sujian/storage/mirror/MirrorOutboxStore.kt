package com.xiwei.sujian.storage.mirror

import android.content.Context
import androidx.core.util.AtomicFile
import org.json.JSONException
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.io.IOException

/**
 * MirrorOutboxStore — 镜像变更意图的持久化 outbox 存储。
 *
 * #649 评论 5575052682：当前 [DefaultMirrorChangeSink] 的 dirtyMap、deleteQueue、
 * Channel.CONFLATED 都是纯内存对象，进程死亡后变更意图丢失。本类提供持久化 outbox，
 * 让发布器在进程重启后仍能恢复待发布的项目。
 *
 * ## 存储位置
 * `context.noBackupFilesDir/sujian-mirror/outbox.json`（与 state.json 同一目录）。
 *
 * ## 线程安全
 * 所有公开方法用 [lock] 保护，保证多线程读写原子。文件 I/O 在锁内同步执行
 * （调用方在 IO 调度器上调用）。
 *
 * ## JSON 结构
 * ```json
 * {
 *   "dirtyProjects": ["proj-1", "proj-2"],
 *   "deleteTombstones": ["proj-3"],
 *   "fullDirty": false,
 *   "lastSignalTime": 1694123456789
 * }
 * ```
 *
 * ## 优先级规则
 * - **delete tombstone 优先**：同一 projectId 既有 dirty 又有 tombstone 时，tombstone 胜出。
 *   [getDirtyProjects()] 会排除 tombstone 项目，[getDeleteTombstones()] 单独返回。
 * - **全量脏标记优先**：fullDirty=true 时，[getDirtyProjects()] 返回空集，
 *   [isFullDirty()] 返回 true。调用方应优先处理全量发布。
 *
 * ## 使用约定
 * 1. 先调用本类方法持久化 outbox
 * 2. 再 signal 通知 worker（调用方负责发信号）
 * 3. publish 成功后调用 [clearDirty] 或 [clearAll] 清理
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

    /** 用 AtomicFile 做原子写入。 */
    private val outboxAtomicFile: AtomicFile by lazy { AtomicFile(outboxFile) }

    /**
     * 标记项目脏（需要重新发布）。
     *
     * 如果该项目已在 deleteTombstones 中，tombstone 优先，不添加 dirty。
     * 如果 fullDirty=true，不需要再标记单个项目。
     *
     * @return true 表示持久化成功；false 表示失败（调用方应停止本轮操作）。
     */
    fun markDirty(projectId: String): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            val dirty = ensureArray(root, DIRTY_PROJECTS_KEY)
            val tombstones = root.optJSONArray(DELETE_TOMBSTONES_KEY)
            // tombstone 优先：如果该项目已在 tombstone 中，不添加 dirty
            if (tombstones != null && containsString(tombstones, projectId)) {
                return true
            }
            // 如果 fullDirty=true，不需要再标记单个项目
            if (root.optBoolean(FULL_DIRTY_KEY, false)) {
                return true
            }
            // 去重：如果已在 dirty 中，不重复添加
            if (containsString(dirty, projectId)) {
                return true
            }
            dirty.put(projectId)
            return writeRoot(root)
        }
    }

    /**
     * 全量脏标记（所有项目都需要重新发布）。
     *
     * 设置 fullDirty=true，并清空 dirtyProjects（全量时不需要逐项记录）。
     * 保留 deleteTombstones（全量发布时仍应执行删除）。
     *
     * @return true 表示持久化成功；false 表示失败。
     */
    fun markDirtyAll(): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            root.put(FULL_DIRTY_KEY, true)
            // 全量时清空逐项 dirty，避免冗余
            root.remove(DIRTY_PROJECTS_KEY)
            return writeRoot(root)
        }
    }

    /**
     * 标记项目删除（tombstone 优先）。
     *
     * 添加 tombstone，并从 dirtyProjects 中移除该项目（删除优先于发布）。
     *
     * @return true 表示持久化成功；false 表示失败。
     */
    fun markDeleted(projectId: String): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            // 添加到 tombstone（去重）
            val tombstones = ensureArray(root, DELETE_TOMBSTONES_KEY)
            if (!containsString(tombstones, projectId)) {
                tombstones.put(projectId)
            }
            // 从 dirty 中移除：删除优先于发布
            val dirty = root.optJSONArray(DIRTY_PROJECTS_KEY)
            if (dirty != null) {
                removeString(dirty, projectId)
                // 如果 dirty 变空，移除字段
                if (dirty.length() == 0) {
                    root.remove(DIRTY_PROJECTS_KEY)
                }
            }
            return writeRoot(root)
        }
    }

    /**
     * 清除指定项目的脏标记（publish 成功后调用）。
     *
     * 从 dirtyProjects 中移除该项目，不影响 deleteTombstones。
     *
     * @return true 表示持久化成功或项目本就不在 dirty 中；false 表示失败。
     */
    fun clearDirty(projectId: String): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            val dirty = root.optJSONArray(DIRTY_PROJECTS_KEY) ?: return true
            removeString(dirty, projectId)
            if (dirty.length() == 0) {
                root.remove(DIRTY_PROJECTS_KEY)
            }
            return writeRoot(root)
        }
    }

    /**
     * 获取所有脏项目（包含 delete tombstone 排除后的项目）。
     *
     * - fullDirty=true 时返回空集（调用方应通过 [isFullDirty] 判断全量发布）。
     * - 返回的项目已排除 deleteTombstones（tombstone 优先）。
     *
     * @return 脏项目 ID 集合，失败时返回空集。
     */
    fun getDirtyProjects(): Set<String> {
        synchronized(lock) {
            val root = readRootForRead() ?: return emptySet()
            // 全量脏标记优先：返回空集，调用方走 isFullDirty
            if (root.optBoolean(FULL_DIRTY_KEY, false)) {
                return emptySet()
            }
            val dirty = root.optJSONArray(DIRTY_PROJECTS_KEY) ?: return emptySet()
            val tombstones = root.optJSONArray(DELETE_TOMBSTONES_KEY)
            val result = mutableSetOf<String>()
            for (i in 0 until dirty.length()) {
                val pid = dirty.optString(i)
                if (pid.isNotEmpty()) {
                    // 排除 tombstone 中的项目
                    if (tombstones == null || !containsString(tombstones, pid)) {
                        result.add(pid)
                    }
                }
            }
            return result
        }
    }

    /**
     * 获取所有删除 tombstone。
     *
     * @return 删除 tombstone 集合，失败时返回空集。
     */
    fun getDeleteTombstones(): Set<String> {
        synchronized(lock) {
            val root = readRootForRead() ?: return emptySet()
            val tombstones = root.optJSONArray(DELETE_TOMBSTONES_KEY) ?: return emptySet()
            val result = mutableSetOf<String>()
            for (i in 0 until tombstones.length()) {
                val pid = tombstones.optString(i)
                if (pid.isNotEmpty()) {
                    result.add(pid)
                }
            }
            return result
        }
    }

    /**
     * 是否有全量脏标记。
     *
     * @return true 表示需要全量发布；false 表示没有全量标记或读取失败。
     */
    fun isFullDirty(): Boolean {
        synchronized(lock) {
            val root = readRootForRead() ?: return false
            return root.optBoolean(FULL_DIRTY_KEY, false)
        }
    }

    /**
     * 清空所有 outbox（应用启动时 drain 完成后调用）。
     *
     * 清除 dirtyProjects、deleteTombstones、fullDirty，保留 lastSignalTime。
     *
     * @return true 表示清空成功；false 表示失败。
     */
    fun clearAll(): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            root.remove(DIRTY_PROJECTS_KEY)
            root.remove(DELETE_TOMBSTONES_KEY)
            root.put(FULL_DIRTY_KEY, false)
            return writeRoot(root)
        }
    }

    /**
     * 是否为空（无 dirty、无 tombstone、无全量标记）。
     *
     * @return true 表示 outbox 为空；false 表示有待处理项或读取失败（失败时视为非空）。
     */
    fun isEmpty(): Boolean {
        synchronized(lock) {
            val root = readRootForRead() ?: return false
            if (root.optBoolean(FULL_DIRTY_KEY, false)) {
                return false
            }
            val dirty = root.optJSONArray(DIRTY_PROJECTS_KEY)
            val tombstones = root.optJSONArray(DELETE_TOMBSTONES_KEY)
            return (dirty == null || dirty.length() == 0) &&
                (tombstones == null || tombstones.length() == 0)
        }
    }

    /**
     * 记录上次发送信号的时间（用于调试）。
     *
     * 调用方在 signal 成功后调用此方法记录时间戳。
     *
     * @return true 表示持久化成功；false 表示失败。
     */
    fun recordSignalTime(): Boolean {
        synchronized(lock) {
            val root = readRootForUpdate() ?: return false
            root.put(LAST_SIGNAL_TIME_KEY, System.currentTimeMillis())
            return writeRoot(root)
        }
    }

    /**
     * 获取上次发送信号的时间（用于调试）。
     *
     * @return 时间戳（毫秒），未记录或失败时返回 0。
     */
    fun getLastSignalTime(): Long {
        synchronized(lock) {
            val root = readRootForRead() ?: return 0L
            return root.optLong(LAST_SIGNAL_TIME_KEY, 0L)
        }
    }

    // ── 内部 JSON 操作 ──

    /**
     * 确保 root 中存在指定 key 的 JSONArray，不存在则创建空数组。
     */
    private fun ensureArray(
        root: JSONObject,
        key: String,
    ): org.json.JSONArray {
        val array = root.optJSONArray(key)
        if (array != null) {
            return array
        }
        val newArr = org.json.JSONArray()
        root.put(key, newArr)
        return newArr
    }

    /**
     * 检查 JSONArray 中是否包含指定字符串值。
     */
    private fun containsString(
        array: org.json.JSONArray,
        value: String,
    ): Boolean {
        for (i in 0 until array.length()) {
            if (array.optString(i) == value) {
                return true
            }
        }
        return false
    }

    /**
     * 从 JSONArray 中移除指定字符串值（如果存在）。
     */
    private fun removeString(
        array: org.json.JSONArray,
        value: String,
    ) {
        // JSONArray 没有按值删除，需要遍历找索引
        val idx = (0 until array.length()).firstOrNull { array.optString(it) == value } ?: return
        // JSONArray.remove(int index) 在 API 19+ 可用
        array.remove(idx)
    }

    // ── 文件读写 ──

    /**
     * 读 root 用于更新；损坏时返回 null（调用方应停止本轮操作）。
     * 文件不存在时返回空 JSONObject（首次启动）。
     */
    private fun readRootForUpdate(): JSONObject? {
        return when (val result = readRoot()) {
            is ReadResult.NotExists -> JSONObject()
            is ReadResult.Parsed -> result.root
            is ReadResult.Corrupted -> null
        }
    }

    /**
     * 读 root 用于只读查询；损坏时返回 null（调用方返回默认值）。
     */
    private fun readRootForRead(): JSONObject? {
        return when (val result = readRoot()) {
            is ReadResult.NotExists -> null
            is ReadResult.Parsed -> result.root
            is ReadResult.Corrupted -> null
        }
    }

    /**
     * 读取 outbox root。
     */
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

    /**
     * 用 AtomicFile 原子写入 outbox root。
     *
     * @return true 表示持久化成功；false 表示失败。
     */
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

    /**
     * 读取结果（与 ReadableMirrorStateStore 保持一致的密封类）。
     */
    private sealed class ReadResult {
        object NotExists : ReadResult()

        data class Parsed(val root: JSONObject) : ReadResult()

        data class Corrupted(val error: Exception) : ReadResult()
    }

    companion object {
        private const val DIR_NAME = "sujian-mirror"
        private const val OUTBOX_FILE_NAME = "outbox.json"
        private const val DIRTY_PROJECTS_KEY = "dirtyProjects"
        private const val DELETE_TOMBSTONES_KEY = "deleteTombstones"
        private const val FULL_DIRTY_KEY = "fullDirty"
        private const val LAST_SIGNAL_TIME_KEY = "lastSignalTime"
    }
}
