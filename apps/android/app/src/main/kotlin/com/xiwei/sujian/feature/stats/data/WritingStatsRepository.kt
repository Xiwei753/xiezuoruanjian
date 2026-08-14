package com.xiwei.sujian.feature.stats.data
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.stats.data.interop.StatsBridge
import com.xiwei.sujian.feature.stats.data.model.ProjectWritingStatsSummary
import com.xiwei.sujian.feature.stats.data.model.WritingStatsSummary
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * WritingStatsRepository — 统计仓库层
 *
 * 对统计领域 Bridge 的封装，提供统一的统计读取接口。
 * UI 层通过此 Repository 访问统计数据，不直接引用 AppServiceProvider 或 BridgeResult。
 *
 * #602 Phase 5：从 ProjectRepository 移入 recordWritingEvent/processWritingEvent/flushWritingStats。
 *
 * #618 六：revision 只读计数 — 每次统计事件成功写入递增一次。统计页 ViewModel 用它判断
 * 是否真的需要重新查询（revision 未变则复用已加载数据，不再每次切回都重跑两遍 Core 查询）。
 * 统计数据只由本地写入事件产生（app-meta/stats 路径在 Core 同步中全量黑名单，同步不会
 * 替换统计文件），因此同步完成不需要额外 invalidate；若未来出现外部替换路径，
 * 在应用新数据的位置调用 [invalidate] 即可，它内部同样只递增 revision。
 *
 * #624 评论11 第3项：编辑器写事件改**进程级串行 writer actor**。
 * 输入热路径（IME/Key → Core EditResult → onEditorApplied）在 UI/input 主线程，
 * Rust StatsStore 在同一次调用里可能直接刷盘（last_flush_ms=0 首次必刷、之后
 * 每 3 秒/100 条触发 create_dir/open/writeln JSONL）— 不能留在输入调用栈里。
 * 公开热路径只 `trySend(Record(...))` 后立即返回；唯一 actor 在注入的
 * [writerScope]（进程级 SupervisorJob + Dispatchers.IO，见 SujianAppDependencies）
 * 串行调用 [StatsBridge.recordWritingEvent]，成功后再 markChanged()。
 * Record/Flush 顺序由同一个 Channel 决定，Rust StatsStore 继续负责缓存/JSONL
 * 顺序与 3 秒 flush 语义，但其磁盘操作不再发生在 UI/input thread。
 */
class WritingStatsRepository(
    private val statsBridge: StatsBridge,
    writerScope: CoroutineScope,
) {
    /** #624 评论11 第3项：writer actor 串行命令。 */
    sealed interface StatsWriteCommand {
        data class Record(val event: PendingWritingEvent) : StatsWriteCommand

        data class Flush(val reply: CompletableDeferred<Unit>? = null) : StatsWriteCommand
    }

    /** #624 评论11 第3项：待写事件负载（与 recordWritingEvent 参数一一对应）。 */
    data class PendingWritingEvent(
        val deviceId: String,
        val projectId: String,
        val volumeId: String,
        val chapterId: String,
        val source: String,
        val insertedChars: Int,
        val deletedChars: Int,
        val pastedChars: Int,
        val aiInsertedChars: Int,
        val durationSeconds: Int,
        val sessionId: String,
    )

    private val _revision = MutableStateFlow(0L)

    /** 统计数据变更计数：事件成功写入即递增，供 UI 判断是否需要重新读取。 */
    val revision: StateFlow<Long> = _revision.asStateFlow()

    private val commands = Channel<StatsWriteCommand>(Channel.UNLIMITED)

    init {
        writerScope.launch {
            for (cmd in commands) {
                when (cmd) {
                    is StatsWriteCommand.Record -> {
                        val result =
                            statsBridge.recordWritingEvent(
                                cmd.event.deviceId,
                                cmd.event.projectId,
                                cmd.event.volumeId,
                                cmd.event.chapterId,
                                cmd.event.source,
                                cmd.event.insertedChars,
                                cmd.event.deletedChars,
                                cmd.event.pastedChars,
                                cmd.event.aiInsertedChars,
                                cmd.event.durationSeconds,
                                cmd.event.sessionId,
                            )
                        if (result is BridgeResult.Success) {
                            markChanged()
                        }
                    }
                    is StatsWriteCommand.Flush -> {
                        statsBridge.flushWritingStats()
                        cmd.reply?.complete(Unit)
                    }
                }
            }
        }
    }

    private fun markChanged() {
        _revision.update { it + 1L }
    }

    /** 外部路径（如同步应用新数据）替换统计数据后调用 — 仅递增 revision。 */
    fun invalidate() {
        markChanged()
    }

    fun getWritingStatsSummary(
        startDate: String,
        endDate: String,
    ): WritingStatsSummary? {
        return when (val result = statsBridge.getWritingStatsSummary(startDate, endDate)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    fun getWritingStatsByProject(
        startDate: String,
        endDate: String,
    ): ProjectWritingStatsSummary? {
        return when (val result = statsBridge.getWritingStatsByProject(startDate, endDate)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    /**
     * #624 评论11 第3项：编辑器写事件热路径 — 只 enqueue 后立即返回。
     * 唯一 actor 在注入的 IO scope 串行调用 StatsBridge，成功后才 markChanged()。
     */
    fun recordWritingEvent(
        deviceId: String,
        projectId: String,
        volumeId: String,
        chapterId: String,
        source: String,
        insertedChars: Int,
        deletedChars: Int,
        pastedChars: Int,
        aiInsertedChars: Int,
        durationSeconds: Int,
        sessionId: String,
    ) {
        commands.trySend(
            StatsWriteCommand.Record(
                PendingWritingEvent(
                    deviceId = deviceId,
                    projectId = projectId,
                    volumeId = volumeId,
                    chapterId = chapterId,
                    source = source,
                    insertedChars = insertedChars,
                    deletedChars = deletedChars,
                    pastedChars = pastedChars,
                    aiInsertedChars = aiInsertedChars,
                    durationSeconds = durationSeconds,
                    sessionId = sessionId,
                ),
            ),
        )
    }

    /**
     * #624 评论11 第3项：flush 也走同一 writer actor 队列 — 保证在它之前入队的
     * Record 全部处理完；[reply] 供调用方等待（onCleared 不等待，直接入队）。
     */
    fun flushWritingStats(reply: CompletableDeferred<Unit>? = null) {
        commands.trySend(StatsWriteCommand.Flush(reply))
    }

    fun processWritingEvent(
        deviceId: String,
        platform: String,
        projectId: String,
        volumeId: String,
        chapterId: String,
        oldText: String,
        newText: String,
        durationSeconds: UInt,
        sessionId: String,
    ): BridgeResult<Boolean> {
        val result =
            statsBridge.processWritingEvent(
                deviceId, platform, projectId, volumeId, chapterId, oldText, newText,
                durationSeconds, sessionId,
            )
        if (result is BridgeResult.Success) {
            markChanged()
        }
        return result
    }
}
