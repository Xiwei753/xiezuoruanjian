package com.xiwei.sujian.feature.stats.data

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
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
}
