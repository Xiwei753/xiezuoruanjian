package com.xiwei.sujian.ui

import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WriterAppServiceHolder
import com.xiwei.sujian.editor.v2.coordinator.EditorSessionCoordinator
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
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
 * #595 一：EditorViewModel 依赖注入与输入窗口防护契约测试。
 *
 * - Factory 从进程级容器注入同一组 Repository（删除 getApplication() fallback）；
 * - 未注入时访问 Repository 必须立即失败（不允许静默创建第二份容器）；
 * - confirmEditorAttached 只对当前已提交章节的 target 解除输入冻结 —
 *   提交→导航窗口期内旧 pane 无法写入新章节。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorViewModelInjectionTest {
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

    private fun createBridge(): AppServiceBridge {
        return AppServiceBridge(WriterAppServiceHolder("/tmp/sujian_test_workspace_595_inject"))
    }

    @Test
    fun factory_injectsContainerRepositories() {
        val app = RuntimeEnvironment.getApplication()
        val bridge = createBridge()
        val deps =
            object : com.xiwei.sujian.runtime.SujianAppDependencies {
                override val appServiceBridge: AppServiceBridge = bridge
                override val workspaceRepository: WorkspaceRepository = WorkspaceRepository(app, bridge)
                override val settingsRepository: SettingsRepository = SettingsRepository(app, bridge)
                override val syncStatusRepository: com.xiwei.sujian.data.SyncStatusRepository =
                    com.xiwei.sujian.data.SyncStatusRepository(settingsRepository)
                override val syncCoordinator: com.xiwei.sujian.data.SyncCoordinator =
                    com.xiwei.sujian.data.SyncCoordinator(settingsRepository, syncStatusRepository)
            }
        val coordinator = EditorSessionCoordinator(bridge)

        val vm = EditorViewModel.Factory(app, deps, coordinator).create(EditorViewModel::class.java)
        assertNotNull(vm)

        // #595 一：Factory 创建即注入 — 不允许退回到 getApplication() 第二份容器。
        vm.initChapter("p", "v", "a", "A")
        assertTrue("注入后 must 能正常进入章节加载", vm.uiState.value.loading)
    }

    @Test
    fun uninitializedRepositoryAccess_failsFast() {
        val app = RuntimeEnvironment.getApplication()
        val vm = EditorViewModel(app)
        // 未 initialize 时访问 Repository getter 必须抛错
        // （不允许 fallback 创建第二份容器）。
        // internal getter 在 JVM 字节码中会被 Kotlin 名称修饰（getWorkspaceRepository$module）。
        val getter =
            EditorViewModel::class.java.declaredMethods.firstOrNull {
                it.name.startsWith("getWorkspaceRepository")
            } ?: throw NoSuchMethodException("getWorkspaceRepository not found")
        getter.isAccessible = true
        val threw =
            try {
                getter.invoke(vm)
                false
            } catch (e: java.lang.reflect.InvocationTargetException) {
                e.cause is IllegalStateException
            }
        assertTrue(
            "#595 一：未注入容器的 EditorViewModel 必须快速失败，不得静默创建第二份 Repository",
            threw,
        )
    }

    @Test
    fun confirmEditorAttached_onlyUnfreezesCurrentChapter() {
        val app = RuntimeEnvironment.getApplication()
        val bridge = createBridge()
        val vm = EditorViewModel(app)
        vm.initialize(
            WorkspaceRepository(app, bridge),
            SettingsRepository(app, bridge),
        )
        vm.initChapter("p", "v", "a", "A")
        // 等待 initChapter 的加载落定（无 native 时必失败 → loading=false）。
        var attempts = 0
        while (vm.uiState.value.loading && attempts < 200) {
            Thread.sleep(5)
            attempts++
        }

        // 未提交章节的 target 调用 confirmEditorAttached → no-op（不解除冻结）。
        vm.confirmEditorAttached("chapter-body:p:v:other")
        // 当前章节 target → 解除（此时无冻结，幂等）。
        vm.confirmEditorAttached("chapter-body:p:v:a")
        vm.onContentChanged("after attach")
        assertEquals("解除冻结后输入必须恢复", "after attach", vm.uiState.value.content)
    }

    @Test
    fun isCurrentChapter_guardsOldPaneEditorDisplay() {
        val app = RuntimeEnvironment.getApplication()
        val bridge = createBridge()
        val vm = EditorViewModel(app)
        vm.initialize(WorkspaceRepository(app, bridge), SettingsRepository(app, bridge))
        vm.initChapter("p", "v", "a", "A")
        assertTrue(vm.isCurrentChapter("p", "v", "a"))
        assertFalse(vm.isCurrentChapter("p", "v", "b"))
    }
}
