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

    /**
     * 获取当前脏项目数量（用于测试/调试）。
     */
    fun getDirtyCount(): Int
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
    private val outboxStore: MirrorOutboxStore,
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
        // #649 评论 5575052682 问题 1：启动时先恢复 pending publish，再 drain outbox
        scope.launch {
            try {
                // 1. 先恢复 pending publish（如果有）
                publisher.recoverPendingPublishIfNeeded()
                
                // 2. Drain outbox：把持久化的 outbox 任务加载到内存 dirtyMap
                drainOutboxToMemory()
                
                // 3. 启动 worker loop
                workerLoop()
            } catch (e: Exception) {
                DiagnosticsLogger.e(TAG, "Failed to initialize MirrorChangeSink", e)
            }
        }
    }

    override fun chapterChanged(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        val key = MirrorKey(projectId, volumeId, chapterId)
        dirtyMap[key] = DirtyEntry(System.currentTimeMillis())
        // #649 评论 5575052682 问题 1：先持久化 outbox，再 signal
        if (outboxStore.markDirty(projectId)) {
            outboxStore.recordSignalTime()
            signal.trySend(Unit)
        } else {
            DiagnosticsLogger.e(TAG, "Failed to write outbox for chapterChanged: $projectId")
        }
    }

    override fun projectStructureChanged(projectId: String) {
        dirtyMap[MirrorKey(projectId, "", "")] = DirtyEntry(System.currentTimeMillis())
        // #649 评论 5575052682 问题 1：先持久化 outbox，再 signal
        if (outboxStore.markDirty(projectId)) {
            outboxStore.recordSignalTime()
            signal.trySend(Unit)
        } else {
            DiagnosticsLogger.e(TAG, "Failed to write outbox for projectStructureChanged: $projectId")
        }
    }

    override fun projectDeleted(projectId: String) {
        deleteQueue.add(DeleteEvent(projectId))
        // 删除项目时清掉该项目的脏标记，避免删除后又触发 publishProject
        dirtyMap.keys.removeAll { it.projectId == projectId }
        // #649 评论 5575052682 问题 1：先持久化 tombstone，再 signal
        // tombstone 优先：delete 必须压过同项目的 upsert
        if (outboxStore.markDeleted(projectId)) {
            outboxStore.recordSignalTime()
            signal.trySend(Unit)
        } else {
            DiagnosticsLogger.e(TAG, "Failed to write outbox for projectDeleted: $projectId")
        }
    }

    override fun everythingChanged() {
        dirtyMap.clear()
        dirtyMap[MirrorKey(WILDCARD_PROJECT, "", "")] = DirtyEntry(System.currentTimeMillis())
        // #649 评论 5575052682 问题 1：先持久化全量脏标记，再 signal
        if (outboxStore.markDirtyAll()) {
            outboxStore.recordSignalTime()
            signal.trySend(Unit)
        } else {
            DiagnosticsLogger.e(TAG, "Failed to write outbox for everythingChanged")
        }
    }

    override fun close() {
        scope.cancel()
    }

    override fun getDirtyCount(): Int = dirtyMap.size + deleteQueue.size

    /**
     * 启动时把持久化的 outbox 加载到内存 dirtyMap（#649 评论 5575052682 问题 1）。
     *
     * 进程重启后，从 outbox.json 读取所有脏项目和删除 tombstone，
     * 加载到内存 dirtyMap/deleteQueue，让 worker 继续处理。
     */
    private suspend fun drainOutboxToMemory() {
        if (outboxStore.isFullDirty()) {
            dirtyMap[MirrorKey(WILDCARD_PROJECT, "", "")] = DirtyEntry(System.currentTimeMillis())
            signal.trySend(Unit)
            return
        }
        
        val dirtyProjects = outboxStore.getDirtyProjects()
        val tombstones = outboxStore.getDeleteTombstones()
        
        for (projectId in dirtyProjects) {
            dirtyMap[MirrorKey(projectId, "", "")] = DirtyEntry(System.currentTimeMillis())
        }
        
        for (projectId in tombstones) {
            deleteQueue.add(DeleteEvent(projectId))
        }
        
        if (dirtyProjects.isNotEmpty() || tombstones.isNotEmpty()) {
            signal.trySend(Unit)
        }
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
        // peek/commit 语义：先 peek 查看队首，处理成功后才 poll 移除。
        // PendingRecovery/RetryableFailure 时保留原事件并补发 signal，保证不丢删除事件。
        while (true) {
            val del = deleteQueue.peek() ?: break
            val result = publisher.deleteProject(del.projectId)
            when (result) {
                is MirrorPublishResult.Committed -> {
                    // 成功提交，移除已处理的事件
                    deleteQueue.poll()
                    // #649 评论 5575052682 问题 1：删除成功后清理 outbox tombstone
                    outboxStore.clearDirty(del.projectId)
                }
                is MirrorPublishResult.PendingRecovery,
                is MirrorPublishResult.RetryableFailure,
                -> {
                    // pending 恢复中或可重试失败：保留事件在队列中，补发信号触发下一轮
                    DiagnosticsLogger.w(
                        TAG,
                        "deleteProject pending/failed for ${del.projectId}, keeping event for retry",
                    )
                    signal.trySend(Unit)
                    return
                }
            }
        }
    }

    private suspend fun processDirtySnapshot() {
        // peek/commit 语义：先拍快照不 remove，Committed 后才 remove(key, value)。
        // PendingRecovery/RetryableFailure 保留原事件并补发 signal，保证不丢事件。
        val snapshot = dirtyMap.entries.map { it.key to it.value }
        if (snapshot.isEmpty()) return
        val wildcardKey = MirrorKey(WILDCARD_PROJECT, "", "")
        // 通配键表示全量
        if (snapshot.any { it.first == wildcardKey }) {
            val result = publisher.publishAll()
            when (result) {
                is MirrorPublishResult.Committed -> {
                    // 成功提交，移除已处理的通配键
                    dirtyMap.remove(wildcardKey, snapshot.first { it.first == wildcardKey }.second)
                }
                is MirrorPublishResult.PendingRecovery,
                is MirrorPublishResult.RetryableFailure,
                -> {
                    // pending 恢复中或可重试失败：保留事件在 dirtyMap 中，补发信号触发下一轮
                    DiagnosticsLogger.w(TAG, "publishAll pending/failed, keeping event for retry")
                    signal.trySend(Unit)
                    return
                }
            }
            return
        }
        // 按项目去重发布
        val projectIds = snapshot.map { it.first.projectId }.distinct()
        for (pid in projectIds) {
            // 收集该项目的所有条目
            val projectEntries = snapshot.filter { it.first.projectId == pid }
            val result = publisher.publishProject(pid)
            when (result) {
                is MirrorPublishResult.Committed -> {
                    // 成功提交，移除该项目已处理的条目
                    for ((key, value) in projectEntries) {
                        dirtyMap.remove(key, value)
                    }
                    // #649 评论 5575052682 问题 1：成功后清理 outbox
                    // 只清掉自己处理的那个 revision/版本，如果处理期间又来了更新，就保留更晚的 dirty
                    outboxStore.clearDirty(pid)
                }
                is MirrorPublishResult.PendingRecovery,
                is MirrorPublishResult.RetryableFailure,
                -> {
                    // pending 恢复中或可重试失败：保留事件在 dirtyMap 中，补发信号触发下一轮
                    DiagnosticsLogger.w(
                        TAG,
                        "publishProject pending/failed for $pid, keeping event for retry",
                    )
                    signal.trySend(Unit)
                    return
                }
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
