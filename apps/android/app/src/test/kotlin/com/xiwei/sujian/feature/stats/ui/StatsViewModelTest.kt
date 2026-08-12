package com.xiwei.sujian.feature.stats.ui

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #618 六：StatsViewModel 契约测试。
 *
 * 单测环境无 Android native 库：Bridge 调用经 wrapResult 如实返回
 * BridgeResult.NotLoaded/Error，统计查询退化为空数据 — 与真机“原生库未加载”行为一致，
 * 不伪装成功。revision 机制（写入失败不递增、invalidate 递增）在仓库层直接验证。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class StatsViewModelTest {
    private fun createRepo(): WritingStatsRepository {
        val bridge =
            AppServiceBridge(
                WriterAppServiceHolder(
                    "/tmp/sujian_stats_test_ws",
                    "/tmp/sujian_stats_test_ws",
                ),
            )
        return WritingStatsRepository(bridge.statsBridge)
    }

    private fun createVm(): StatsViewModel = StatsViewModel(createRepo())

    @Test
    fun `initial state is loading with no data`() {
        val vm = createVm()
        assertTrue("初始必须处于加载中", vm.uiState.value.loading)
        assertNull(vm.uiState.value.summary)
        assertTrue(vm.uiState.value.projects.isEmpty())
    }

    @Test
    fun `refreshIfNeeded settles to empty data when native unavailable`() {
        val vm = createVm()
        vm.refreshIfNeeded()
        awaitUntil(
            predicate = { !vm.uiState.value.loading },
            message = "refreshIfNeeded must settle (loading -> false)",
        )
        // 原生库未加载：查询如实返回空，不伪装成有数据。
        assertNull(vm.uiState.value.summary)
        assertTrue(vm.uiState.value.projects.isEmpty())
    }

    @Test
    fun `second refreshIfNeeded after settle does not reset to loading`() {
        val vm = createVm()
        vm.refreshIfNeeded()
        awaitUntil(
            predicate = { !vm.uiState.value.loading },
            message = "first refresh must settle",
        )
        // revision 未变：第二次 refresh 命中缓存，不应回到 loading 重新查询。
        vm.refreshIfNeeded()
        assertFalse(
            "revision 未变化时不得重新进入加载态",
            vm.uiState.value.loading,
        )
    }

    @Test
    fun `revision bump triggers requery and advances loadedRevision`() {
        val repo = createRepo()
        val vm = StatsViewModel(repo)
        vm.refreshIfNeeded()
        awaitUntil(
            predicate = { !vm.uiState.value.loading },
            message = "first refresh must settle",
        )
        // 失败路径契约：查询结束后已推进到当时的 revision，再次 refresh 命中缓存。
        val settled = vm.loadedRevision
        vm.refreshIfNeeded()
        assertEquals(settled, vm.loadedRevision)
        // 统计数据变化（revision 递增）：下次 refresh 必须重新查询并推进 loadedRevision。
        repo.invalidate()
        vm.refreshIfNeeded()
        awaitUntil(
            predicate = { vm.loadedRevision == repo.revision.value },
            message = "revision bump must trigger a requery that settles on the new revision",
        )
        assertEquals(settled + 1L, vm.loadedRevision)
    }

    @Test
    fun `revision bumps only on successful stats write`() {
        val repo = createRepo()
        val before = repo.revision.value
        val result =
            repo.processWritingEvent(
                deviceId = "test-device",
                platform = "android",
                projectId = "p",
                volumeId = "v",
                chapterId = "c",
                oldText = "",
                newText = "x",
                durationSeconds = 1u,
                sessionId = "s",
            )
        // 契约：只有写入成功才递增 revision；失败（单测环境原生库未加载 →
        // NotLoaded/Error）不得递增，否则 UI 会因失败不断重查。
        assertEquals(
            "revision 只在写入成功时递增",
            if (result is com.xiwei.sujian.core.interop.common.BridgeResult.Success) {
                before + 1L
            } else {
                before
            },
            repo.revision.value,
        )
    }

    @Test
    fun `invalidate bumps revision`() {
        val repo = createRepo()
        val before = repo.revision.value
        repo.invalidate()
        assertEquals(before + 1L, repo.revision.value)
    }

    private fun awaitUntil(
        predicate: () -> Boolean,
        message: String,
        timeoutMs: Long = 15_000,
    ) {
        val shadow = org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper())
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            shadow.idle()
            if (predicate()) return
            Thread.sleep(10)
        }
        org.junit.Assert.fail("$message (within ${timeoutMs}ms)")
    }
}
