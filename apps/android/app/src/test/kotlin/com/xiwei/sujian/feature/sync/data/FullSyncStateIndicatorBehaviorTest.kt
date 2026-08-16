package com.xiwei.sujian.feature.sync.data

import com.xiwei.sujian.feature.sync.data.model.FullSyncState
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import com.xiwei.sujian.feature.sync.work.AutoSyncScheduler
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论 5307423953 Part B 行为测试：
 *
 * 1. [SyncStatusRepository.refreshState] 只看 [FullSyncState.overallStatus] 决定灯色，
 *    不再看 loadAppSyncState()。
 * 2. [AutoSyncScheduler.shouldSyncByInterval] 用 lastSuccessTime 判 interval —
 *    AutoSyncWorker.shouldSyncNow 现在传 fullState?.lastSuccessTime。
 *
 * 用 fake [SyncRepository] override loadSyncConfig/getSyncCapability/loadFullSyncState，
 * 不触发真实 native 调用。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class FullSyncStateIndicatorBehaviorTest {
    /**
     * Fake [SyncRepository]：override 三个读取方法返回预设值。
     * 构造时传 null context/bridge（不使用，只 override 方法）。
     */
    private class FakeSyncRepo(
        private val config: SyncConfig,
        private val capability: SyncCapabilityData,
        private val fullState: FullSyncState?,
    ) : SyncRepository(
            org.robolectric.RuntimeEnvironment.getApplication(),
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    appDataRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-part-b-data",
                    projectsRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-part-b-projects",
                ),
            ),
        ) {
        override fun loadSyncConfig(): SyncConfig = config

        override fun getSyncCapability(): SyncCapabilityData = capability

        override fun loadFullSyncState(): FullSyncState? = fullState
    }

    private fun enabledConfig() = SyncConfig(enabled = true, remoteUrl = "https://example.com/b.git")

    private fun canRunCapability() = SyncCapabilityData(canRun = true)

    @Test
    fun refreshState_fullStateSuccess_showsSynced() =
        runTest {
            val repo =
                FakeSyncRepo(
                    enabledConfig(),
                    canRunCapability(),
                    FullSyncState(SyncStatus.Success, 1000L, 1000L, emptyList()),
                )
            val statusRepo = SyncStatusRepository(repo)
            statusRepo.refreshState()
            assertEquals(SyncIndicatorState.Synced, statusRepo.state.value)
        }

    @Test
    fun refreshState_fullStateError_showsFailed() =
        runTest {
            val repo =
                FakeSyncRepo(
                    enabledConfig(),
                    canRunCapability(),
                    FullSyncState(SyncStatus.Error, 1000L, 500L, listOf("project:p1")),
                )
            val statusRepo = SyncStatusRepository(repo)
            statusRepo.refreshState()
            assertEquals(SyncIndicatorState.Failed, statusRepo.state.value)
        }

    @Test
    fun refreshState_fullStateNull_showsUnconfigured() =
        runTest {
            // 从未同步过 — fullState 为 null
            val repo = FakeSyncRepo(enabledConfig(), canRunCapability(), null)
            val statusRepo = SyncStatusRepository(repo)
            statusRepo.refreshState()
            assertEquals(SyncIndicatorState.Unconfigured, statusRepo.state.value)
        }

    @Test
    fun refreshState_fullStatePartialConflict_showsFailed() =
        runTest {
            val repo =
                FakeSyncRepo(
                    enabledConfig(),
                    canRunCapability(),
                    FullSyncState(SyncStatus.PartialConflict, 1000L, 500L, listOf("project:p1")),
                )
            val statusRepo = SyncStatusRepository(repo)
            statusRepo.refreshState()
            assertEquals(SyncIndicatorState.Failed, statusRepo.state.value)
        }

    @Test
    fun refreshState_fullStateNoChanges_showsSynced() =
        runTest {
            val repo =
                FakeSyncRepo(
                    enabledConfig(),
                    canRunCapability(),
                    FullSyncState(SyncStatus.NoChanges, 1000L, 1000L, emptyList()),
                )
            val statusRepo = SyncStatusRepository(repo)
            statusRepo.refreshState()
            assertEquals(SyncIndicatorState.Synced, statusRepo.state.value)
        }

    @Test
    fun refreshState_configDisabled_showsUnconfigured() =
        runTest {
            val repo =
                FakeSyncRepo(
                    SyncConfig(enabled = false),
                    canRunCapability(),
                    FullSyncState(SyncStatus.Success, 1000L, 1000L, emptyList()),
                )
            val statusRepo = SyncStatusRepository(repo)
            statusRepo.refreshState()
            assertEquals(SyncIndicatorState.Unconfigured, statusRepo.state.value)
        }

    /**
     * interval 用 lastSuccessTime：上次全量部分失败（lastSuccessTime=500），
     * now=600，interval=300 → 600-500=100 < 300 → 不应同步。
     * 旧代码用 App target lastSyncTime（可能=1000，把失败当成功）会误判。
     */
    @Test
    fun shouldSyncByInterval_usesLastSuccessTime_notAppTargetLastSyncTime() {
        val lastSuccessTime = 500L
        val now = 600L
        val interval = 300L
        assertFalse(
            "部分失败后 lastSuccessTime=500，now=600，interval=300 → 100 < 300 不应同步",
            AutoSyncScheduler.shouldSyncByInterval(interval, lastSuccessTime, now),
        )
    }

    /**
     * interval 用 lastSuccessTime：上次全量整体成功（lastSuccessTime=500），
     * now=900，interval=300 → 900-500=400 >= 300 → 应同步。
     */
    @Test
    fun shouldSyncByInterval_syncsAfterIntervalSinceLastSuccess() {
        val lastSuccessTime = 500L
        val now = 900L
        val interval = 300L
        assertTrue(
            "lastSuccessTime=500，now=900，interval=300 → 400 >= 300 应同步",
            AutoSyncScheduler.shouldSyncByInterval(interval, lastSuccessTime, now),
        )
    }

    /**
     * lastSuccessTime 为 null（从未整体成功过）→ 应同步。
     */
    @Test
    fun shouldSyncByInterval_nullLastSuccessTime_syncs() {
        assertTrue(
            "lastSuccessTime=null → 从未整体成功，应同步",
            AutoSyncScheduler.shouldSyncByInterval(300L, null, 1000L),
        )
    }
}
