package com.xiwei.sujian.ui

import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WriterAppServiceHolder
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
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
import org.robolectric.annotation.Config
import org.robolectric.RuntimeEnvironment

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
class EditorViewModelChapterSwitchContractTest {

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
        val bridge = AppServiceBridge(WriterAppServiceHolder("/tmp/sujian_test_workspace_595"))
        val repo = WorkspaceRepository(app, bridge)
        val vm = EditorViewModel(app)
        vm.initialize(repo, SettingsRepository(app, bridge))
        return vm
    }

    @Test
    fun saveFailure_returnsSaveFailedAndKeepsCurrentChapter() = runTest {
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
    fun switchChapterSetsLoadingTrueDuringTransition() = runTest {
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
    fun loadFailure_returnsLoadFailedAndRestoresCurrentChapter() = runTest {
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
    fun firstEntryLoadFailure_returnsLoadFailedWithoutRollbackTarget() = runTest {
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
    fun sameChapterSwitchIsNoOp() = runTest {
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
}
