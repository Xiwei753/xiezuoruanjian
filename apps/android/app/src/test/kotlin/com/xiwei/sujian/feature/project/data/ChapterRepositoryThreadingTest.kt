package com.xiwei.sujian.feature.project.data

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * #624 评论12 第3项：ChapterRepository 自己负责 main-safe —
 * 保存/清空必须经注入的 IO dispatcher 派发，不得在调用方（Compose/Main）
 * 线程直接执行同步 UniFFI + 文件操作。`suspend` 关键字不会自动换线程。
 *
 * 记录 dispatch 调用：旧实现直接在 suspend 函数体里调 bridge（不经过
 * dispatcher），本测试的 RecordingDispatcher.dispatchCount 会保持 0。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ChapterRepositoryThreadingTest {
    private class RecordingDispatcher : CoroutineDispatcher() {
        var dispatchCount = 0

        override fun dispatch(
            context: kotlin.coroutines.CoroutineContext,
            block: Runnable,
        ) {
            dispatchCount++
            block.run()
        }
    }

    private fun createRepo(dispatcher: CoroutineDispatcher): ChapterRepository {
        val app = RuntimeEnvironment.getApplication()
        val bridge =
            AppServiceBridge(
                WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_624_repo_threading",
                    "/tmp/sujian_test_workspace_624_repo_threading",
                ),
            )
        return ChapterRepository(app, bridge, ioDispatcher = dispatcher)
    }

    @Test
    fun saveChapterContent_runsThroughInjectedIoDispatcher() =
        runTest {
            val dispatcher = RecordingDispatcher()
            val repo = createRepo(dispatcher)

            val result = repo.saveChapterContent("p", "v", "c", "正文")

            assertTrue(
                "saveChapterContent 必须经注入的 IO dispatcher 派发（main-safe），" +
                    "不得在调用方线程直接执行桥调用",
                dispatcher.dispatchCount >= 1,
            )
            // 桥调用真实执行到边界（无 native → NotLoaded）。
            assertEquals(BridgeResult.NotLoaded, result)
        }

    @Test
    fun clearChapterContent_runsThroughInjectedIoDispatcher() =
        runTest {
            val dispatcher = RecordingDispatcher()
            val repo = createRepo(dispatcher)

            val result = repo.clearChapterContent("p", "v", "c")

            assertTrue(
                "clearChapterContent 必须经注入的 IO dispatcher 派发（main-safe）",
                dispatcher.dispatchCount >= 1,
            )
            // 桥调用真实执行到边界（无 native → NotLoaded）。
            assertEquals(BridgeResult.NotLoaded, result)
        }
}
