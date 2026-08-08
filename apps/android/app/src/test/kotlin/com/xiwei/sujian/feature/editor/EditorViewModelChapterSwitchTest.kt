package com.xiwei.sujian.feature.editor

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.settings.SettingsRepository
import com.xiwei.sujian.core.interop.stats.StatsRepository
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.UnconfinedTestDispatcher
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
 * #595 一：章节切换事务契约测试。
 *
 * switchChapter 返回 [ChapterSwitchResult]：
 * - 保存旧章节 → 加载新章节 → 成功后一次性提交（Success）；
 * - 保存失败返回 SaveFailed，currentSession/标题保持旧章节（导航必须回滚）；
 * - 加载失败返回 LoadFailed 并回退 currentSession/标题（防止回滚后把旧正文写入新章节）。
 *
 * 测试环境无 native 库：所有 Bridge 调用返回 NotLoaded（wrapResult 捕获
 * UnsatisfiedLinkError），因此保存必然失败、加载必然失败 — 正好覆盖两条失败路径。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorViewModelChapterSwitchTest {
    class MainDispatcherRule(
        val dispatcher: TestDispatcher = UnconfinedTestDispatcher(),
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
            AppServiceBridge(WriterAppServiceHolder("/tmp/sujian_test_workspace_595", "/tmp/sujian_test_workspace_595"))
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
    fun saveFailure_returnsSaveFailedAndKeepsCurrentChapter() =
        runTest {
            val vm = createVm()
            // 先有正文内容（loading=false 初始态下 onContentChanged 生效）
            vm.onContentChanged("已有章节内容")
            assertEquals("已有章节内容", vm.uiState.value.content)

            // 进入章节 A（initChapter 同步置 loading=true）
            vm.initChapter("p", "v", "a", "A")

            // 切换到章节 B：oldSession=A → 保存 A 的正文 → 无 native → 保存失败
            val result = vm.switchChapter("p", "v", "b", "B")

            assertTrue(
                "保存失败必须返回 ChapterSwitchResult.SaveFailed（不能只把 loading 改回 false）",
                result is ChapterSwitchResult.SaveFailed,
            )
            assertFalse(
                "章节切换保存失败必须恢复 loading=false（否则编辑器永久卡在加载态）",
                vm.uiState.value.loading,
            )
            assertEquals(
                "保存失败必须上报 SaveFailed",
                SaveStatus.SaveFailed,
                vm.uiState.value.saveStatus,
            )
            assertEquals(
                "#595 一：保存失败时 ViewModel 当前章节必须保持旧章节（A）— " +
                    "导航目标与 currentSession 不得分裂",
                "A",
                vm.uiState.value.chapterTitle,
            )
            assertEquals(
                "保存失败时正文保持旧章节内容，不得被替换",
                "已有章节内容",
                vm.uiState.value.content,
            )
        }

    @Test
    fun switchChapterSetsLoadingTrueDuringTransition() =
        runTest {
            val vm = createVm()
            vm.initChapter("p", "v", "a", "A")

            // 内容为空 → 保存跳过 → 同步部分（loading=true、建新 session、启动加载）直接完成，
            // 然后挂起在真实 IO 加载上。加载完成前 loading 必须已置 true —
            // 编辑器在旧正文可见期间不会被重新绑定到新章节。
            val switchJob = launch { vm.switchChapter("p", "v", "b", "B") }
            assertTrue(
                "切换章节时 loading 必须已置 true（编辑器隐藏，新章节 session 待内容就绪后创建）",
                vm.uiState.value.loading,
            )
            switchJob.join()
        }

    @Test
    fun loadFailure_returnsLoadFailedAndRestoresCurrentChapter() =
        runTest {
            val vm = createVm()
            vm.initChapter("p", "v", "a", "A")

            // 切换到 B：旧章节内容为空 → 跳过保存 → 新 session=B → 加载 B 失败
            val result = vm.switchChapter("p", "v", "b", "B")

            assertTrue(
                "加载失败必须返回 ChapterSwitchResult.LoadFailed",
                result is ChapterSwitchResult.LoadFailed,
            )
            val failed = result as ChapterSwitchResult.LoadFailed
            assertEquals("LoadFailed 必须携带请求章节 key", ChapterKey("p", "v", "b"), failed.requested)
            assertFalse("加载失败后 loading 必须恢复 false", vm.uiState.value.loading)
            assertEquals(
                "#595 一：加载失败必须回退标题到旧章节 — 不能让 UI 停留在“新标题 + 旧正文”分裂态",
                "A",
                vm.uiState.value.chapterTitle,
            )
        }

    @Test
    fun firstEntryLoadFailure_returnsLoadFailedWithoutRollbackTarget() =
        runTest {
            val vm = createVm()
            // 无旧章节（首次进入编辑器）→ 加载失败 → LoadFailed，标题保持空（无旧章节可回退）。
            val result = vm.switchChapter("p", "v", "b", "B")
            assertTrue(
                "首次进入加载失败必须返回 LoadFailed",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertEquals(
                "首次进入失败时没有旧章节标题可回退",
                "",
                vm.uiState.value.chapterTitle,
            )
            assertFalse(vm.uiState.value.loading)
        }

    @Test
    fun sameChapterSwitchIsNoOp() =
        runTest {
            val vm = createVm()
            vm.initChapter("p", "v", "a", "A")
            val before = vm.uiState.value.loading
            val result = vm.switchChapter("p", "v", "a", "A")
            assertTrue(
                "相同章节切换必须直接返回 Success（无操作），不改变 loading",
                result is ChapterSwitchResult.Success,
            )
            assertEquals("相同章节切换不改变 loading", before, vm.uiState.value.loading)
            assertEquals("A", vm.uiState.value.chapterTitle)
        }

    @Test
    fun cancelledSwitch_restoresFullOldStateAndRethrowsCancellation() =
        runTest {
            val vm = createVm()
            vm.initChapter("p", "v", "a", "A")
            // 等 initChapter 的加载落定，保证事务起点是稳定状态。
            var attempts = 0
            while (vm.uiState.value.loading && attempts < 200) {
                Thread.sleep(5)
                attempts++
            }
            // 旧章节有非空正文 → 切换事务的“保存旧章节”阶段会调用保存端口。
            // initChapter 事务后 inputFrozen 保持 true（等待编辑器附着），
            // 测试环境无编辑器 — 显式确认附着以解除冻结，模拟真实附着。
            vm.confirmEditorAttached(vm.chapterTargetId("p", "v", "a"))
            vm.onContentChanged("正文A")

            // #597：可控保存端口 — 保存 A 时挂起，为取消制造确定性挂起点
            // （loadChapter 的 withContext(IO) 在无 native 时几乎立即返回，
            // 直接取消会落在已完成的 job 上，满载调度下偶发）。
            val saveGate = kotlinx.coroutines.CompletableDeferred<Unit>()
            var saveCalls = 0
            vm.chapterSavePort =
                object : com.xiwei.sujian.core.interop.project.ChapterContentSavePort {
                    override suspend fun saveChapterContent(
                        projectId: String,
                        volumeId: String,
                        chapterId: String,
                        content: String,
                    ): BridgeResult<ChapterSaveReceipt> {
                        saveCalls++
                        saveGate.await()
                        return BridgeResult.Success(
                            ChapterSaveReceipt("c", 0L, "h", "m", "t", 0),
                        )
                    }
                }

            var cancellationSeen = false
            val job =
                launch {
                    try {
                        vm.requestOpenChapter("p", "v", "b", "B")
                    } catch (e: kotlinx.coroutines.CancellationException) {
                        // #595 一：取消必须向上重抛，不得被当作普通加载失败（LoadFailed）。
                        cancellationSeen = true
                        throw e
                    }
                }
            // 事务进入保存 A 的挂起点后取消。
            var spin = 0
            while (saveCalls < 1 && spin < 200) {
                runCurrent()
                spin++
            }
            assertTrue("切换事务必须调用保存端口（进入保存挂起点）", saveCalls >= 1)
            job.cancelAndJoin()
            // 放行保存端口，避免事务协程悬挂泄漏。
            saveGate.complete(Unit)
            runCurrent()

            assertTrue(
                "取消必须重新抛出 CancellationException — 不得吞掉并当加载失败处理",
                cancellationSeen,
            )
            // 取消后旧状态完整恢复：标题、正文、loading、saveStatus 全部回到切换前。
            assertEquals("取消后标题必须恢复旧章节", "A", vm.uiState.value.chapterTitle)
            assertEquals("取消后正文必须保留旧章节内容", "正文A", vm.uiState.value.content)
            assertFalse("取消后 loading 必须恢复 false", vm.uiState.value.loading)
            // 取消后 inputFrozen 必须释放：后续输入能正常进入状态（否则输入被冻结）。
            vm.onContentChanged("取消后的新输入")
            assertEquals(
                "取消后输入必须解冻（inputFrozen 由 finally 复位）",
                "取消后的新输入",
                vm.uiState.value.content,
            )
        }

    @Test
    fun loadFailure_restoresCompleteOldUiStateSnapshot() =
        runTest {
            val vm = createVm()
            vm.initChapter("p", "v", "a", "A")
            // 等 initChapter 的加载落定（无 native 时必失败 → loading=false）。
            var attempts = 0
            while (vm.uiState.value.loading && attempts < 200) {
                Thread.sleep(5)
                attempts++
            }
            assertFalse("前置状态必须是已落定（loading=false）", vm.uiState.value.loading)
            val before = vm.uiState.value

            val result = vm.requestOpenChapter("p", "v", "b", "B")

            assertTrue(
                "加载失败必须返回 LoadFailed",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertEquals(
                "#595 一：加载失败必须完整恢复旧 EditorUiState（content/hash/note/" +
                    "editorEnabled/saveStatus/loading/title 全部一致），不能只恢复标题",
                before,
                vm.uiState.value,
            )
        }

    @Test
    fun concurrentSwitchRequests_serializeWithoutDeadlock() =
        runTest {
            val vm = createVm()
            vm.initChapter("p", "v", "a", "A")

            // #595 一：并发点击多个章节 — 请求经 ChapterSwitchGate 串行执行，
            // 不得死锁；失败环境下最终状态必须回到旧章节。
            val first = async { vm.requestOpenChapter("p", "v", "b", "B") }
            val second = async { vm.requestOpenChapter("p", "v", "c", "C") }
            val results = listOf(first.await(), second.await())

            for (r in results) {
                assertTrue(
                    "并发切换必须正常完成（Success/SaveFailed/LoadFailed/Stale 之一），不得挂起：$r",
                    r is ChapterSwitchResult.Success ||
                        r is ChapterSwitchResult.SaveFailed ||
                        r is ChapterSwitchResult.LoadFailed ||
                        r is ChapterSwitchResult.Stale,
                )
            }
            assertFalse("并发切换后 loading 必须恢复 false", vm.uiState.value.loading)
            assertEquals("并发切换后标题回到旧章节", "A", vm.uiState.value.chapterTitle)
        }

    @Test
    fun requestOpenChapter_successPathIsRequiredByCallers() =
        runTest {
            val vm = createVm()
            vm.initChapter("p", "v", "a", "A")

            // 同一章节（已提交）→ Success；调用方据此导航，不触发回滚。
            val same = vm.requestOpenChapter("p", "v", "a", "A")
            assertTrue("已提交章节的请求必须 Success", same is ChapterSwitchResult.Success)
        }

    @Test
    fun isCurrentChapter_reflectsCommittedSession() {
        val vm = createVm()
        vm.initChapter("p", "v", "a", "A")
        assertTrue(
            "initChapter 后当前章节必须匹配",
            vm.isCurrentChapter("p", "v", "a"),
        )
        assertFalse(
            "未提交的章节必须不匹配 — 防止旧 pane 用新正文 beginEdit 旧 target",
            vm.isCurrentChapter("p", "v", "b"),
        )
    }
}
