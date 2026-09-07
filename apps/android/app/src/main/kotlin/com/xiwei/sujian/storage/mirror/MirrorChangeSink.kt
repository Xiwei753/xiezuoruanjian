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
 * #649 评论 5575551884 问题 1：旧实现的 dirtyMap 只保存 projectId（无 generation），
 * processDirtySnapshot 无条件 clearDirty(pid) 会清掉"处理期间新到的 R2"。
 *
 * 新实现：
 * - [DirtyEntry] 带 [generation] + [kind]，与 outbox 的代次对齐。
 * - processDirtySnapshot/ackProject 只在 generation 未变时 ACK。
 * - drainOutboxToMemory 从 outbox snapshot 完整恢复 intent（含 fullDirty + tombstone）。
 * - processDeletes 成功后用 ackProject 清 tombstone（不再用 clearDirty）。
 * - publishAll 成功后用 ackFullDirty 清 fullDirty（不再只清内存 wildcard）。
 */
interface MirrorChangeSink {
    fun chapterChanged(projectId: String, volumeId: String, chapterId: String)
    fun projectStructureChanged(projectId: String)
    fun projectDeleted(projectId: String)
    fun everythingChanged()
    fun close()
    fun getDirtyCount(): Int
}

/**
 * 带代次号的脏标记条目（#649 评论 5575551884 问题 1）。
 *
 * @property timestamp 入队时间。
 * @property generation outbox 中该意图的代次号，ACK 时校验。
 * @property kind 操作类型：UPSERT 或 DELETE。
 */
data class DirtyEntry(
    val timestamp: Long,
    val generation: Long = 0L,
    val kind: OutboxIntentKind = OutboxIntentKind.UPSERT,
)

/**
 * 删除事件：带 generation，ACK 时校验。
 */
data class DeleteEvent(
    val projectId: String,
    val generation: Long = 0L,
)

