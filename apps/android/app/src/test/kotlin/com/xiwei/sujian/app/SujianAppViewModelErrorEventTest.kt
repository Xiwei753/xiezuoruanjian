package com.xiwei.sujian.app

import androidx.lifecycle.SavedStateHandle
import com.xiwei.sujian.core.interop.common.RepositoryException
import com.xiwei.sujian.feature.project.data.model.Project
import com.xiwei.sujian.feature.project.data.model.ProjectSummary
import com.xiwei.sujian.feature.project.data.model.RecentEdit
import com.xiwei.sujian.feature.project.domain.ProjectUseCasePort
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import kotlinx.coroutines.withTimeoutOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #614：SujianAppViewModel 错误事件与 refreshProjects 保留行为单元测试。
 *
 * - createProject/deleteProject/renameProject 失败时必须通过 uiEvents 发出 [WorkspaceUiEvent.Error]；
 * - refreshProjects 失败时保留上一份 projects（不覆盖为空），仅首次加载失败设 loadError。
 *
 * 用 fake [ProjectUseCasePort] 注入，避免构造真实 Repository/Bridge。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class SujianAppViewModelErrorEventTest {
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

    /** 阻塞主线程轮询条件，让真实 IO 线程有机会完成 withContext(Dispatchers.IO)。 */
    private fun awaitCondition(
        timeoutMs: Long = 3000,
        predicate: () -> Boolean,
    ) {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (!predicate() && System.currentTimeMillis() < deadline) {
            Thread.sleep(5)
        }
        assertTrue("等待条件超时", predicate())
    }

    /** 在 vm.uiEvents 上启动收集，捕获首个 [WorkspaceUiEvent.Error]。必须在触发操作前构造。 */
    private class ErrorCollector(vm: SujianAppViewModel) {
        private val firstError = CompletableDeferred<WorkspaceUiEvent.Error>()
        private val scope = CoroutineScope(Dispatchers.Unconfined)
        private val job =
            scope.launch {
                vm.uiEvents.collect { e ->
                    if (e is WorkspaceUiEvent.Error && !firstError.isCompleted) {
                        firstError.complete(e)
                    }
                }
            }

        fun await(timeoutMs: Long = 3000): WorkspaceUiEvent.Error {
            val error =
                runBlocking {
                    withTimeoutOrNull(timeoutMs) { firstError.await() }
                }
            assertNotNull("必须在 uiEvents 收到 WorkspaceUiEvent.Error", error)
            return error!!
        }

        fun close() {
            job.cancel()
            scope.cancel()
        }
    }

    @Test
    fun createProject_emitsErrorEventOnFailure() {
        val fake = FakeProjectUseCase()
        fake.createProjectError = RepositoryException("新建失败")
        val vm = SujianAppViewModel(SavedStateHandle())
        vm.setProjectUseCaseForTesting(fake)

        val collector = ErrorCollector(vm)
        vm.createProject("x")
        val error = collector.await()
        collector.close()

        assertTrue(
            "错误事件 message 必须含「新建失败」，实际: ${error.message}",
            error.message.contains("新建失败"),
        )
    }

    @Test
    fun deleteProject_emitsErrorEventOnFailure() {
        val fake = FakeProjectUseCase()
        fake.deleteProjectError = RepositoryException("删除失败")
        val vm = SujianAppViewModel(SavedStateHandle())
        vm.setProjectUseCaseForTesting(fake)

        val collector = ErrorCollector(vm)
        vm.deleteProject("p1")
        val error = collector.await()
        collector.close()

        assertTrue(
            "错误事件 message 必须含「删除失败」，实际: ${error.message}",
            error.message.contains("删除失败"),
        )
    }

    @Test
    fun renameProject_emitsErrorEventOnFailure() {
        val fake = FakeProjectUseCase()
        fake.renameProjectError = RepositoryException("重命名失败")
        val vm = SujianAppViewModel(SavedStateHandle())
        vm.setProjectUseCaseForTesting(fake)

        val collector = ErrorCollector(vm)
        vm.renameProject("p1", "newTitle")
        val error = collector.await()
        collector.close()

        assertTrue(
            "错误事件 message 必须含「重命名失败」，实际: ${error.message}",
            error.message.contains("重命名失败"),
        )
    }

    @Test
    fun refreshProjects_setsLoadErrorOnFirstLoadFailure() {
        val fake = FakeProjectUseCase()
        fake.getProjectsError = RepositoryException("首次失败")
        val vm = SujianAppViewModel(SavedStateHandle())
        vm.setProjectUseCaseForTesting(fake)

        val collector = ErrorCollector(vm)
        vm.refreshProjects()
        awaitCondition { vm.loadError != null }
        val error = collector.await()
        collector.close()

        assertTrue(
            "loadError 必须含「首次失败」，实际: ${vm.loadError}",
            vm.loadError!!.contains("首次失败"),
        )
        assertTrue(
            "首次加载失败时 projects 必须仍为空",
            vm.projects.isEmpty(),
        )
        assertTrue(
            "错误事件 message 必须含「首次失败」，实际: ${error.message}",
            error.message.contains("首次失败"),
        )
    }

    @Test
    fun refreshProjects_keepsPreviousProjectsOnFailure() {
        val fake = FakeProjectUseCase()
        val previousProjects =
            listOf(
                Project(id = "p1", title = "作品一", createdAt = "", updatedAt = ""),
                Project(id = "p2", title = "作品二", createdAt = "", updatedAt = ""),
            )
        fake.projectsResult = previousProjects
        val vm = SujianAppViewModel(SavedStateHandle())
        vm.setProjectUseCaseForTesting(fake)

        // 1. 首次加载成功，填充 projects 并清 loadError。
        vm.refreshProjects()
        awaitCondition { vm.projects.isNotEmpty() }
        assertEquals("首次加载成功后 projects 必须为 fake 返回列表", previousProjects, vm.projects)
        assertNull("首次加载成功后 loadError 必须为 null", vm.loadError)

        // 2. 再次加载失败 — 必须保留上一份 projects（不覆盖为空），且不设 loadError。
        fake.getProjectsError = RepositoryException("刷新失败")
        val collector = ErrorCollector(vm)
        vm.refreshProjects()
        val error = collector.await()
        collector.close()

        assertEquals(
            "刷新失败时必须保留上一份 projects，不得覆盖为空",
            previousProjects,
            vm.projects,
        )
        assertNull(
            "projects 非空时刷新失败不得设 loadError",
            vm.loadError,
        )
        assertTrue(
            "错误事件 message 必须含「刷新失败」，实际: ${error.message}",
            error.message.contains("刷新失败"),
        )
    }

    @Test
    fun createProject_emitsErrorEventWhenNotInitialized() {
        val vm = SujianAppViewModel(SavedStateHandle())
        // 不调 setProjectUseCaseForTesting — projectUseCase 为 null
        val collector = ErrorCollector(vm)
        vm.createProject("x")
        val error = collector.await()
        collector.close()
        assertTrue(
            "未初始化时必须发 Error 事件而非静默成功，实际: ${error.message}",
            error.message.isNotEmpty(),
        )
    }

    @Test
    fun refreshProjects_setsLoadErrorWhenNotInitialized() {
        val vm = SujianAppViewModel(SavedStateHandle())
        val collector = ErrorCollector(vm)
        vm.refreshProjects()
        awaitCondition { vm.loadError != null }
        val error = collector.await()
        collector.close()
        assertNotNull("未初始化时必须设 loadError", vm.loadError)
        assertTrue(
            "未初始化时必须发 Error 事件，实际: ${error.message}",
            error.message.isNotEmpty(),
        )
    }
}

