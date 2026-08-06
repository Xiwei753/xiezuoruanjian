package com.xiwei.sujian.ui

import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WriterAppServiceHolder
import kotlinx.coroutines.Dispatchers
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
 * #595 一/转场：章节切换 loading 状态契约测试。
 *
 * 修复内容：switchChapter 在旧章节保存完成前就同步置 loading=true（编辑器隐藏），
 * 防止 WritingPane 在保存窗口期用旧章节正文对目标章节 beginEdit/resetPersistentSession；
 * 保存失败路径必须恢复 loading=false，否则编辑器会卡在加载转圈。
 *
 * 测试环境无 native 库：所有 Bridge 调用返回 NotLoaded（wrapResult 捕获
 * UnsatisfiedLinkError），因此保存必然失败 — 正好覆盖保存失败路径。
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
    fun saveFailureRestoresLoadingFalse() = runTest {
        val vm = createVm()
        // 先有正文内容（loading=false 初始态下 onContentChanged 生效）
        vm.onContentChanged("已有章节内容")
        assertEquals("已有章节内容", vm.uiState.value.content)

        // 进入章节 A（initChapter 同步置 loading=true）
        vm.initChapter("p", "v", "a", "A")
        assertTrue("initChapter 必须同步置 loading=true", vm.uiState.value.loading)

        // 切换到章节 B：oldSession=A → 保存 A 的正文 → 无 native → 保存失败
        // 修复前：保存失败提前返回时 loading 保持 true，编辑器卡在转圈；
        // 修复后：恢复 loading=false。
        vm.switchChapter("p", "v", "b", "B")

        assertFalse(
            "章节切换保存失败必须恢复 loading=false（否则编辑器永久卡在加载态）",
            vm.uiState.value.loading,
        )
        assertEquals(
            "保存失败必须上报 SaveFailed",
            SaveStatus.SaveFailed,
            vm.uiState.value.saveStatus,
        )
    }

    @Test
    fun switchChapterSetsLoadingTrueBeforeNewContentReady() = runTest {
        val vm = createVm()
        vm.initChapter("p", "v", "a", "A")

        // 内容为空 → 保存跳过 → 同步部分（loading=true、建新 session、启动加载）直接完成。
        // 断言在加载完成前 loading 已为 true — 编辑器在旧正文可见期间不会被重新绑定。
        vm.switchChapter("p", "v", "b", "B")
        assertTrue(
            "切换章节时 loading 必须已置 true（编辑器隐藏，新章节 session 待内容就绪后创建）",
            vm.uiState.value.loading,
        )
    }

    @Test
    fun sameChapterSwitchIsNoOp() = runTest {
        val vm = createVm()
        vm.initChapter("p", "v", "a", "A")
        val before = vm.uiState.value.loading
        vm.switchChapter("p", "v", "a", "A")
        assertEquals("相同章节切换必须直接返回，不改变 loading", before, vm.uiState.value.loading)
        assertEquals("A", vm.uiState.value.chapterTitle)
    }
}
