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
import java.util.concurrent.atomic.AtomicLong
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * WritingStatsRepository — 统计仓库层
 *
 * 对统计领域 Bridge 的封装，提供统一的统计读取接口。
 * UI 层通过此 Repository 访问统计数据，不直接引用 AppServiceProvider 或 BridgeResult。
 *
 * #618 六：revision 只读计数 — 每次统计事件成功写入递增一次。统计页 ViewModel 用它判断
 * 是否真的需要重新查询（revision 未变则复用已加载数据，不再每次切回都重跑两遍 Core 查询）。
 * 统计数据只由本地写入事件产生（app-meta/stats 路径在 Core 同步中全量黑名单，同步不会
 * 替换统计文件），因此同步完成不需要额外 invalidate；若未来出现外部替换路径，
 * 在应用新数据的位置调用 [invalidate] 即可，它内部同样只递增 revision。
 *
 * #843：编辑器写事件改**进程级串行 writer actor**。
 * 输入热路径（IME/Key → Core EditResult → onEditorApplied）在 UI/input 主线程，
 * Core 事件直接追加写入 JSONL，无缓冲，不需要 flush。
 * 公开热路径只 `trySend(Record(...))` 后立即返回；唯一 actor 在注入的
 * [writerScope]（进程级 SupervisorJob + Dispatchers.IO，见 SujianAppDependencies）
 * 串行调用 [StatsBridge.recordEditorChangeStats]，成功后再 markChanged()。
 * Core 的 record_editor_change_stats 内部做 cause → EventSource 映射，
 * Android 不再自己拼 source/session_id/device_id。
 */
class WritingStatsRepository(
    private val statsBridge: StatsBridge,
    writerScope: CoroutineScope,
) {
    /** #843：writer actor 串行命令。 */
    sealed interface StatsWriteCommand {
        data class Record(val seq: Long, val event: PendingWritingEvent) : StatsWriteCommand

        /**
         * #843 三轮复核：查询 barrier — actor 处理到 Barrier 时，它前面的 Record 已全部执行完。
         * [barrierSeq] 是入队 Barrier 时的 seqCounter 快照，actor 据此判断 1..=barrierSeq 有无脏事件。
         * [ack] 完成时 true=干净 / false=有失败。
         */
        data class Barrier(val barrierSeq: Long, val ack: CompletableDeferred<Boolean>) : StatsWriteCommand
    }

    /** #843：待写事件负载（typed editor-change）。 */
    data class PendingWritingEvent(
        val projectId: String,
        val volumeId: String,
        val chapterId: String,
        val cause: EditorTransactionCauseDto,
        val insertedChars: Int,
        val deletedChars: Int,
    )

    private val _revision = MutableStateFlow(0L)

    /** 统计数据变更计数：事件成功写入即递增，供 UI 判断是否需要重新读取。 */
    val revision: StateFlow<Long> = _revision.asStateFlow()

    /** #843 三轮复核：单调递增序号，在 trySend 时分配（非 actor 处理时），保证 barrier 语义。 */
    private val seqCounter = AtomicLong(0L)

    /** #843 三轮复核：最早的失败序号（0 = 至今无失败）。只在 actor 中读写，无需同步。 */
    private var firstFailedSeq = 0L

    private val commands = Channel<StatsWriteCommand>(Channel.UNLIMITED)

    init {
        writerScope.launch {
            for (cmd in commands) {
                when (cmd) {
                    is StatsWriteCommand.Record -> {
                        val result =
                            statsBridge.recordEditorChangeStats(
                                cmd.event.projectId,
                                cmd.event.volumeId,
                                cmd.event.chapterId,
                                cmd.event.cause,
                                cmd.event.insertedChars,
                                cmd.event.deletedChars,
                            )
                        if (result is BridgeResult.Success) {
                            markChanged()
                        } else {
                            // #843 三轮复核：保留最早失败序号，barrier 据此判断 1..=barrierSeq 有无脏事件。
                            if (firstFailedSeq == 0L) {
                                firstFailedSeq = cmd.seq
                            }
                        }
                    }
                    is StatsWriteCommand.Barrier -> {
                        // actor 处理到 Barrier 时，前面的 Record 已全部执行完（Channel FIFO）。
                        // 1..=barrierSeq 有脏事件 → false；否则 true。
                        val clean = firstFailedSeq == 0L || firstFailedSeq > cmd.barrierSeq
                        cmd.ack.complete(clean)
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
     * #843：编辑器写事件热路径 — 只 enqueue 后立即返回。
     * 唯一 actor 在注入的 IO scope 串行调用 StatsBridge，成功后才 markChanged()。
     * Core 的 record_editor_change_stats 内部做 cause → EventSource 映射，
     * 事件直接追加写入 JSONL，无缓冲，不需要 flush。
     */
    fun recordEditorChangeStats(
        projectId: String,
        volumeId: String,
        chapterId: String,
        cause: EditorTransactionCauseDto,
        insertedChars: Int,
        deletedChars: Int,
    ) {
        val seq = seqCounter.incrementAndGet()
        commands.trySend(
            StatsWriteCommand.Record(
                seq,
                PendingWritingEvent(
                    projectId = projectId,
                    volumeId = volumeId,
                    chapterId = chapterId,
                    cause = cause,
                    insertedChars = insertedChars,
                    deletedChars = deletedChars,
                ),
            ),
        )
    }

    /**
     * #843 三轮复核：等待所有已入队的 Record 写入完成。
     *
     * 把 Barrier 发进同一个 Channel，actor 串行处理到 Barrier 时前面的 Record 已全部执行完。
     * 返回 true=全部成功 / false=有失败（某条 Record 写 Core 失败）。
     * 不另开第二个 channel，不轮询 revision；顺序由现有唯一 actor 保证。
     * 语义与 Harmony 的 `SerialStatsDrain.flushThrough(barrier)` 一致。
     */
    suspend fun awaitPendingWrites(): Boolean {
        val barrierSeq = seqCounter.get()
        val ack = CompletableDeferred<Boolean>()
        commands.send(StatsWriteCommand.Barrier(barrierSeq, ack))
        return ack.await()
    }
}
