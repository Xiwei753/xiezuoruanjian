package com.xiwei.sujian.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 七：Flush 持久化屏障契约测试 — 替代旧全局 lastSaveResult。
 *
 * 规则（issue 解决七）：Flush(targetId, sessionId, requiredRustRevision,
 * requiredDocumentVersion, reply) 只有确认该 revision 对应正文已经得到保存回执
 * 才能返回成功；删除跨章节全局 lastSaveResult。空正文不能靠字符串猜测 —
 * 删除全部正文产生类型化 ClearDocument 操作并记录回执。
 */
class DocumentSaveReceiptTrackerTest {

    @Test
    fun noReceipt_flushFails() {
        val tracker = DocumentSaveReceiptTracker()
        assertFalse(
            "从未保存过的 target 不得报告假成功",
            tracker.canFlush("t1", requiredRevision = 0L, committedContentHash = null),
        )
    }

    @Test
    fun receiptMatchingRevision_flushSucceeds() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record("t1", revision = 5L, contentHash = "hash-5")
        assertTrue(tracker.canFlush("t1", requiredRevision = 5L, committedContentHash = "hash-5"))
    }

    @Test
    fun revisionMovedPastReceipt_flushFails() {
        // 保存后又输入：屏幕 revision 6 ≠ 回执 revision 5 → 不得放行同步。
        val tracker = DocumentSaveReceiptTracker()
        tracker.record("t1", revision = 5L, contentHash = "hash-5")
        assertFalse(
            "保存后新输入未落盘时 flush 必须失败",
            tracker.canFlush("t1", requiredRevision = 6L, committedContentHash = "hash-5"),
        )
    }

    @Test
    fun committedHashMismatch_flushFails() {
        // 版本事实已被外部推进但磁盘未同步保存（回执仍是旧 hash）→ flush 失败。
        val tracker = DocumentSaveReceiptTracker()
        tracker.record("t1", revision = 5L, contentHash = "hash-5")
        assertFalse(
            "committedVersion 与回执 hash 不一致时 flush 必须失败",
            tracker.canFlush("t1", requiredRevision = 5L, committedContentHash = "hash-other"),
        )
    }

    @Test
    fun emptyCommittedHash_skipsHashCheck() {
        // 版本事实尚未建立（空 committed）→ 只验证 revision 回执。
        val tracker = DocumentSaveReceiptTracker()
        tracker.record("t1", revision = 3L, contentHash = "hash-3")
        assertTrue(tracker.canFlush("t1", requiredRevision = 3L, committedContentHash = null))
        assertTrue(tracker.canFlush("t1", requiredRevision = 3L, committedContentHash = ""))
    }

    @Test
    fun perTargetIsolation() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record("t1", revision = 5L, contentHash = "hash-5")
        assertFalse(
            "其他 target 的回执不得放行本 target 的 flush（跨章节假成功防护）",
            tracker.canFlush("t2", requiredRevision = 5L, committedContentHash = null),
        )
        tracker.record("t2", revision = 1L, contentHash = "hash-1")
        assertTrue(tracker.canFlush("t2", requiredRevision = 1L, committedContentHash = "hash-1"))
        assertTrue(tracker.canFlush("t1", requiredRevision = 5L, committedContentHash = "hash-5"))
    }

    @Test
    fun clearRemovesReceipt() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record("t1", revision = 5L, contentHash = "hash-5")
        tracker.clear("t1")
        assertNull(tracker.receipt("t1"))
        assertFalse(tracker.canFlush("t1", requiredRevision = 5L, committedContentHash = null))
    }

    @Test
    fun clearDocumentRecordsReceiptLikeSave() {
        // 删除全部正文（ClearDocument）同样记录回执 — 同步前 flush 不再使用磁盘旧正文。
        val tracker = DocumentSaveReceiptTracker()
        tracker.record("t1", revision = 9L, contentHash = "hash-empty")
        assertTrue(tracker.canFlush("t1", requiredRevision = 9L, committedContentHash = "hash-empty"))
        val receipt = tracker.receipt("t1")
        assertEquals(9L, receipt!!.revision)
        assertEquals("hash-empty", receipt.contentHash)
    }

    @Test
    fun lastReceiptWins() {
        val tracker = DocumentSaveReceiptTracker()
        tracker.record("t1", revision = 1L, contentHash = "hash-1")
        tracker.record("t1", revision = 2L, contentHash = "hash-2")
        assertFalse(tracker.canFlush("t1", requiredRevision = 1L, committedContentHash = "hash-2"))
        assertTrue(tracker.canFlush("t1", requiredRevision = 2L, committedContentHash = "hash-2"))
    }
}
