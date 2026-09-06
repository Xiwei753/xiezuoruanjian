package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ConcurrentLinkedQueue

/**
 * MirrorChangeSink — 镜像变更入口。
 *
 * #649 评论 5559759935 / 5560685734：接收业务变更通知（`chapterChanged`、
 * `projectStructureChanged`、`projectDeleted`、`everythingChanged`），
 * 把变更排队后异步发布到 Download/Sujian 镜像。
 *
 * 与 [com.xiwei.sujian.storage.recovery.RecoveryChangeSink] 区分：
 * - RecoveryChangeSink：恢复完成后刷新 UI/缓存（只读）
 * - MirrorChangeSink：业务变更后写 Download/Sujian（写镜像）
 *
 * ## 设计要点
 * - 异步：Bridge 在保存热路径不能同步写 Download；所有发布都进队列，
 *   由后台协程串行消费。
 * - 合并：短时间内多次 chapterChanged 合并成一次按项目发布，
 *   避免频繁 I/O。
 * - 幂等：发布失败可重试；manifest 与正文文件都是幂等写入。
 *
 * ## 使用
 * ```kotlin
 * // 在 ProjectBridge/ChapterBridge 成功后调用：
 * mirrorChangeSink.chapterChanged(projectId, volumeId, chapterId)
 * ```
 */
interface MirrorChangeSink {
    /**
     * 单章正文变更。
     *
     * 触发发布该章正文 + 更新 manifest。
     */
    fun chapterChanged(
        projectId: String,
        volumeId: String,
        chapterId: String,
    )

    /**
     * 项目结构变更（新建/重命名/删除 卷或章节、重新排序）。
     *
     * 触发发布该项目全部正文 + 完整 manifest。
     */
    fun projectStructureChanged(projectId: String)

    /**
     * 项目删除。
     *
     * 触发删除镜像中该项目目录 + 更新 manifest。
     */
    fun projectDeleted(projectId: String)

    /**
     * 全部变更（恢复完成、设置变更等）。
     *
     * 触发全量发布。
     */
    fun everythingChanged()

    /**
     * 关闭并取消待处理任务。
     */
    fun close()
}

/**
 * 默认实现：ConcurrentHashMap 脏标记 + Channel.CONFLATED 信号 + debounce。
 *
 * #649 评论 5560971132 修复 5：旧实现用 `Mutex + pendingTask + lastPublishTime` 做
 * 合并，存在两个问题：
 * 1. `lastPublishTime` 让合并窗口内的后续事件被静默丢弃（`return@launch` 不再调度），
 *    导致最后一次变更可能永远不发布。
 * 2. `pendingTask` 单值合并丢失并发到达的多项目事件。
 *
 * 新实现：
 * - [dirtyMap] 用 ConcurrentHashMap 累积脏项目/章节键，不丢事件。
 * - [deleteQueue] 用 ConcurrentLinkedQueue 单独保留删除事件（删除不能被 publish 吞掉）。
 * - [signal] 用 Channel.CONFLATED 合并信号：多次 trySend 只保留一个待处理信号。
 * - [workerLoop] 收到信号后 delay(debounceMs) 让后续事件合并进 map，再一次性处理。
 *
 * #649 评论 5561974464 问题 3：pendingPublish 没有恢复逻辑。
 * 在初始化时调用 [ReadableMirrorPublisher.recoverPendingPublishIfNeeded] 恢复未完成的发布。
 *
 * @param publisher 实际的发布器（注入以便测试）
 * @param debounceMs debounce 窗口（毫秒），默认 500ms。窗口内到达的多个事件
 *   合并进同一个 dirtyMap 快照，窗口结束后一次性发布。
 */
