package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncResult
import com.xiwei.sujian.model.SyncStatus
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test
import java.util.concurrent.atomic.AtomicInteger

/**
 * #600 评论 #7: AppSyncDataBarrier.reloadAfterSync 正反测试.
 *
 * - 正面: downloadedFiles 包含 starmaps/settings/themes 前缀 -> 对应回调被调用
 * - 删除路径: localDeletes/remoteDeletes 包含对应前缀 -> 回调被调用
 * - 反面: 不包含上述前缀 / 空结果 -> 回调不被调用
 */
class AppSyncDataBarrierTest {
    // reloadAfterSync 不使用 starmapBridge, 传入不会触发 native 库初始化的实例.
    // WriterAppServiceHolder.service 是 lazy 的, 构造时不初始化.
    private val starmapBridge: StarMapBridge =
        StarMapBridge(
            WriterAppServiceHolder(
                appDataRoot = "/tmp/sujian-test-app-data",
                projectsRoot = "/tmp/sujian-test-projects",
            ),
        )

    private fun makeBarrier(
        settingsCalls: AtomicInteger,
        themesCalls: AtomicInteger,
        starmapCalls: AtomicInteger,
    ): AppSyncDataBarrier =
        AppSyncDataBarrier(
            starmapBridge = starmapBridge,
            reloadSettings = { settingsCalls.incrementAndGet() },
            reloadThemes = { themesCalls.incrementAndGet() },
            invalidateStarmapCache = { starmapCalls.incrementAndGet() },
        )

    private fun syncResult(
        downloaded: List<String> = emptyList(),
        localDeletes: List<String> = emptyList(),
        remoteDeletes: List<String> = emptyList(),
    ): SyncResult =
        SyncResult(
            status = SyncStatus.Success,
            downloadedFiles = downloaded,
            localDeletes = localDeletes,
            remoteDeletes = remoteDeletes,
        )

    // === 正面测试: downloadedFiles ===

    @Test
    fun downloadedStarmapPath_invokesInvalidateStarmapCache() =
        runTest {
            val settingsCalls = AtomicInteger(0)
            val themesCalls = AtomicInteger(0)
            val starmapCalls = AtomicInteger(0)
            val barrier = makeBarrier(settingsCalls, themesCalls, starmapCalls)

            barrier.reloadAfterSync(syncResult(downloaded = listOf("starmaps/foo.json")))

            assertEquals(1, starmapCalls.get())
            assertEquals(0, settingsCalls.get())
            assertEquals(0, themesCalls.get())
        }

    @Test
    fun downloadedSettingsPath_invokesReloadSettings() =
        runTest {
            val settingsCalls = AtomicInteger(0)
            val themesCalls = AtomicInteger(0)
            val starmapCalls = AtomicInteger(0)
            val barrier = makeBarrier(settingsCalls, themesCalls, starmapCalls)

            barrier.reloadAfterSync(syncResult(downloaded = listOf("settings.sync.json")))

            assertEquals(1, settingsCalls.get())
            assertEquals(0, starmapCalls.get())
            assertEquals(0, themesCalls.get())
        }

    @Test
    fun downloadedThemesPath_invokesReloadThemes() =
        runTest {
            val settingsCalls = AtomicInteger(0)
            val themesCalls = AtomicInteger(0)
            val starmapCalls = AtomicInteger(0)
            val barrier = makeBarrier(settingsCalls, themesCalls, starmapCalls)

            barrier.reloadAfterSync(syncResult(downloaded = listOf("themes/palettes/x.json")))

            assertEquals(1, themesCalls.get())
            assertEquals(0, starmapCalls.get())
            assertEquals(0, settingsCalls.get())
        }

    // === 删除路径测试: localDeletes / remoteDeletes ===

    @Test
    fun remoteDeletedStarmapPath_invokesInvalidateStarmapCache() =
        runTest {
            val settingsCalls = AtomicInteger(0)
            val themesCalls = AtomicInteger(0)
            val starmapCalls = AtomicInteger(0)
            val barrier = makeBarrier(settingsCalls, themesCalls, starmapCalls)

            barrier.reloadAfterSync(syncResult(remoteDeletes = listOf("starmaps/foo.json")))

            assertEquals(1, starmapCalls.get())
            assertEquals(0, settingsCalls.get())
            assertEquals(0, themesCalls.get())
        }

    @Test
    fun localDeletedSettingsPath_invokesReloadSettings() =
        runTest {
            val settingsCalls = AtomicInteger(0)
            val themesCalls = AtomicInteger(0)
            val starmapCalls = AtomicInteger(0)
            val barrier = makeBarrier(settingsCalls, themesCalls, starmapCalls)

            barrier.reloadAfterSync(syncResult(localDeletes = listOf("settings.sync.json")))

            assertEquals(1, settingsCalls.get())
            assertEquals(0, starmapCalls.get())
            assertEquals(0, themesCalls.get())
        }

    @Test
    fun remoteDeletedThemesPath_invokesReloadThemes() =
        runTest {
            val settingsCalls = AtomicInteger(0)
            val themesCalls = AtomicInteger(0)
            val starmapCalls = AtomicInteger(0)
            val barrier = makeBarrier(settingsCalls, themesCalls, starmapCalls)

            barrier.reloadAfterSync(syncResult(remoteDeletes = listOf("themes/palettes/x.json")))

            assertEquals(1, themesCalls.get())
            assertEquals(0, starmapCalls.get())
            assertEquals(0, settingsCalls.get())
        }

    // === 反面测试 ===

    @Test
    fun unrelatedDownloadedPath_invokesNoCallbacks() =
        runTest {
            val settingsCalls = AtomicInteger(0)
            val themesCalls = AtomicInteger(0)
            val starmapCalls = AtomicInteger(0)
            val barrier = makeBarrier(settingsCalls, themesCalls, starmapCalls)

            barrier.reloadAfterSync(
                syncResult(downloaded = listOf("projects/foo/chapters/1.md", "README.md")),
            )

            assertEquals(0, starmapCalls.get())
            assertEquals(0, settingsCalls.get())
            assertEquals(0, themesCalls.get())
        }

    @Test
    fun emptyResult_invokesNoCallbacks() =
        runTest {
            val settingsCalls = AtomicInteger(0)
            val themesCalls = AtomicInteger(0)
            val starmapCalls = AtomicInteger(0)
            val barrier = makeBarrier(settingsCalls, themesCalls, starmapCalls)

            barrier.reloadAfterSync(syncResult())

            assertEquals(0, starmapCalls.get())
            assertEquals(0, settingsCalls.get())
            assertEquals(0, themesCalls.get())
        }

    @Test
    fun unrelatedDeletedPath_invokesNoCallbacks() =
        runTest {
            val settingsCalls = AtomicInteger(0)
            val themesCalls = AtomicInteger(0)
            val starmapCalls = AtomicInteger(0)
            val barrier = makeBarrier(settingsCalls, themesCalls, starmapCalls)

            barrier.reloadAfterSync(
                syncResult(
                    localDeletes = listOf("projects/foo/chapters/1.md"),
                    remoteDeletes = listOf("docs/old.md"),
                ),
            )

            assertEquals(0, starmapCalls.get())
            assertEquals(0, settingsCalls.get())
            assertEquals(0, themesCalls.get())
        }
}
