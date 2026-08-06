package com.xiwei.sujian.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 七/四：Flush 持久化屏障契约测试 — 替代旧全局 lastSaveResult。
 *
 * 规则（issue 解决七/四）：Flush 只有确认该 token 对应正文已经得到保存回执
 * 才能返回成功；删除跨章节全局 lastSaveResult。SaveToken 携带完整文档身份
 * （target/session/epoch/revision/hash），不只比较 revision 数字。
 */
class DocumentSaveReceiptTrackerTest {

    private fun token(
        targetId: String = "t1",
        coreSessionId: ULong = 1UL,
        inputEpoch: Long = 0L,
        rustRevision: Long = 0L,
        textHash: String = "",
    ) = DocumentSaveReceiptTracker.SaveToken(
        operationId = 0L,
        targetId = targetId,
        coreSessionId = coreSessionId,
        inputEpoch = inputEpoch,
        rustRevision = rustRevision,
        textHash = textHash,
    )

    @Test
    fun noReceipt_flushFails() {
        val tracker = DocumentSaveReceiptTracker()
        assertFalse(
            "从未保存过的 target 不得报告假成功",
            tracker.canFlush(token(targetId = "t1"), committedContentHash = null),
        )
    }

    @Test
    fun receiptMatchingToken_flushSucceeds() {
        val tracker = DocumentSaveReceiptTracker()
        val t = token(rustRevision = 5L, textHash = "hash-5")
        tracker.record(t)
        assertTrue(tracker.canFlush(t, committedContentHash = "hash-5"))
    }

    @Test
    fun revisionMovedPastReceipt_flushFails() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record(token(rustRevision = 5L, textHash = "hash-5"))
        assertFalse(
            "保存后新输入未落盘时 flush 必须失败",
            tracker.canFlush(token(rustRevision = 6L, textHash = "hash-5"), committedContentHash = "hash-5"),
        )
    }

    @Test
    fun committedHashMismatch_flushFails() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record(token(rustRevision = 5L, textHash = "hash-5"))
        assertFalse(
            "committedVersion 与回执 hash 不一致时 flush 必须失败",
            tracker.canFlush(token(rustRevision = 5L, textHash = "hash-5"), committedContentHash = "hash-other"),
        )
    }

    @Test
    fun emptyCommittedHash_skipsHashCheck() {
        val tracker = DocumentSaveReceiptTracker()
        val t = token(rustRevision = 3L, textHash = "hash-3")
        tracker.record(t)
        assertTrue(tracker.canFlush(t, committedContentHash = null))
        assertTrue(tracker.canFlush(t, committedContentHash = ""))
    }

    @Test
    fun perTargetIsolation() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record(token(targetId = "t1", rustRevision = 5L, textHash = "hash-5"))
        assertFalse(
            "其他 target 的回执不得放行本 target 的 flush（跨章节假成功防护）",
            tracker.canFlush(token(targetId = "t2", rustRevision = 5L), committedContentHash = null),
        )
        val t2 = token(targetId = "t2", coreSessionId = 2UL, rustRevision = 1L, textHash = "hash-1")
        tracker.record(t2)
        assertTrue(tracker.canFlush(t2, committedContentHash = "hash-1"))
        assertTrue(tracker.canFlush(token(targetId = "t1", rustRevision = 5L, textHash = "hash-5"), committedContentHash = "hash-5"))
    }

    @Test
    fun clearRemovesReceipt() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record(token(targetId = "t1", rustRevision = 5L, textHash = "hash-5"))
        tracker.clear("t1")
        assertNull(tracker.receipt("t1"))
        assertFalse(tracker.canFlush(token(targetId = "t1", rustRevision = 5L), committedContentHash = null))
    }

    @Test
    fun clearDocumentRecordsReceiptLikeSave() {
        val tracker = DocumentSaveReceiptTracker()
        val t = token(rustRevision = 9L, textHash = "hash-empty")
        tracker.record(t)
        assertTrue(tracker.canFlush(t, committedContentHash = "hash-empty"))
        val receipt = tracker.receipt("t1")
        assertEquals(9L, receipt!!.rustRevision)
        assertEquals("hash-empty", receipt.textHash)
    }

    @Test
    fun lastReceiptWins() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record(token(rustRevision = 1L, textHash = "hash-1"))
        tracker.record(token(rustRevision = 2L, textHash = "hash-2"))
        assertFalse(tracker.canFlush(token(rustRevision = 1L, textHash = "hash-1"), committedContentHash = "hash-2"))
        assertTrue(tracker.canFlush(token(rustRevision = 2L, textHash = "hash-2"), committedContentHash = "hash-2"))
    }

    @Test
    fun differentSessionId_flushFails() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record(token(coreSessionId = 1UL, rustRevision = 5L, textHash = "hash-5"))
        assertFalse(
            "不同 Rust session 的 revision 数值可以相同，不得跨 session 假成功",
            tracker.canFlush(token(coreSessionId = 2UL, rustRevision = 5L, textHash = "hash-5"), committedContentHash = "hash-5"),
        )
    }

    @Test
    fun differentInputEpoch_flushFails() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record(token(inputEpoch = 0L, rustRevision = 5L, textHash = "hash-5"))
        assertFalse(
            "章节切换后旧保存结果不匹配新 epoch",
            tracker.canFlush(token(inputEpoch = 1L, rustRevision = 5L, textHash = "hash-5"), committedContentHash = "hash-5"),
        )
    }
}