/**
 * #614：测试用 [ProjectUseCasePort] fake。各方法可独立控制抛异常/返回值。
 */
private class FakeProjectUseCase : ProjectUseCasePort {
    var projectsResult: List<Project> = emptyList()
    var projectSummariesResult: List<ProjectSummary> = emptyList()
    var recentEditsResult: List<RecentEdit> = emptyList()
    var getProjectsError: Throwable? = null
    var createProjectError: Throwable? = null
    var deleteProjectError: Throwable? = null
    var renameProjectError: Throwable? = null

    override suspend fun getProjects(): List<Project> {
        getProjectsError?.let { throw it }
        return projectsResult
    }

    override suspend fun getProjectSummaries(): List<ProjectSummary> = projectSummariesResult

    override suspend fun getRecentEdits(limit: Int): List<RecentEdit> = recentEditsResult.take(limit)

    override suspend fun createProject(title: String): Project {
        createProjectError?.let { throw it }
        return Project(id = "", title = title, createdAt = "", updatedAt = "")
    }

    override suspend fun renameProject(
        projectId: String,
        newTitle: String,
    ) {
        renameProjectError?.let { throw it }
    }

    override suspend fun deleteProject(projectId: String) {
        deleteProjectError?.let { throw it }
    }

    override suspend fun reorderProjects(orderedProjectIds: List<String>) {}

    override suspend fun getProjectTitle(projectId: String): String = ""

    override suspend fun getChapterTitle(chapterId: String): String = ""
}
