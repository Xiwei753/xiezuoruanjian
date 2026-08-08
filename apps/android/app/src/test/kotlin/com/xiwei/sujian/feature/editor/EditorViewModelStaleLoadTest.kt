package com.xiwei.sujian.feature.editor

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.project.ChapterContentSavePort
import com.xiwei.sujian.core.interop.project.ProjectRepository
import com.xiwei.sujian.core.interop.settings.SettingsRepository
import com.xiwei.sujian.core.interop.stats.StatsRepository
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * #597：迟到背景加载不得覆盖切换事务已提交状态 — 确定性回归测试。
 *
 * 复现场景：initChapter 遗留入口会后台启动 loadChapter；切换事务保存旧章节
 * 失败并提交 SaveFailed 后，若该背景加载（同一 session，事务失败不替换
 * currentSession）的失败写入迟到，会把 SaveFailed 覆盖成 Idle。
 *
 * 确定性手段：
 * - StandardTestDispatcher：initChapter 的后台加载任务先排队，切换事务
 *   （同步失败保存端口，无真实 IO 挂起）先完整提交 SaveFailed；
 * - 之后才让调度器运行后台加载 — 其失败写入（真实 IO 完成后回到主队列）
 *   必须被纪元校验拒绝，不得覆盖 SaveFailed。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorViewModelStaleLoadTest {
    class MainDispatcherRule(
        val dispatcher: TestDispatcher = StandardTestDispatcher(),
    ) : TestWatcher() {
        override fun starting(description: Description) {
            Dispatchers.setMain(dispatcher)
        }

        override fun finished(description: Description) {
            Dispatchers.resetMain()
        }
    }

    @get:Rule
    val mainDispatcherRule = MainDispatcherRule()

    private fun createVm(): EditorViewModel {
        val app = RuntimeEnvironment.getApplication()
        // 无 native 库时所有桥接调用返回 NotLoaded，不会抛 UnsatisfiedLinkError。
        val bridge =
            AppServiceBridge(
                WriterAppServiceHolder("/tmp/sujian_test_workspace_597_stale", "/tmp/sujian_test_workspace_597_stale"),
            )
        val repo = ProjectRepository(app, bridge)
        val vm = EditorViewModel(app)
        vm.initialize(
            repo,
            SettingsRepository(app, bridge),
            chapterRepo = ChapterRepository(app, bridge),
            recentEditsRepo = RecentEditsRepository(app, bridge),
            statsRepo = StatsRepository(bridge.statsBridge),
        )
        return vm
    }

    @Test
    fun staleBackgroundLoad_mustNotOverwriteTransactionSaveFailed() =
        runTest(mainDispatcherRule.dispatcher) {
            val vm = createVm()
            // 保存端口同步失败：切换事务的"保存旧章节"阶段不挂真实 IO，
            // 在后台加载任务运行之前就把 SaveFailed 提交完成。
            vm.chapterSavePort =
                object : ChapterContentSavePort {
                    override suspend fun saveChapterContent(
                        projectId: String,
                        volumeId: String,
                        chapterId: String,
                        content: String,
                    ): BridgeResult<com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt> =
                        BridgeResult.Error(
                            com.xiwei.sujian.core.interop.common.ResultEnvelope.errorOf(
                                "DETERMINISTIC_SAVE_FAILURE",
                                "deterministic save failure",
                            ),
                        )
                }

            vm.onContentChanged("已有章节内容")
            vm.initChapter("p", "v", "a", "A")
            // 让 initChapter 的后台加载启动并挂起在真实 IO 上（捕获事务前的纪元）。
            runCurrent()
            assertTrue("后台加载必须已启动（挂起在 IO 上）", vm.isLoadingChapter)

            val result = vm.switchChapter("p", "v", "b", "B")
            assertTrue("保存失败必须返回 ChapterSwitchResult.SaveFailed", result is ChapterSwitchResult.SaveFailed)
            assertEquals("事务已提交 SaveFailed", SaveStatus.SaveFailed, vm.uiState.value.saveStatus)

            // 后台加载的真实 IO 失败后，其迟到写入回到主队列 — 必须被纪元校验拒绝。
            var attempts = 0
            while (vm.isLoadingChapter && attempts < 200) {
                Thread.sleep(5)
                runCurrent()
                attempts++
            }
            assertFalse("后台加载必须已经落定", vm.isLoadingChapter)

            assertEquals(
                "迟到的后台加载失败写入不得覆盖事务已提交的 SaveFailed",
                SaveStatus.SaveFailed,
                vm.uiState.value.saveStatus,
            )
            assertEquals("旧章节正文不得被迟到加载覆盖", "已有章节内容", vm.uiState.value.content)
            assertEquals("旧章节标题不得被迟到加载覆盖", "A", vm.uiState.value.chapterTitle)
            assertFalse(vm.uiState.value.loading)
        }
}
