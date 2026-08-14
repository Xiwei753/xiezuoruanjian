package com.xiwei.sujian.feature.stats.data

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.editor.session.EditorContentDelta
import com.xiwei.sujian.feature.editor.session.statsCountsFor
import kotlinx.coroutines.CompletableDeferred
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
 * #624 评论11 第3/4项：
 * - 第3项：编辑器写事件走进程级串行 writer actor — 热路径只
 *   `trySend(Record)` 后立即返回；唯一 actor 在注入的 IO scope 串行调用
 *   StatsBridge，Flush(reply) 保证 Record/Flush 顺序由 Channel 决定；
 * - 第4项：cause → 各分类计数收成 [statsCountsFor] 一个 mapper —
 *   Paste 不再把净增字符算两遍（Core `net_delta_chars = inserted + pasted
 *   + ai_inserted - deleted`，粘贴 5 个字符记 inserted=5 又 pasted=5 会记成
 *   +10）；一次事件只应有一个来源的非零字段。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class WritingStatsWriterActorTest {
    private fun createRepo(): WritingStatsRepository {
        val bridge =
            AppServiceBridge(
                WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_624_stats_actor",
                    "/tmp/sujian_test_workspace_624_stats_actor",
                ),
            )
        return WritingStatsRepository(bridge.statsBridge, CoroutineScope(SupervisorJob() + Dispatchers.IO))
    }

    // ── 第3项：writer actor 队列语义 ──

    /**
     * 热路径 `recordWritingEvent` 只 enqueue 后立即返回：
     * 返回时不得已在调用线程同步完成写盘（revision 尚未递增 —
     * markChanged 只在 actor 处理成功后发生）。
     */
    @Test
    fun recordWritingEvent_enqueuesWithoutSynchronousWrite() =
        runTest {
            val repo = createRepo()
            val flushReply = CompletableDeferred<Unit>()

            repo.recordWritingEvent("d1", "p", "v", "c", "typing", 5, 0, 0, 0, 0, "s1")
            assertEquals(
                "#624 评论11 第3项：recordWritingEvent 只 enqueue — 返回时不得已同步写盘",
                0L,
                repo.revision.value,
            )

            repo.flushWritingStats(flushReply)
            flushReply.await()
        }

    /**
     * Record/Flush 顺序由唯一 Channel 决定：先入队的 Record 必须在该
     * Flush(reply) 之前被同一 actor 处理完（FIFO 串行）。
     */
    @Test
    fun flushReplyCompletesOnlyAfterPriorRecordsProcessed() =
        runTest {
            val repo = createRepo()
            val flushReply = CompletableDeferred<Unit>()

            repo.recordWritingEvent("d1", "p", "v", "c", "typing", 3, 0, 0, 0, 0, "s1")
            repo.recordWritingEvent("d1", "p", "v", "c", "pasted", 0, 0, 7, 0, 0, "s1")
            repo.flushWritingStats(flushReply)

            flushReply.await()
        }

    // ── 第4项：cause → 各分类计数 mapper ──

    /** 粘贴 5 个字符替换 2 个：inserted=0、pasted=5、deleted=2 → net=+3（不是 +8/+10）。 */
    @Test
    fun statsCountsFor_pasteIsCountedOnce() {
        val counts =
            statsCountsFor(EditorTransactionCauseDto.PASTE, EditorContentDelta(insertedChars = 5, deletedChars = 2))

        assertEquals(
            "#624 评论11 第4项：Paste 不得同时计 inserted 和 pasted（Core 净增会翻倍）",
            0,
            counts.insertedChars,
        )
        assertEquals(5, counts.pastedChars)
        assertEquals(2, counts.deletedChars)
        // Core WritingInputEvent::new(): net = inserted + pasted + ai_inserted - deleted
        assertEquals("粘贴 5 替换 2 → 净增必须 +3", 3, counts.insertedChars + counts.pastedChars - counts.deletedChars)
    }

    /** 全 cause 分类矩阵：一次事件只应有一个来源的非零字段。 */
    @Test
    fun statsCountsFor_matrixPerCause() {
        val typing =
            statsCountsFor(EditorTransactionCauseDto.TYPING, EditorContentDelta(insertedChars = 4, deletedChars = 1))
        assertEquals(4, typing.insertedChars)
        assertEquals(0, typing.pastedChars)
        assertEquals(1, typing.deletedChars)

        val typingCommit =
            statsCountsFor(
                EditorTransactionCauseDto.TYPING_COMMIT,
                EditorContentDelta(insertedChars = 4, deletedChars = 1),
            )
        assertEquals(4, typingCommit.insertedChars)
        assertEquals(0, typingCommit.pastedChars)

        val ime =
            statsCountsFor(
                EditorTransactionCauseDto.IME_COMPOSITION,
                EditorContentDelta(insertedChars = 4, deletedChars = 1),
            )
        assertEquals(4, ime.insertedChars)
        assertEquals(0, ime.pastedChars)

        val delete = statsCountsFor(EditorTransactionCauseDto.DELETE, EditorContentDelta(deletedChars = 6))
        assertEquals("Delete 不得计入 inserted", 0, delete.insertedChars)
        assertEquals("Delete 不得计入 pasted", 0, delete.pastedChars)
        assertEquals(6, delete.deletedChars)

        // Undo/Redo/Programmatic：按实际 delta 传 inserted/deleted；
        // source 继续由 writingEventSourceFrom 映射为非 HumanTyped。
        val undo =
            statsCountsFor(EditorTransactionCauseDto.UNDO, EditorContentDelta(deletedChars = 3, insertedChars = 1))
        assertEquals(1, undo.insertedChars)
        assertEquals(0, undo.pastedChars)
        assertEquals(3, undo.deletedChars)

        val redo = statsCountsFor(EditorTransactionCauseDto.REDO, EditorContentDelta(insertedChars = 3))
        assertEquals(3, redo.insertedChars)
        assertEquals(0, redo.pastedChars)

        val programmatic =
            statsCountsFor(
                EditorTransactionCauseDto.PROGRAMMATIC,
                EditorContentDelta(insertedChars = 9),
            )
        assertEquals(9, programmatic.insertedChars)
        assertEquals(0, programmatic.pastedChars)

        val load = statsCountsFor(EditorTransactionCauseDto.LOAD, EditorContentDelta())
        assertEquals(0, load.insertedChars)
        assertEquals(0, load.pastedChars)
    }
}
