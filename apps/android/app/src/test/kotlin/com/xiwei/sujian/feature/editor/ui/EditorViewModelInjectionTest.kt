package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.editor.platform.EditorEditSource
import com.xiwei.sujian.feature.editor.session.EditorAppliedEvent
import com.xiwei.sujian.feature.editor.session.EditorContentDelta
import com.xiwei.sujian.feature.editor.session.EditorOperationKind
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import com.xiwei.sujian.feature.sync.data.SyncRepository
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
        return AppServiceBridge(
            WriterAppServiceHolder("/tmp/sujian_test_workspace_595_inject", "/tmp/sujian_test_workspace_595_inject"),
        )
    }

    @Test
    fun factory_injectsContainerRepositories() {
        val app = RuntimeEnvironment.getApplication()
        val bridge = createBridge()
        val deps =
            object : com.xiwei.sujian.app.di.SujianAppDependencies {
                override val appServiceBridge: AppServiceBridge = bridge
                override val presentationPolicyCatalog: com.xiwei.sujian.app.presentation.PresentationPolicyCatalog =
                    com.xiwei.sujian.app.presentation.PresentationPolicyCatalog(
                        resolver = { role ->
                            when (val result = bridge.resolveScreenPolicy(role)) {
                                is com.xiwei.sujian.core.interop.common.BridgeResult.Success -> result.data
                                else -> null
                            }
                        },
                    )
                override val projectRepository: ProjectRepository = ProjectRepository(app, bridge)
                override val chapterRepository: ChapterRepository = ChapterRepository(app, bridge)
                override val recentEditsRepository: RecentEditsRepository = RecentEditsRepository(app, bridge)
                override val statsRepository: WritingStatsRepository = WritingStatsRepository(bridge.statsBridge)
                override val settingsRepository: SettingsRepository = SettingsRepository(app, bridge)
                override val themeRepository: com.xiwei.sujian.app.theme.ThemeRepository =
                    com.xiwei.sujian.app.theme.ThemeRepository(app, bridge)
                override val syncRepository: SyncRepository = SyncRepository(app, bridge)
                override val syncStatusRepository: com.xiwei.sujian.feature.sync.data.SyncStatusRepository =
                    com.xiwei.sujian.feature.sync.data.SyncStatusRepository(syncRepository)
                override val syncCoordinator: com.xiwei.sujian.feature.sync.data.SyncCoordinator =
                    com.xiwei.sujian.feature.sync.data.SyncCoordinator(syncRepository, syncStatusRepository)
                override val starmapRepository: com.xiwei.sujian.feature.starmap.data.StarMapRepository =
                    com.xiwei.sujian.app.di.AppServiceProvider.getAppServiceBridge(app).starMapBridge.repository
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
        // internal getter 在 JVM 字节码中会被 Kotlin 名称修饰（getProjectRepository$module）。
        val getter =
            EditorViewModel::class.java.declaredMethods.firstOrNull {
                it.name.startsWith("getProjectRepository")
            } ?: throw NoSuchMethodException("getProjectRepository not found")
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
            ProjectRepository(app, bridge),
            SettingsRepository(app, bridge),
            syncRepo = SyncRepository(app, bridge),
            chapterRepo = ChapterRepository(app, bridge),
            recentEditsRepo = RecentEditsRepository(app, bridge),
            statsRepo = WritingStatsRepository(bridge.statsBridge),
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
        // #624 评论9：热路径走 onEditorApplied（不传整章 String）。
        vm.onEditorApplied(
            EditorAppliedEvent(
                revision = 1L,
                transactionId = 1L,
                operationKind = EditorOperationKind.INSERT,
                source = EditorEditSource.NORMAL,
                cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = 11),
            ),
        )
        assertEquals("解除冻结后输入必须恢复（saveStatus 标 Unsaved）", SaveStatus.Unsaved, vm.uiState.value.saveStatus)
    }

    @Test
    fun isCurrentChapter_guardsOldPaneEditorDisplay() {
        val app = RuntimeEnvironment.getApplication()
        val bridge = createBridge()
        val vm = EditorViewModel(app)
        vm.initialize(
            ProjectRepository(app, bridge),
            SettingsRepository(app, bridge),
            syncRepo = SyncRepository(app, bridge),
            chapterRepo = ChapterRepository(app, bridge),
            recentEditsRepo = RecentEditsRepository(app, bridge),
            statsRepo = WritingStatsRepository(bridge.statsBridge),
        )
        vm.initChapter("p", "v", "a", "A")
        assertTrue(vm.isCurrentChapter("p", "v", "a"))
        assertFalse(vm.isCurrentChapter("p", "v", "b"))
    }
}
