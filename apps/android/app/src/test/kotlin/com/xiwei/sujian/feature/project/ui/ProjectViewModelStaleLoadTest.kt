package com.xiwei.sujian.feature.project.ui

// #617 复审：章节树跨作品切换的陈旧加载结果不得覆盖新作品的卷/统计 — 确定性回归测试。
//
// 复现场景：ChapterTreeContent 的 viewModel() 属于 Works NavEntry，作品切换时
// 同一 ProjectViewModel 复用（LaunchedEffect 以 projectId 为 key 重新 initialize）。
// viewModelScope 协程不受 LaunchedEffect 重启取消：旧作品 A 的加载在真实 IO 上
// 挂起时切到作品 B，若 A 的结果迟到写回，会把 B 的章节树覆盖成 A 的数据。
//
// 确定性手段：
// - StandardTestDispatcher：viewModelScope 的任务排队在测试主调度器上，由
//   runCurrent() 精确驱动；真实 IO 的完成用 runCurrent()+轮询等待落定；
// - GatedProjectRepository 按 projectId 用 CountDownLatch 阻塞/放行
//   getVolumes / getChapters / getProjectStats，并记录"进入/返回"标记：
//   先证明 A 的加载已挂在章节栅栏上（进入标记），再放行，让 A 的迟到写回
//   在观察窗口内进入主队列 — 必须被加载纪元校验拒绝。

