package com.xiwei.sujian.feature.stats.data

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * #843：编辑器写事件走进程级串行 writer actor — 热路径只
 * `trySend(Record)` 后立即返回；唯一 actor 在注入的 IO scope 串行调用
 * StatsBridge.recordEditorChangeStats。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class WritingStatsWriterActorTest {
    private fun createRepo(): WritingStatsRepository {
        val bridge =
            AppServiceBridge(
                WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_843_stats_actor",
                    "/tmp/sujian_test_workspace_843_stats_actor",
                ),
            )
        return WritingStatsRepository(bridge.statsBridge, CoroutineScope(SupervisorJob() + Dispatchers.IO))
    }

    /**
     * 热路径 `recordEditorChangeStats` 只 enqueue 后立即返回：
     * 返回时不得已在调用线程同步完成写盘（revision 尚未递增 —
     * markChanged 只在 actor 处理成功后发生）。
     */
    @Test
    fun recordEditorChangeStats_enqueuesWithoutSynchronousWrite() =
        runTest {
            val repo = createRepo()

            repo.recordEditorChangeStats(
                projectId = "p",
                volumeId = "v",
                chapterId = "c",
                cause = EditorTransactionCauseDto.TYPING,
                insertedChars = 5,
                deletedChars = 0,
            )
            assertEquals(
                "#843：recordEditorChangeStats 只 enqueue — 返回时不得已同步写盘",
                0L,
                repo.revision.value,
            )
        }

    /**
     * #843 复核评论 6045538399：没有任何待写 Record 时，barrier 必须直接回干净，
     * 统计页才能据此继续读 Core。
     */
    @Test
    fun awaitPendingWrites_returnsTrueWhenNothingIsPending() =
        runTest {
            val repo = createRepo()

            assertTrue(
                "空队列的 barrier 必须返回干净",
                repo.awaitPendingWrites(),
            )
        }

    /**
     * #843 复核评论 6045538399：barrier 必须等它前面的 Record 处理完，并按「1..=barrierSeq
     * 有没有失败」判成败。单测环境原生库未加载，Record 一定写 Core 失败，所以这里能稳定观察到
     * false —— 统计页据此显示读取失败，不能把缺数据的结果当完整结果。
     * 后续再来的 barrier 同样为 false：最早的失败不会被更晚的 barrier 洗掉（与 Harmony
     * `StatsBarrierLedger` 同口径，一条事件脏了就是永久缺失）。
     */
    @Test
    fun awaitPendingWrites_returnsFalseWhenAnEarlierRecordFailed() =
        runTest {
            val repo = createRepo()
            repo.recordEditorChangeStats(
                projectId = "p",
                volumeId = "v",
                chapterId = "c",
                cause = EditorTransactionCauseDto.TYPING,
                insertedChars = 5,
                deletedChars = 0,
            )

            assertFalse(
                "barrier 必须覆盖它前面的失败 Record，不能返回干净",
                repo.awaitPendingWrites(),
            )
            assertFalse(
                "更晚的 barrier 不能把更早的失败洗掉",
                repo.awaitPendingWrites(),
            )
        }
}
