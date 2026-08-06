package com.xiwei.sujian.ui

import com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.filterNotNull
import java.util.concurrent.ConcurrentHashMap

/**
 * #595 二：按 target 分区的最新正文更新事件总线（带 replay 语义）。
 *
 * 替代单消费者 `Channel.receiveAsFlow()`：章节快速重组或 collector 短暂重叠时，
 * 事件不再可能被错误页面取走。每个 target 维护一个 latest-event StateFlow：
 * - 新 collector 立即收到该 target 的最新事件（不会丢失加载/同步事件）；
 * - 只接收自己 target 的事件，跨 target 事件不投递；
 * - 同 target 连续事件由 StateFlow 合并为最新值，旧事件由 reducer 的
 *   contentVersion 比较丢弃（幂等重放安全）。
 */
class TargetDocumentUpdateBus {

    private val flows = ConcurrentHashMap<String, MutableStateFlow<EditorDocumentUpdate?>>()

    /** 发布事件 — 只更新对应 target 的最新值，任何 collector 都不会消费掉事件。 */
    fun emit(update: EditorDocumentUpdate) {
        flows.computeIfAbsent(update.targetId) { MutableStateFlow(null) }.value = update
    }

    /** 指定 target 的更新流 — 新 collector 先收到该 target 的最新事件。 */
    fun updates(targetId: String): Flow<EditorDocumentUpdate> {
        val flow = flows.computeIfAbsent(targetId) { MutableStateFlow(null) }
        return flow.filterNotNull()
    }
}
