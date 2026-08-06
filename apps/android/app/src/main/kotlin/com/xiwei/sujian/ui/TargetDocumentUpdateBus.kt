package com.xiwei.sujian.ui

import com.xiwei.sujian.editor.v2.coordinator.TargetDocumentFact
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.filterNotNull
import java.util.concurrent.ConcurrentHashMap

/**
 * #595 二：按 target 分区的最新文档事实总线（带 replay 语义）。
 *
 * 每个 target 保存的是**完整文档事实**（[TargetDocumentFact]：text +
 * sourceVersion + baseVersion），不是"最后一个事件对象"：
 * - 新 collector 立即拿到该 target 的当前文档事实（不丢失加载/同步事件）；
 * - 只投递自己 target 的事实（分区隔离，跨 target 不投递）；
 * - 重放旧事实由 reducer 的 sourceVersion 幂等判断忽略（同版本重放不再次
 *   执行 reset 副作用）；本地 dirty 时走冲突路径，禁止直接 reset。
 *
 * 替代单消费者 Channel.receiveAsFlow()：章节快速重组或 collector 短暂重叠时，
 * 事件不再可能被错误页面取走。
 */
class TargetDocumentUpdateBus {

    private val flows = ConcurrentHashMap<String, MutableStateFlow<TargetDocumentFact?>>()

    /** 发布文档事实 — 只更新对应 target 的当前事实，任何 collector 都不会消费掉。 */
    fun emit(fact: TargetDocumentFact) {
        flows.computeIfAbsent(fact.targetId) { MutableStateFlow(null) }.value = fact
    }

    /** 指定 target 的文档事实流 — 新 collector 先收到该 target 的当前事实。 */
    fun updates(targetId: String): Flow<TargetDocumentFact> {
        val flow = flows.computeIfAbsent(targetId) { MutableStateFlow(null) }
        return flow.filterNotNull()
    }
}
