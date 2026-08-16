package com.xiwei.sujian.feature.sync.data

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.ResultEnvelope
import com.xiwei.sujian.feature.sync.data.interop.SyncBridge
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论 5308439467 Part 1 单元测试：[SyncRepository.recoverInterruptedFullSyncState]。
 *
 * 用 fake [SyncBridge] 覆盖 [SyncBridge.recoverInterruptedFullSyncState] 的三种
 * [BridgeResult] 分支，验证 Repository 行为：
 * - Success(true) → 返回 true；
 * - Success(false) → 返回 false（旧状态非 Syncing，Core 未改动）；
 * - Error → 返回 false 且不抛异常（只记日志，不阻断应用启动）；
 * - NotLoaded → 返回 false（原生库未加载）。
 *
 * 不触发真实 native 调用 — fake bridge 直接返回预设结果。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RecoverInterruptedFullSyncStateTest {
    /**
     * Fake [SyncBridge]：仅覆盖 [recoverInterruptedFullSyncState] 返回预设结果。
     *
     * 其他方法不 override，本测试只调 recover，不触及其他方法。
     */
    private class FakeSyncBridge(
        holder: WriterAppServiceHolder,
        private val result: BridgeResult<Boolean>,
    ) : SyncBridge(holder) {
        override fun recoverInterruptedFullSyncState(): BridgeResult<Boolean> = result
    }

    /**
     * Fake [AppServiceBridge]：覆盖 [syncBridge] 返回 [FakeSyncBridge]。
     */
    private class FakeAppServiceBridge(
        holder: WriterAppServiceHolder,
        syncBridge: SyncBridge,
    ) : AppServiceBridge(holder) {
        override val syncBridge: SyncBridge = syncBridge
    }

    private fun newRepo(result: BridgeResult<Boolean>): SyncRepository {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val holder =
            WriterAppServiceHolder(
                appDataRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-recover-sync-data",
                projectsRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-recover-sync-projects",
            )
        val fakeBridge = FakeAppServiceBridge(holder, FakeSyncBridge(holder, result))
        return SyncRepository(context, fakeBridge)
    }

    @Test
    fun recover_successTrue_returnsTrue() {
        val repo = newRepo(BridgeResult.Success(true))
        assertTrue(
            "BridgeResult.Success(true) 时 Repository 应返回 true（发生了恢复落盘）",
            repo.recoverInterruptedFullSyncState(),
        )
    }

    @Test
    fun recover_successFalse_returnsFalse() {
        val repo = newRepo(BridgeResult.Success(false))
        assertFalse(
            "BridgeResult.Success(false) 时 Repository 应返回 false（旧状态非 Syncing，未恢复）",
            repo.recoverInterruptedFullSyncState(),
        )
    }

    @Test
    fun recover_error_returnsFalseAndDoesNotThrow() {
        val repo =
            newRepo(
                BridgeResult.Error(
                    ResultEnvelope.errorOf("IO_ERROR", "disk read failed"),
                ),
            )
        // 失败只记日志，不抛异常，不阻断应用启动。
        assertFalse(
            "BridgeResult.Error 时 Repository 应返回 false 且不抛异常",
            repo.recoverInterruptedFullSyncState(),
        )
    }

    @Test
    fun recover_notLoaded_returnsFalse() {
        val repo = newRepo(BridgeResult.NotLoaded)
        assertFalse(
            "BridgeResult.NotLoaded 时 Repository 应返回 false（原生库未加载）",
            repo.recoverInterruptedFullSyncState(),
        )
    }
}
