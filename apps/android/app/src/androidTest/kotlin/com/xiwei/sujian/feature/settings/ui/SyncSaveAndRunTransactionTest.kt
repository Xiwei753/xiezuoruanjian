package com.xiwei.sujian.feature.settings.ui

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.xiwei.sujian.app.state.ActiveProjectGate
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.support.AndroidTestEnvironment
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * “保存并同步”串行事务契约（#592 三）：
 *
 * - PerformSync 必须通过 SaveAndRunSync 命令进入 saveChannel 串行队列，
 *   不允许绕过保存队列直接写 config/secrets；
 * - 事务只保存“捕获 revision 对应的值”：若保存期间已有更新版本的修改排队/合并，
 *   事务不得用旧值反向覆盖磁盘新值，也不得把更新版本误标记为已保存；
 * - 所有路径都必须结束在明确终态（未配置 → 失败），不允许停留在 RUNNING/IDLE。
 *
 * 使用真实 Core 工作区（TestDependenciesRule），config 经 Core 持久化往返。
 *
 * #600 评论 #3：同步配置已改为 per-project，测试需创建作品并设为活动作品，
 * 用 projectId 调用 loadSyncConfig。
 */
@RunWith(AndroidJUnit4::class)
class SyncSaveAndRunTransactionTest {
    @get:Rule
    val rule = AndroidTestEnvironment.TestDependenciesRule()

    @Test
    fun performSyncTransaction_doesNotOverwriteNewerQueuedConfig() {
        val session = AndroidTestEnvironment.requireCurrentSession()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val repo = session.deps.settingsRepository
        val projectData =
            AndroidTestEnvironment.ensureTestProjectAndVolume(
                instrumentation.targetContext,
                session,
            )
        ActiveProjectGate.setCurrentProjectId(projectData.projectId)

        val configA = SyncConfig(enabled = false, autoSync = false, remoteUrl = "https://a.example/repo.git")
        val configB = SyncConfig(enabled = false, autoSync = false, remoteUrl = "https://b.example/repo.git")

        var vm: SettingsViewModel? = null
        instrumentation.runOnMainSync {
            vm =
                SettingsViewModel(
                    repo,
                    session.deps.themeRepository,
                    session.deps.syncRepository,
                    session.deps.syncCoordinator,
                )
        }

        // 同一批次进入保存队列：旧配置编辑 → 保存并同步（捕获旧值）→ 更新为新配置。
        instrumentation.runOnMainSync {
            vm!!.handleIntent(SettingsIntent.UpdateProjectSyncConfig(configA))
            vm!!.handleIntent(SettingsIntent.PerformSync)
            vm!!.handleIntent(SettingsIntent.UpdateProjectSyncConfig(configB))
        }

        awaitTerminalState(instrumentation, vm!!)

        assertEquals(
            "未配置同步时事务必须以失败终态结束，不允许停留在 RUNNING/IDLE",
            SyncCommandState.FAILURE,
            vm!!.uiState.value.projectPerformSyncState,
        )
        val persisted = session.deps.syncRepository.loadSyncConfig(projectData.projectId)
        assertEquals(
            "事务不得用捕获的旧值反向覆盖排队中的更新版本（#592）",
            "https://b.example/repo.git",
            persisted.remoteUrl,
        )
    }

    @Test
    fun performSyncTransaction_persistsCapturedConfig_whenNoNewerEdit() {
        val session = AndroidTestEnvironment.requireCurrentSession()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val repo = session.deps.settingsRepository
        val projectData =
            AndroidTestEnvironment.ensureTestProjectAndVolume(
                instrumentation.targetContext,
                session,
            )
        ActiveProjectGate.setCurrentProjectId(projectData.projectId)

        val config = SyncConfig(enabled = false, autoSync = false, remoteUrl = "https://c.example/repo.git")

        var vm: SettingsViewModel? = null
        instrumentation.runOnMainSync {
            vm =
                SettingsViewModel(
                    repo,
                    session.deps.themeRepository,
                    session.deps.syncRepository,
                    session.deps.syncCoordinator,
                )
        }
        instrumentation.runOnMainSync {
            vm!!.handleIntent(SettingsIntent.UpdateProjectSyncConfig(config))
            vm!!.handleIntent(SettingsIntent.PerformSync)
        }

        awaitTerminalState(instrumentation, vm!!)

        assertEquals(SyncCommandState.FAILURE, vm!!.uiState.value.projectPerformSyncState)
        assertEquals(
            "无并发新编辑时事务必须保存捕获 revision 对应的配置",
            "https://c.example/repo.git",
            session.deps.syncRepository.loadSyncConfig(projectData.projectId).remoteUrl,
        )
    }

    private fun awaitTerminalState(
        instrumentation: android.app.Instrumentation,
        vm: SettingsViewModel,
        timeoutMs: Long = 20_000,
    ) {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            instrumentation.waitForIdleSync()
            val state = vm.uiState.value.projectPerformSyncState
            if (state == SyncCommandState.SUCCESS || state == SyncCommandState.FAILURE) return
            Thread.sleep(50)
        }
        fail(
            "SaveAndRunSync transaction did not reach terminal state within ${timeoutMs}ms; " +
                "state=${vm.uiState.value.projectPerformSyncState}",
        )
    }
}
