package com.xiwei.sujian.data

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #600 评论 #5：应用级同步 interval/elapsed 纯函数契约测试。
 *
 * AutoSyncWorker.syncApp() 调用 shouldAppSyncNow → AutoSyncScheduler.shouldSyncByInterval
 * 判定是否到同步时间点。本测试固定纯函数契约，不依赖 Worker/Repository 基础设施。
 */
class AutoSyncAppIntervalContractTest {
    private val now = 1_000_000L

    @Test
    fun neverSynced_syncsImmediately() {
        assertTrue(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = 300L,
                lastSyncTime = null,
                nowEpochSeconds = now,
            ),
        )
        assertTrue(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = 300L,
                lastSyncTime = 0L,
                nowEpochSeconds = now,
            ),
        )
    }

    @Test
    fun withinInterval_skipsSync() {
        assertFalse(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = 300L,
                lastSyncTime = now - 100L,
                nowEpochSeconds = now,
            ),
        )
    }

    @Test
    fun atOrBeyondInterval_syncs() {
        assertTrue(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = 300L,
                lastSyncTime = now - 300L,
                nowEpochSeconds = now,
            ),
        )
        assertTrue(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = 300L,
                lastSyncTime = now - 600L,
                nowEpochSeconds = now,
            ),
        )
    }

    @Test
    fun nullOrZeroInterval_usesDefault300s() {
        // interval = null → 默认 300s
        assertFalse(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = null,
                lastSyncTime = now - 100L,
                nowEpochSeconds = now,
            ),
        )
        assertTrue(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = null,
                lastSyncTime = now - 300L,
                nowEpochSeconds = now,
            ),
        )
        // interval = 0 → 默认 300s
        assertFalse(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = 0L,
                lastSyncTime = now - 100L,
                nowEpochSeconds = now,
            ),
        )
        // interval = -1 → 默认 300s
        assertFalse(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = -1L,
                lastSyncTime = now - 50L,
                nowEpochSeconds = now,
            ),
        )
    }

    @Test
    fun customInterval_respected() {
        // 自定义 60s interval
        assertTrue(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = 60L,
                lastSyncTime = now - 60L,
                nowEpochSeconds = now,
            ),
        )
        assertFalse(
            AutoSyncScheduler.shouldSyncByInterval(
                intervalSeconds = 60L,
                lastSyncTime = now - 30L,
                nowEpochSeconds = now,
            ),
        )
    }
}
