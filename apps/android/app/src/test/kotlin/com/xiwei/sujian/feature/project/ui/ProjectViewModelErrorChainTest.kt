package com.xiwei.sujian.feature.project.ui

// #617 评论九：章节树错误链统一回归测试 — 验证 ProjectViewModel 不再吞异常，
// 失败时保留旧数据、设 loadError、发 ProjectTreeUiEvent.Error 事件；
// CRUD 失败不 refresh，createChapter 失败不展开卷。
//
// 覆盖场景：
// 1. getVolumes 抛 RepositoryException → refreshProject 失败：isLoading=false，
//    首次加载设 loadError，uiEvents 发 Error，volumes 不被覆盖。
// 2. createVolume 抛异常 → mutateAndRefresh 失败：不 refresh（卷列表不变），发 Error。
// 3. createChapter 失败 → 不加入 expandedVolumeIds，发 Error。
// 4. createChapter 成功 → 加入 expandedVolumeIds + refreshProject。

import androidx.lifecycle.SavedStateHandle
import com.xiwei.sujian.core.interop.common.RepositoryException
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ProjectStats
import com.xiwei.sujian.feature.project.data.model.Volume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ProjectViewModelErrorChainTest {
    companion object {
        private const val testTimestamp = "2026-01-01T00:00:00"
        private const val loadVolumesFailed = "加载卷失败"
        private const val createVolumeFailed = "创建卷失败"
        private const val createChapterFailed = "创建章节失败"
        private const val mustEmitError = "必须发出 Error 事件"
    }

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

    /**
     * #617 评论九：可控行为的 fake repo — 各方法可注入异常或返回成功数据。
     * 继承 ProjectRepository（open class），override 数据读取与 create 方法。
     */
    private class FakeProjectRepository(
        context: android.content.Context,
        private val volumes: Map<String, List<Volume>> = emptyMap(),
        private val stats: Map<String, ProjectStats> = emptyMap(),
    ) : ProjectRepository(context) {
        var getVolumesException: Exception? = null
        var createVolumeException: Exception? = null
        var createChapterException: Exception? = null
        var createVolumeCallCount = 0
        var createChapterCallCount = 0

        override fun getVolumes(projectId: String): List<Volume> {
            getVolumesException?.let { throw it }
            return volumes[projectId].orEmpty()
        }

        override fun getChapters(
            projectId: String,
            volumeId: String,
        ): List<ChapterMeta> = emptyList()

        override fun getProjectStats(projectId: String): ProjectStats = stats[projectId] ?: ProjectStats(0, 0, 0)

        override fun createVolume(
            projectId: String,
            title: String,
        ): Volume {
            createVolumeCallCount++
            createVolumeException?.let { throw it }
            return Volume("v_new_$createVolumeCallCount", title, testTimestamp, testTimestamp)
        }

        override fun createChapter(
            projectId: String,
            volumeId: String,
            title: String,
        ): ChapterMeta {
            createChapterCallCount++
            createChapterException?.let { throw it }
            return ChapterMeta(
                "ch_new_$createChapterCallCount",
                title,
                testTimestamp,
                testTimestamp,
                wordCount = 0,
                hash = "",
            )
        }
    }

    private fun volume(
        id: String,
        title: String,
    ) = Volume(id, title, testTimestamp, testTimestamp)

    private val volumesA = listOf(volume("vA", "A卷"))

    /** 轮询驱动主调度器，直到 [predicate] 满足（真实 IO 完成需要真实等待）。 */
    private fun kotlinx.coroutines.test.TestScope.settleUntil(predicate: () -> Boolean) {
        var attempts = 0
        while (!predicate() && attempts < 400) {
            runCurrent()
            Thread.sleep(5)
            attempts++
        }
        runCurrent()
        assertTrue("条件必须在超时前满足", predicate())
    }

    /** 收集 uiEvents 到 list，返回取消句柄。必须在发出事件前启动。
     *  用 Dispatchers.Unconfined scope 启动 collect，完全脱离 TestDispatcher —
     *  避免 StandardTestDispatcher 下 collect 协程的调度影响 refreshProject 的续体恢复。 */
    private fun collectEvents(vm: ProjectViewModel): Pair<MutableList<ProjectTreeUiEvent>, kotlinx.coroutines.Job> {
        val events = mutableListOf<ProjectTreeUiEvent>()
        val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Unconfined)
        val job = scope.launch { vm.uiEvents.collect { events.add(it) } }
        return events to job
    }

    @Test
    fun getVolumesThrows_setsLoadErrorAndEmitsErrorEvent() =
        runTest(mainDispatcherRule.dispatcher) {
            val app = RuntimeEnvironment.getApplication()
            val repo =
                FakeProjectRepository(
                    context = app,
                    volumes = mapOf("A" to volumesA),
                    stats = mapOf("A" to ProjectStats(100, 1, 1)),
                )
            repo.getVolumesException = RepositoryException(loadVolumesFailed)
            val vm = ProjectViewModel(SavedStateHandle())
            val (events, collectJob) = collectEvents(vm)

            vm.initialize("A", repo)
            for (i in 0..200) {
                runCurrent()
                Thread.sleep(10)
            }
            runCurrent()

            // #617 评论九：getVolumes 抛 RepositoryException → refreshProject 失败：
            // isLoading=false，首次加载 volumes 为空时设 loadError，发 Error 事件。
            assertFalse("加载失败后不得停留在 loading 态", vm.uiState.value.isLoading)
            assertNotNull("首次加载失败必须设 loadError", vm.uiState.value.loadError)
            assertEquals(loadVolumesFailed, vm.uiState.value.loadError)
            assertTrue("volumes 不被覆盖为非空（首次无旧数据保持空）", vm.uiState.value.volumes.isEmpty())
            for (i in 0..50) {
                runCurrent()
                Thread.sleep(5)
            }
            runCurrent()
            assertTrue(mustEmitError, events.any { it is ProjectTreeUiEvent.Error })
            assertEquals(
                loadVolumesFailed,
                (events.filterIsInstance<ProjectTreeUiEvent.Error>().first()).message,
            )
            collectJob.cancel()
        }

    @Test
    fun createVolumeThrows_doesNotRefreshAndEmitsError() =
        runTest(mainDispatcherRule.dispatcher) {
            val app = RuntimeEnvironment.getApplication()
            val repo =
                FakeProjectRepository(
                    context = app,
                    volumes = mapOf("A" to volumesA),
                    stats = mapOf("A" to ProjectStats(100, 1, 1)),
                )
            val vm = ProjectViewModel(SavedStateHandle())
            val (events, collectJob) = collectEvents(vm)

            vm.initialize("A", repo)
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vA") &&
                    !vm.uiState.value.isLoading
            }

            // 首次加载成功后，让 createVolume 抛异常。
            repo.createVolumeException = RepositoryException(createVolumeFailed)
            val statsBefore = vm.uiState.value.projectStats
            vm.createVolume("新卷")
            settleUntil { events.any { it is ProjectTreeUiEvent.Error } }

            // #617 评论九：createVolume 抛异常 → mutateAndRefresh 失败：
            // 不发 refreshProject（卷列表不变），只发错误事件。
            assertEquals(
                "卷列表不得因失败的 create 变化",
                listOf("vA"),
                vm.uiState.value.volumes.map { it.id },
            )
            assertEquals("统计不得因失败的 create 变化", statsBefore, vm.uiState.value.projectStats)
            assertTrue(mustEmitError, events.any { it is ProjectTreeUiEvent.Error })
            assertEquals(
                createVolumeFailed,
                (events.filterIsInstance<ProjectTreeUiEvent.Error>().first()).message,
            )
            collectJob.cancel()
        }

    @Test
    fun createChapterThrows_doesNotExpandAndEmitsError() =
        runTest(mainDispatcherRule.dispatcher) {
            val app = RuntimeEnvironment.getApplication()
            val repo =
                FakeProjectRepository(
                    context = app,
                    volumes = mapOf("A" to volumesA),
                    stats = mapOf("A" to ProjectStats(100, 1, 1)),
                )
            val vm = ProjectViewModel(SavedStateHandle())
            val (events, collectJob) = collectEvents(vm)

            vm.initialize("A", repo)
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vA") &&
                    !vm.uiState.value.isLoading
            }

            // createChapter 抛异常。
            repo.createChapterException = RepositoryException(createChapterFailed)
            vm.createChapter("vA", "新章")
            settleUntil { events.any { it is ProjectTreeUiEvent.Error } }

            // #617 评论九：createChapter 失败 → 不加入 expandedVolumeIds（卷不展开），发 Error。
            assertFalse(
                "失败的 createChapter 不得展开卷",
                vm.uiState.value.expandedVolumeIds.contains("vA"),
            )
            assertTrue(mustEmitError, events.any { it is ProjectTreeUiEvent.Error })
            assertEquals(
                createChapterFailed,
                (events.filterIsInstance<ProjectTreeUiEvent.Error>().first()).message,
            )
            collectJob.cancel()
        }

    @Test
    fun refreshFailureAfterSuccessfulLoad_keepsOldDataAndEmitsError() =
        runTest(mainDispatcherRule.dispatcher) {
            // #617 评论九：重载失败（已有成功数据后再次 refresh 失败）— 必须保留上一次
            // 成功的 volumes/projectStats，不得写 emptyList()/null 覆盖；已有数据时不设
            // loadError（首次加载才设），只发一次性 Error 事件交 Snackbar。
            val app = RuntimeEnvironment.getApplication()
            val repo =
                FakeProjectRepository(
                    context = app,
                    volumes = mapOf("A" to volumesA),
                    stats = mapOf("A" to ProjectStats(100, 1, 1)),
                )
            val vm = ProjectViewModel(SavedStateHandle())
            val (events, collectJob) = collectEvents(vm)

            vm.initialize("A", repo)
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vA") &&
                    vm.uiState.value.projectStats?.totalWordCount == 100 &&
                    !vm.uiState.value.isLoading
            }

            // 首次加载成功后让 getVolumes 抛异常：createVolume 成功 → refreshProject 失败。
            repo.getVolumesException = RepositoryException(loadVolumesFailed)
            vm.createVolume("新卷")
            settleUntil { events.any { it is ProjectTreeUiEvent.Error } }

            assertEquals(
                "重载失败不得把已有卷列表覆盖为空",
                listOf("vA"),
                vm.uiState.value.volumes.map { it.id },
            )
            assertEquals(
                "重载失败不得把已有统计覆盖为 null",
                100,
                vm.uiState.value.projectStats?.totalWordCount,
            )
            assertFalse("重载失败后不得停留在 loading 态", vm.uiState.value.isLoading)
            assertNull("已有成功数据时重载失败不得设 loadError", vm.uiState.value.loadError)
            assertTrue(mustEmitError, events.any { it is ProjectTreeUiEvent.Error })
            assertEquals(
                loadVolumesFailed,
                (events.filterIsInstance<ProjectTreeUiEvent.Error>().first()).message,
            )
            collectJob.cancel()
        }

    @Test
    fun firstLoadFailure_thenSuccessfulMutationRefresh_clearsLoadError() =
        runTest(mainDispatcherRule.dispatcher) {
            // #617 评论九：首次加载失败设 loadError 后，后续成功刷新（如重试/变更）
            // 必须清掉 loadError 并填充真实数据 — 错误态不得永久粘滞。
            val app = RuntimeEnvironment.getApplication()
            val repo =
                FakeProjectRepository(
                    context = app,
                    volumes = mapOf("A" to volumesA),
                    stats = mapOf("A" to ProjectStats(100, 1, 1)),
                )
            val vm = ProjectViewModel(SavedStateHandle())

            repo.getVolumesException = RepositoryException(loadVolumesFailed)
            vm.initialize("A", repo)
            settleUntil { vm.uiState.value.loadError != null }
            assertNotNull("首次加载失败必须设 loadError", vm.uiState.value.loadError)

            // 仓库恢复后触发一次成功变更刷新（createVolume 成功 → refreshProject）。
            repo.getVolumesException = null
            vm.createVolume("新卷")
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vA") &&
                    !vm.uiState.value.isLoading
            }

            assertNull("成功刷新后必须清掉 loadError", vm.uiState.value.loadError)
            assertEquals(
                "成功刷新后必须填充真实卷数据",
                listOf("vA"),
                vm.uiState.value.volumes.map { it.id },
            )
            assertEquals(
                "成功刷新后统计恢复",
                100,
                vm.uiState.value.projectStats?.totalWordCount,
            )
        }

    @Test
    fun createChapterSuccess_expandsAndRefreshes() =
        runTest(mainDispatcherRule.dispatcher) {
            val app = RuntimeEnvironment.getApplication()
            val repo =
                FakeProjectRepository(
                    context = app,
                    volumes = mapOf("A" to volumesA),
                    stats = mapOf("A" to ProjectStats(100, 1, 1)),
                )
            val vm = ProjectViewModel(SavedStateHandle())

            vm.initialize("A", repo)
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vA") &&
                    !vm.uiState.value.isLoading
            }

            // createChapter 成功（无异常注入）。
            vm.createChapter("vA", "新章")
            settleUntil {
                vm.uiState.value.expandedVolumeIds.contains("vA") &&
                    !vm.uiState.value.isLoading
            }

            // #617 评论九：createChapter 成功 → 加入 expandedVolumeIds + refreshProject。
            assertTrue(
                "成功的 createChapter 必须展开卷",
                vm.uiState.value.expandedVolumeIds.contains("vA"),
            )
            assertEquals(
                "refresh 后卷列表仍为 volumesA",
                listOf("vA"),
                vm.uiState.value.volumes.map { it.id },
            )
        }
}
