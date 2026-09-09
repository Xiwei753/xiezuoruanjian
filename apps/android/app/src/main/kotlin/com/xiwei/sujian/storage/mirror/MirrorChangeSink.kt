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
 * DefaultMirrorChangeSink — ConcurrentHashMap 脏标记 + Channel.CONFLATED 信号 + debounce。
 *
 * #649 评论 5575551884 问题 1：generation-aware ACK。
 * - dirtyMap / deleteQueue 中每个条目带 outbox 的 generation。
 * - processDirtySnapshot 成功后用 ackProject(generation) 精确清除。
 * - publishAll 成功后用 ackFullDirty(generation) 清除全量标记。
 * - processDeletes 成功后用 ackProject(generation) 清除 tombstone。
 * - drainOutboxToMemory 从 snapshot 完整恢复所有 intent（含 fullDirty + tombstone）。
 *
 * #649 评论 5575950895 问题 1/2：
 * - 不再用全局 `loadedFullDirtyGeneration` 做全量 ACK。运行期 `everythingChanged()`
 *   根本没给它赋值，导致运行期 fullDirty 成功后无法 ACK。直接用本轮 snapshot 中
 *   wildcard 的 `DirtyEntry.generation` 做 ACK，发布期间若来了新一代 wildcard，
 *   `remove(key, oldValue)` 和 `ackFullDirty(oldGeneration)` 都不会误删新事件。
 * - `projectDeleted()` 改为先 durable outbox 成功再改内存，不再"先放占位再 poll"
 *   误删队头其他删除事件。
 * - `chapterChanged()` / `projectStructureChanged()` 用 `intent.kind` 而非硬编码
 *   `OutboxIntentKind.UPSERT`，避免磁盘里项目已是 DELETE 时仍硬塞 UPSERT。
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
        val intent = outboxStore.markDirty(projectId) ?: run {
            DiagnosticsLogger.e(TAG, "Failed to write outbox for chapterChanged: $projectId")
            return
        }
        // #649 评论 5576464076 问题 4.1：DELETE tombstone 不进 dirtyMap。
        // markDirty 遇到已有 DELETE 返回已有 DELETE intent；此时不应再 publish 该项目。
        if (intent.kind == OutboxIntentKind.DELETE) {
            signal.trySend(Unit)
            return
        }
        dirtyMap[key] = DirtyEntry(
            timestamp = System.currentTimeMillis(),
            generation = intent.generation,
            // #649 评论 5575950895 问题 2：用 intent.kind 而非硬编码 UPSERT。
            // 若磁盘里项目已是 DELETE，markDirty 返回已有 DELETE intent，
            // 调用方不能再把它硬塞成 UPSERT。
            kind = intent.kind,
        )
        outboxStore.recordSignalTime()
        signal.trySend(Unit)
    }

    override fun projectStructureChanged(projectId: String) {
        val intent = outboxStore.markDirty(projectId) ?: run {
            DiagnosticsLogger.e(TAG, "Failed to write outbox for projectStructureChanged: $projectId")
            return
        }
        // #649 评论 5576464076 问题 4.1：DELETE tombstone 不进 dirtyMap。
        if (intent.kind == OutboxIntentKind.DELETE) {
            signal.trySend(Unit)
            return
        }
        dirtyMap[MirrorKey(projectId, "", "")] = DirtyEntry(
            timestamp = System.currentTimeMillis(),
            generation = intent.generation,
            // #649 评论 5575950895 问题 2：用 intent.kind 而非硬编码 UPSERT。
            kind = intent.kind,
        )
        outboxStore.recordSignalTime()
        signal.trySend(Unit)
    }

    override fun projectDeleted(projectId: String) {
        // #649 评论 5575950895 问题 2：先 durable outbox 成功再改内存。
        // 旧实现"先 add 占位再 poll"会 poll 掉队头其他删除事件（ConcurrentLinkedQueue FIFO）。
        val intent = outboxStore.markDeleted(projectId) ?: run {
            DiagnosticsLogger.e(TAG, "Failed to write outbox for projectDeleted: $projectId")
            return
        }

        // durable outbox 已成功，再改内存：清掉该项目的脏标记，加入带 generation 的删除事件。
        dirtyMap.keys.removeAll { it.projectId == projectId }
        deleteQueue.add(DeleteEvent(projectId, intent.generation))
        outboxStore.recordSignalTime()
        signal.trySend(Unit)
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
     *
     * #649 评论 5575950895 问题 1：不再保存 `loadedFullDirtyGeneration` 全局变量。
     * 运行期 `everythingChanged()` 不给它赋值，导致运行期 fullDirty 成功后无法 ACK。
     * 改用本轮 snapshot 中 wildcard 的 `DirtyEntry.generation` 做 ACK（见 processDirtySnapshot）。
     */
    private suspend fun drainOutboxToMemory() {
        val snapshot = outboxStore.readSnapshot() ?: return

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
            processWildcardSnapshot(snapshot, wildcardKey)
            return
        }

        // 按项目去重发布
        processPerProjectSnapshot(snapshot)
    }

    /**
     * 处理全量（wildcard）快照。
     *
     * #649 评论 5575950895 问题 1：用本轮 snapshot 中 wildcard 的 DirtyEntry.generation
     * 做 ACK，不再用全局 loadedFullDirtyGeneration（运行期 everythingChanged 不给它赋值）。
     * 发布期间若来了新一代 wildcard，remove(key, oldValue) 和 ackFullDirty(oldGeneration)
     * 都不会误删新事件。
     */
    private suspend fun processWildcardSnapshot(
        snapshot: List<Pair<MirrorKey, DirtyEntry>>,
        wildcardKey: MirrorKey,
    ) {
        val processedWildcard = snapshot.first { it.first == wildcardKey }.second
        val result = publisher.publishAll()
        when (result) {
            is MirrorPublishResult.Committed -> {
                dirtyMap.remove(wildcardKey, processedWildcard)
                // #649 评论 5575551884：用 generation-aware ACK 清 fullDirty
                if (processedWildcard.generation > 0) {
                    outboxStore.ackFullDirty(processedWildcard.generation)
                }
            }
            is MirrorPublishResult.PendingRecovery,
            is MirrorPublishResult.RetryableFailure,
            -> {
                DiagnosticsLogger.w(TAG, "publishAll pending/failed, keeping event for retry")
                signal.trySend(Unit)
            }
        }
    }

    /** 按项目去重发布。 */
    private suspend fun processPerProjectSnapshot(snapshot: List<Pair<MirrorKey, DirtyEntry>>) {
        val projectIds = snapshot.map { it.first.projectId }.distinct()
        for (pid in projectIds) {
            val projectEntries = snapshot.filter { it.first.projectId == pid }
            val result = publisher.publishProject(pid)
            when (result) {
                is MirrorPublishResult.Committed -> ackCommittedProject(pid, projectEntries)
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

    /**
     * ACK 已成功提交的项目：清除脏标记 + generation-aware ACK outbox。
     *
     * #649 评论 5575551884：用 generation-aware ACK，只清除仍在脏 map 中且 generation 未变的项目。
     * #649 评论 5576464076 问题 4.2：取 maxOf 而非 firstOrNull，避免多个 dirty key
     * （如 gen 10 + gen 11）时 ACK 拿到旧的 gen 10 失败。
     */
    private fun ackCommittedProject(
        pid: String,
        projectEntries: List<Pair<MirrorKey, DirtyEntry>>,
    ) {
        for ((key, value) in projectEntries) {
            dirtyMap.remove(key, value)
        }
        val entry = dirtyMap.entries.firstOrNull { it.key.projectId == pid }
        if (entry == null) {
            // 项目已全部移除：用本轮 snapshot 的最大 generation ACK
            val processedGeneration = projectEntries.maxOf { it.second.generation }
            if (processedGeneration > 0) {
                outboxStore.ackProject(pid, processedGeneration, OutboxIntentKind.UPSERT)
            }
        }
        // 如果 entry 仍在 map 中（处理期间有新 dirty），generation 已变，ACK 会失败，保留新 dirty
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