import androidx.lifecycle.SavedStateHandle
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ProjectStats
import com.xiwei.sujian.feature.project.data.model.Volume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ProjectViewModelStaleLoadTest {
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

    /** 按 projectId 控制三个数据读取的阻塞/放行，并记录调用进度。 */
    private class GatedProjectRepository(
        context: android.content.Context,
        private val volumes: Map<String, List<Volume>>,
        stats: Map<String, ProjectStats>,
        private val volumeGates: Map<String, CountDownLatch> = emptyMap(),
        private val chapterGates: Map<String, CountDownLatch> = emptyMap(),
        private val statsGates: Map<String, CountDownLatch> = emptyMap(),
    ) : ProjectRepository(context) {
        private val statsMap = stats.toMutableMap()

        /** 变更测试用：模拟 Core 侧数据变化（创建/删除卷章节后统计变化）。 */
        fun setStats(
            projectId: String,
            newStats: ProjectStats,
        ) {
            statsMap[projectId] = newStats
        }

        /** getVolumes("A", …) 已进入（真实 IO 侧已开始执行，尚未放行）。 */
        val volumeAEntered = AtomicBoolean(false)

        /** getChapters("A", …) 已进入（挂在章节栅栏上）。 */
        val chapterAEntered = AtomicBoolean(false)

        /** getChapters("A", …) 已返回（IO 侧完成，迟到写回即将入队）。 */
        val chapterAReturned = AtomicBoolean(false)

        /** getProjectStats("A") 已返回。 */
        val statsAReturned = AtomicBoolean(false)

        private fun await(gate: CountDownLatch?) {
            check(gate == null || gate.await(10, TimeUnit.SECONDS)) { "测试栅栏超时" }
        }

        override fun getVolumes(projectId: String): List<Volume> {
            if (projectId == "A") volumeAEntered.set(true)
            await(volumeGates[projectId])
            return volumes[projectId].orEmpty()
        }

        override fun getChapters(
            projectId: String,
            volumeId: String,
        ): List<ChapterMeta> {
            if (projectId == "A") chapterAEntered.set(true)
            await(chapterGates[projectId])
            if (projectId == "A") chapterAReturned.set(true)
            return emptyList()
        }

        override fun getProjectStats(projectId: String): ProjectStats {
            await(statsGates[projectId])
            if (projectId == "A") statsAReturned.set(true)
            return statsMap[projectId] ?: ProjectStats(0, 0, 0)
        }
    }

    private fun volume(
        id: String,
        title: String,
    ) = Volume(id, title, "2026-01-01T00:00:00", "2026-01-01T00:00:00")

    private val volumesA = listOf(volume("vA", "A卷"))
    private val volumesB = listOf(volume("vB", "B卷"))

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

    @Test
    fun staleVolumesAndStats_mustNotOverwriteNewerProjectData() =
        runTest(mainDispatcherRule.dispatcher) {
            val app = RuntimeEnvironment.getApplication()
            val volumeGateA = CountDownLatch(1)
            val chapterGateA = CountDownLatch(1)
            val statsGateA = CountDownLatch(1)
            val repo =
                GatedProjectRepository(
                    context = app,
                    volumes = mapOf("A" to volumesA, "B" to volumesB),
                    stats = mapOf("A" to ProjectStats(100, 1, 1), "B" to ProjectStats(200, 2, 2)),
                    volumeGates = mapOf("A" to volumeGateA),
                    chapterGates = mapOf("A" to chapterGateA),
                    statsGates = mapOf("A" to statsGateA),
                )
            val vm = ProjectViewModel(SavedStateHandle())

            // 1. 进入作品 A：卷/章节/统计三处读取都挂在栅栏上（真实 IO 挂起）。
            //    必须先等到 A 的 IO 块真实开始执行（进入 getVolumes），
            //    否则切 B 时取消会使 withContext 根本不启动 A 的 IO 块。
            vm.initialize("A", repo)
            runCurrent()
            settleUntil { repo.volumeAEntered.get() }

            // 2. 切到作品 B：B 无栅栏，加载立即完成并写回。
            vm.initialize("B", repo)
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vB") &&
                    vm.uiState.value.projectStats?.totalWordCount == 200 &&
                    !vm.uiState.value.isLoading
            }

            // 3. 放行 A 的卷栅栏：A 的加载继续并停在章节栅栏上（确定性证明）。
            volumeGateA.countDown()
            settleUntil { repo.chapterAEntered.get() }

            // 4. 放行 A 的统计栅栏。刷新链内读取顺序为 卷 → 章节 → 统计，
            //    统计读取发生在章节放行之后，先放行统计栅栏确保它到达时已开放。
            statsGateA.countDown()

            // 5. 放行 A 的章节栅栏：A 的 IO 链跑完（含统计读取），迟到写回进入主队列 —
            //    观察窗口内必须被丢弃。
            chapterGateA.countDown()
            settleUntil { repo.chapterAReturned.get() && repo.statsAReturned.get() }
            assertEquals(
                "迟到的 A 统计不得覆盖 B 的统计",
                200,
                vm.uiState.value.projectStats?.totalWordCount,
            )
            var attempts = 0
            var staleLanded = false
            while (attempts < 400 && !staleLanded) {
                runCurrent()
                Thread.sleep(5)
                attempts++
                staleLanded = vm.uiState.value.volumes.any { it.id == "vA" }
            }
            assertFalse("迟到的 A 卷数据不得覆盖 B 的章节树", staleLanded)
            assertEquals(
                "B 的章节树必须保持",
                listOf("vB"),
                vm.uiState.value.volumes.map { it.id },
            )
            assertEquals("迟到的 A 数据不得残留加载态", false, vm.uiState.value.isLoading)
        }

    @Test
    fun freshReloadAfterStaleDrop_stillWritesCurrentProjectData() =
        runTest(mainDispatcherRule.dispatcher) {
            val app = RuntimeEnvironment.getApplication()
            val volumeGateA = CountDownLatch(1)
            val repo =
                GatedProjectRepository(
                    context = app,
                    volumes = mapOf("A" to volumesA, "B" to volumesB),
                    stats = mapOf("A" to ProjectStats(100, 1, 1), "B" to ProjectStats(200, 2, 2)),
                    volumeGates = mapOf("A" to volumeGateA),
                )
            val vm = ProjectViewModel(SavedStateHandle())

            vm.initialize("A", repo)
            runCurrent()
            settleUntil { repo.volumeAEntered.get() }
            vm.initialize("B", repo)
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vB") &&
                    vm.uiState.value.projectStats?.totalWordCount == 200
            }

            // 旧 A 加载放行后迟到完成 — 必须被纪元拒绝，不得覆盖 B。
            volumeGateA.countDown()
            settleUntil { repo.chapterAReturned.get() }
            assertEquals(listOf("vB"), vm.uiState.value.volumes.map { it.id })

            // 再次真实切回 A：新加载必须正常写回，纪元守卫不得误伤合法加载。
            vm.initialize("A", repo)
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vA") &&
                    vm.uiState.value.projectStats?.totalWordCount == 100 &&
                    !vm.uiState.value.isLoading
            }
        }

    @Test
    fun projectSwitch_clearsStaleUiStateAndKeepsExpansionPerProject() =
        runTest(mainDispatcherRule.dispatcher) {
            val app = RuntimeEnvironment.getApplication()
            val volumeGateB = CountDownLatch(1)
            val repo =
                GatedProjectRepository(
                    context = app,
                    volumes = mapOf("A" to volumesA, "B" to volumesB),
                    stats = mapOf("A" to ProjectStats(100, 1, 1), "B" to ProjectStats(200, 2, 2)),
                    volumeGates = mapOf("B" to volumeGateB),
                )
            val vm = ProjectViewModel(SavedStateHandle())

            vm.initialize("A", repo)
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vA") &&
                    !vm.uiState.value.isLoading
            }
            vm.toggleVolumeExpand("vA")
            vm.selectChapter("chA")
            assertEquals(setOf("vA"), vm.uiState.value.expandedVolumeIds)

            // 切到 B，B 的加载挂在栅栏上 — 旧 A 的卷/选中章节/统计必须立即清掉。
            vm.initialize("B", repo)
            runCurrent()
            assertTrue("切作品后必须进入加载态", vm.uiState.value.isLoading)
            assertTrue("旧作品的卷列表不得残留", vm.uiState.value.volumes.isEmpty())
            assertNull("旧作品的选中章节不得残留", vm.uiState.value.selectedChapterId)
            assertNull("旧作品的统计不得残留", vm.uiState.value.projectStats)
            assertTrue("A 的展开状态不得混入 B", vm.uiState.value.expandedVolumeIds.isEmpty())

            volumeGateB.countDown()
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vB") &&
                    !vm.uiState.value.isLoading
            }
            vm.toggleVolumeExpand("vB")
            assertEquals(setOf("vB"), vm.uiState.value.expandedVolumeIds)

            // 切回 A：展开状态按 projectId 独立恢复，B 的展开不得混入。
            vm.initialize("A", repo)
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vA") &&
                    !vm.uiState.value.isLoading
            }
            assertEquals(
                "A 的展开状态必须按 projectId 恢复",
                setOf("vA"),
                vm.uiState.value.expandedVolumeIds,
            )
            assertNull("切换后选中章节重置", vm.uiState.value.selectedChapterId)
        }

    @Test
    fun mutationRefresh_reloadsStatsAndKeepsCurrentProject() =
        runTest(mainDispatcherRule.dispatcher) {
            val app = RuntimeEnvironment.getApplication()
            val repo =
                GatedProjectRepository(
                    context = app,
                    volumes = mapOf("A" to volumesA),
                    stats = mapOf("A" to ProjectStats(100, 1, 1)),
                )
            val vm = ProjectViewModel(SavedStateHandle())

            vm.initialize("A", repo)
            settleUntil {
                vm.uiState.value.volumes.map { it.id } == listOf("vA") &&
                    vm.uiState.value.projectStats?.totalWordCount == 100 &&
                    !vm.uiState.value.isLoading
            }

            // 模拟 Core 侧数据变化（创建/删除卷章节后统计变化），再触发一次变更刷新。
            repo.setStats("A", ProjectStats(300, 3, 3))
            vm.createVolume("新卷")
            settleUntil {
                vm.uiState.value.projectStats?.totalWordCount == 300 &&
                    !vm.uiState.value.isLoading
            }
            // 测试环境 native 未加载：createVolume 失败被吞掉，卷列表保持原状，
            // 但变更刷新必须重读统计（#617 评论七：变更后不能只刷卷列表）。
            assertEquals("卷列表不得因失败的 create 变化", listOf("vA"), vm.uiState.value.volumes.map { it.id })
            assertEquals(300, vm.uiState.value.projectStats?.totalWordCount)
        }
}
