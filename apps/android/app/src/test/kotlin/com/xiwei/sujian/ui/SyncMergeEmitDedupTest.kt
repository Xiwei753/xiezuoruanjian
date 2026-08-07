package com.xiwei.sujian.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 二：SyncMergeEmitDedup 契约测试。
 *
 * 每个章节只发射一次同一 fileHash 的 SyncMerged；章节提交（reset）后允许
 * 同一 hash 重新发射（重新进入章节时正文由 RepositoryLoaded 装载）。
 */
class SyncMergeEmitDedupTest {
    @Test
    fun firstEmit_isAllowedAndRecorded() {
        val dedup = SyncMergeEmitDedup()
        assertTrue("首次发射必须允许", dedup.shouldEmit("hash-1"))
        assertFalse("同一 hash 第二次发射必须被抑制", dedup.shouldEmit("hash-1"))
    }

    @Test
    fun newHash_isAllowedAfterSuppression() {
        val dedup = SyncMergeEmitDedup()
        assertTrue(dedup.shouldEmit("hash-1"))
        assertFalse(dedup.shouldEmit("hash-1"))
        assertTrue("新 hash 必须允许发射", dedup.shouldEmit("hash-2"))
        assertFalse(dedup.shouldEmit("hash-2"))
    }

    @Test
    fun reset_allowsReEmitOfSameHash() {
        val dedup = SyncMergeEmitDedup()
        assertTrue(dedup.shouldEmit("hash-1"))
        assertFalse(dedup.shouldEmit("hash-1"))
        dedup.reset()
        assertTrue("章节提交后同一 hash 必须允许重新发射", dedup.shouldEmit("hash-1"))
    }

    @Test
    fun emptyHash_isRejectedWithoutRecording() {
        val dedup = SyncMergeEmitDedup()
        assertFalse("空 hash 不得发射", dedup.shouldEmit(""))
        assertTrue("空 hash 不得污染去重状态", dedup.shouldEmit("hash-1"))
    }
}