/**
 * 默认实现：ConcurrentHashMap 脏标记 + Channel.CONFLATED 信号 + debounce。
 *
 * #649 评论 5575551884 问题 1：generation-aware ACK。
 * - dirtyMap / deleteQueue 中每个条目带 outbox 的 generation。
 * - processDirtySnapshot 成功后用 ackProject(generation) 精确清除。
 * - publishAll 成功后用 ackFullDirty(generation) 清除全量标记。
 * - processDeletes 成功后用 ackProject(generation) 清除 tombstone。
 * - drainOutboxToMemory 从 snapshot 完整恢复所有 intent（含 fullDirty + tombstone）。
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

    /** 从 outbox 加载的 fullDirty 代次号，ACK 时校验。 */
    @Volatile
    private var loadedFullDirtyGeneration: Long? = null

    init {
        scope.launch {
            try {
                publisher.recoverPendingPublishIfNeeded()
                drainOutboxToMemory()
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
        val intent = outboxStore.markDirty(projectId)
        if (intent != null) {
            dirtyMap[key] = DirtyEntry(
                timestamp = System.currentTimeMillis(),
                generation = intent.generation,
                kind = OutboxIntentKind.UPSERT,
            )
            outboxStore.recordSignalTime()
            signal.trySend(Unit)
        } else {
            DiagnosticsLogger.e(TAG, "Failed to write outbox for chapterChanged: $projectId")
        }
    }

    override fun projectStructureChanged(projectId: String) {
        val intent = outboxStore.markDirty(projectId)
        if (intent != null) {
            dirtyMap[MirrorKey(projectId, "", "")] = DirtyEntry(
                timestamp = System.currentTimeMillis(),
                generation = intent.generation,
                kind = OutboxIntentKind.UPSERT,
            )
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
        val intent = outboxStore.markDeleted(projectId)
        if (intent != null) {
            // 重新加入 deleteQueue 带 generation
            deleteQueue.poll() // 移除不带 generation 的那个
            deleteQueue.add(DeleteEvent(projectId, intent.generation))
            outboxStore.recordSignalTime()
            signal.trySend(Unit)
        } else {
            DiagnosticsLogger.e(TAG, "Failed to write outbox for projectDeleted: $projectId")
        }
    }

    override fun everythingChanged() {
        dirtyMap.clear()
        val fullGen = outboxStore.markDirtyAll()
        if (fullGen != null) {
            dirtyMap[MirrorKey(WILDCARD_PROJECT, "", "")] = DirtyEntry(
                timestamp = System.currentTimeMillis(),
                generation = fullGen,
                kind = OutboxIntentKind.UPSERT,
            )
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
     * 启动时把持久化的 outbox 完整 snapshot 加载到内存（#649 评论 5575551884 问题 1）。
     *
     * 修复：旧实现 `isFullDirty() { return }` 跳过 tombstone 加载，与 markDirtyAll 保留 tombstone 冲突。
     * 新实现：无论 fullDirty 状态，都加载所有 intent（UPSERT + DELETE）。
     */
    private suspend fun drainOutboxToMemory() {
        val snapshot = outboxStore.readSnapshot() ?: return

        // 保存 fullDirty 代次号，ACK 时校验
        loadedFullDirtyGeneration = snapshot.fullDirtyGeneration

        // 加载全量标记
        if (snapshot.fullDirtyGeneration != null) {
            dirtyMap[MirrorKey(WILDCARD_PROJECT, "", "")] = DirtyEntry(
                timestamp = System.currentTimeMillis(),
                generation = snapshot.fullDirtyGeneration,
                kind = OutboxIntentKind.UPSERT,
            )
        }

        // 加载所有项目意图（无论 fullDirty 状态）
        for ((pid, intent) in snapshot.projects) {
            when (intent.kind) {
                OutboxIntentKind.UPSERT -> {
                    dirtyMap[MirrorKey(pid, "", "")] = DirtyEntry(
                        timestamp = System.currentTimeMillis(),
                        generation = intent.generation,
                        kind = OutboxIntentKind.UPSERT,
                    )
                }
                OutboxIntentKind.DELETE -> {
                    deleteQueue.add(DeleteEvent(pid, intent.generation))
                }
            }
        }

        if (dirtyMap.isNotEmpty() || deleteQueue.isNotEmpty()) {
            signal.trySend(Unit)
        }
    }

    /**
     * 后台消费循环。
     *
     * #649 评论 5575551884 问题 1：ACK 使用 generation 精确匹配。
     */
    private suspend fun workerLoop() {
        while (true) {
            signal.receive()
            delay(debounceMs)
            processDeletes()
            processDirtySnapshot()
            if (dirtyMap.isNotEmpty() || deleteQueue.isNotEmpty()) {
                signal.trySend(Unit)
            }
        }
    }

    private suspend fun processDeletes() {
        while (true) {
            val del = deleteQueue.peek() ?: break
            val result = publisher.deleteProject(del.projectId)
            when (result) {
                is MirrorPublishResult.Committed -> {
                    deleteQueue.poll()
                    // #649 评论 5575551884：用 generation-aware ACK 清 tombstone
                    if (del.generation > 0) {
                        outboxStore.ackProject(del.projectId, del.generation, OutboxIntentKind.DELETE)
                    }
                }
                is MirrorPublishResult.PendingRecovery,
                is MirrorPublishResult.RetryableFailure,
                -> {
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
        val snapshot = dirtyMap.entries.map { it.key to it.value }
        if (snapshot.isEmpty()) return
        val wildcardKey = MirrorKey(WILDCARD_PROJECT, "", "")

        // 通配键表示全量
        if (snapshot.any { it.first == wildcardKey }) {
            val result = publisher.publishAll()
            when (result) {
                is MirrorPublishResult.Committed -> {
                    dirtyMap.remove(wildcardKey, snapshot.first { it.first == wildcardKey }.second)
                    // #649 评论 5575551884：用 generation-aware ACK 清 fullDirty
                    val fullGen = loadedFullDirtyGeneration
                    if (fullGen != null) {
                        outboxStore.ackFullDirty(fullGen)
                        loadedFullDirtyGeneration = null
                    }
                }
                is MirrorPublishResult.PendingRecovery,
                is MirrorPublishResult.RetryableFailure,
                -> {
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
            val projectEntries = snapshot.filter { it.first.projectId == pid }
            val result = publisher.publishProject(pid)
            when (result) {
                is MirrorPublishResult.Committed -> {
                    for ((key, value) in projectEntries) {
                        dirtyMap.remove(key, value)
                    }
                    // #649 评论 5575551884：用 generation-aware ACK
                    // 只清除仍在脏 map 中且 generation 未变的项目
                    val entry = dirtyMap.entries.firstOrNull { it.key.projectId == pid }
                    if (entry == null) {
                        // 项目已全部移除：用处理时的 generation ACK
                        // （处理期间没新 dirty 到来，generation 未变）
                        val processedEntry = projectEntries.firstOrNull()?.second
                        if (processedEntry != null && processedEntry.generation > 0) {
                            outboxStore.ackProject(pid, processedEntry.generation, OutboxIntentKind.UPSERT)
                        }
                    }
                    // 如果 entry 仍在 map 中（处理期间有新 dirty），generation 已变，ACK 会失败，保留新 dirty
                }
                is MirrorPublishResult.PendingRecovery,
                is MirrorPublishResult.RetryableFailure,
                -> {
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