class DefaultMirrorChangeSink(
    private val publisher: ReadableMirrorPublisher,
    private val debounceMs: Long = 500L,
) : MirrorChangeSink {
    private val dirtyMap = ConcurrentHashMap<MirrorKey, DirtyEntry>()
    private val deleteQueue = ConcurrentLinkedQueue<DeleteEvent>()
    private val signal = Channel<Unit>(Channel.CONFLATED)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    init {
        // #649 评论 5562462046 问题 5：pending recovery 和正常 worker 必须串行。
        // 旧实现两个 scope.launch 并行，recoverPendingPublishIfNeeded() 和 workerLoop()
        // 可能同时改 state/journal/文件。改成同一个串行 worker：先恢复 pending，
        // 恢复完成前不启动新的镜像事务。
        scope.launch {
            try {
                publisher.recoverPendingPublishIfNeeded()
            } catch (e: Exception) {
                DiagnosticsLogger.e(TAG, "Failed to recover pending publish", e)
            }
            workerLoop()
        }
    }

    override fun chapterChanged(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        dirtyMap[MirrorKey(projectId, volumeId, chapterId)] = DirtyEntry(System.currentTimeMillis())
        signal.trySend(Unit)
    }

    override fun projectStructureChanged(projectId: String) {
        dirtyMap[MirrorKey(projectId, "", "")] = DirtyEntry(System.currentTimeMillis())
        signal.trySend(Unit)
    }

    override fun projectDeleted(projectId: String) {
        deleteQueue.add(DeleteEvent(projectId))
        // 删除项目时清掉该项目的脏标记，避免删除后又触发 publishProject
        dirtyMap.keys.removeAll { it.projectId == projectId }
        signal.trySend(Unit)
    }

    override fun everythingChanged() {
        dirtyMap.clear()
        dirtyMap[MirrorKey(WILDCARD_PROJECT, "", "")] = DirtyEntry(System.currentTimeMillis())
        signal.trySend(Unit)
    }

    override fun close() {
        scope.cancel()
    }

    /**
     * 后台消费循环：等信号 → debounce → 处理删除 → 处理 dirty → 补发信号。
     *
     * Channel.CONFLATED 保证：在 workerLoop delay 期间到达的多次 trySend 只积压一个信号，
     * delay 结束后一次性处理 dirtyMap 快照，自然合并。
     *
     * #649 评论 5561286861 第 1 点：处理结束后若 dirtyMap/deleteQueue 仍非空，
     * 说明处理期间又有新事件到达（且未被本轮精确移除覆盖），补发一轮信号，
     * 保证最后一次正文一定会有下一轮处理，不再依赖下一笔外部事件触发。
     */
    private suspend fun workerLoop() {
        while (true) {
            // 阻塞等信号（CONFLATED channel 的 receive 在空时挂起）
            signal.receive()
            // debounce：让后续事件合并进 dirtyMap
            delay(debounceMs)
            // 先处理删除队列（删除优先，避免删后又 publish）
            processDeletes()
            // 再处理 dirty 快照
            processDirtySnapshot()
            // 处理期间新到达的事件（精确移除后仍残留的新版本）补发一轮信号，
            // 保证不丢最后一次正文。
            if (dirtyMap.isNotEmpty() || deleteQueue.isNotEmpty()) {
                signal.trySend(Unit)
            }
        }
    }

    private suspend fun processDeletes() {
        // poll 是精确移除：只取走队列头部一个事件，处理期间新 add 的事件保留在队列里，
        // 由 workerLoop 末尾的补发信号触发下一轮。
        while (true) {
            val del = deleteQueue.poll() ?: break
            try {
                publisher.deleteProject(del.projectId)
            } catch (e: Exception) {
                DiagnosticsLogger.e(TAG, "deleteProject failed: ${del.projectId}", e)
            }
        }
    }

    private suspend fun processDirtySnapshot() {
        // #649 评论 5561286861 第 1 点：按快照条目精确移除。
        // 用 entries.map 拿到当前 (key, value) 对，再逐条 remove(key, value)。
        // ConcurrentHashMap.remove(key, value) 是原子的条件移除：
        // 只在该 key 当前仍映射到该 value 时才移除。
        // 因此处理期间新写入的更新版本（value 不同）会保留在 dirtyMap，
        // 由 workerLoop 末尾的补发信号触发下一轮，不再被 clear() 误删。
        val snapshot = dirtyMap.entries.map { it.key to it.value }
        if (snapshot.isEmpty()) return
        for ((key, value) in snapshot) {
            dirtyMap.remove(key, value)
        }
        val wildcardKey = MirrorKey(WILDCARD_PROJECT, "", "")
        // 通配键表示全量
        if (snapshot.any { it.first == wildcardKey }) {
            try {
                publisher.publishAll()
            } catch (e: Exception) {
                DiagnosticsLogger.e(TAG, "publishAll failed", e)
            }
            return
        }
        // 按项目去重发布
        val projectIds = snapshot.map { it.first.projectId }.distinct()
        for (pid in projectIds) {
            try {
                publisher.publishProject(pid)
            } catch (e: Exception) {
                DiagnosticsLogger.e(TAG, "publishProject failed: $pid", e)
            }
        }
    }

    companion object {
        private const val TAG = "DefaultMirrorChangeSink"
        private const val WILDCARD_PROJECT = "*"
    }
}

/** 脏标记键：projectId + volumeId + chapterId。volumeId/chapterId 为空表示项目级。 */
data class MirrorKey(
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
)

/** 脏条目：记录入队时间（供未来按时间窗口策略扩展）。 */
data class DirtyEntry(val timestamp: Long)

/** 删除事件：单独队列保留，不被 publish 吞掉。 */
data class DeleteEvent(val projectId: String)
