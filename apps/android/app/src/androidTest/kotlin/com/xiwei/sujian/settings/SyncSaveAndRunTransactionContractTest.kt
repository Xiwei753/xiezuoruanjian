package com.xiwei.sujian.settings

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.xiwei.sujian.model.SyncConfig
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.ui.compose.settings.SettingsIntent
import com.xiwei.sujian.ui.compose.settings.SettingsViewModel
import com.xiwei.sujian.ui.compose.settings.SyncCommandState
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * “保存并同步”串行事务契约（#592 三）：
 *
 * - PerformSync 必须通过 SaveSyncAndRun 命令进入 saveChannel 串行队列，
 *   不允许绕过保存队列直接写 config/secrets；
 * - 事务只保存“捕获 revision 对应的值”：若保存期间已有更新版本的修改排队/合并，
 *   事务不得用旧值反向覆盖磁盘新值，也不得把更新版本误标记为已保存；
 * - 所有路径都必须结束在明确终态（未配置 → 失败），不允许停留在 RUNNING/IDLE。
 *
 * 使用真实 Core 工作区（TestDependenciesRule），config 经 Core 持久化往返。
 */
@RunWith(AndroidJUnit4::class)
class SyncSaveAndRunTransactionContractTest {

    @get:Rule
    val rule = AndroidTestEnvironment.TestDependenciesRule()

    @Test
    fun performSyncTransaction_doesNotOverwriteNewerQueuedConfig() {
        val session = AndroidTestEnvironment.requireCurrentSession()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val repo = session.deps.settingsRepository

        val configA = SyncConfig(enabled = false, autoSync = false, remoteUrl = "https://a.example/repo.git")
        val configB = SyncConfig(enabled = false, autoSync = false, remoteUrl = "https://b.example/repo.git")

        var vm: SettingsViewModel? = null
        instrumentation.runOnMainSync {
            vm = SettingsViewModel(repo, session.deps.syncCoordinator)
        }

        // 同一批次进入保存队列：旧配置编辑 → 保存并同步（捕获旧值）→ 更新为新配置。
        instrumentation.runOnMainSync {
            vm!!.handleIntent(SettingsIntent.UpdateSyncConfig(configA))
            vm!!.handleIntent(SettingsIntent.PerformSync)
            vm!!.handleIntent(SettingsIntent.UpdateSyncConfig(configB))
        }

        awaitTerminalState(instrumentation, vm!!)

        assertEquals(
            "未配置同步时事务必须以失败终态结束，不允许停留在 RUNNING/IDLE",
            SyncCommandState.FAILURE,
            vm!!.uiState.value.performSyncState,
        )
        val persisted = repo.loadSyncConfig()
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

        val config = SyncConfig(enabled = false, autoSync = false, remoteUrl = "https://c.example/repo.git")

        var vm: SettingsViewModel? = null
        instrumentation.runOnMainSync {
            vm = SettingsViewModel(repo, session.deps.syncCoordinator)
        }
        instrumentation.runOnMainSync {
            vm!!.handleIntent(SettingsIntent.UpdateSyncConfig(config))
            vm!!.handleIntent(SettingsIntent.PerformSync)
        }

        awaitTerminalState(instrumentation, vm!!)

        assertEquals(SyncCommandState.FAILURE, vm!!.uiState.value.performSyncState)
        assertEquals(
            "无并发新编辑时事务必须保存捕获 revision 对应的配置",
            "https://c.example/repo.git",
            repo.loadSyncConfig().remoteUrl,
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
            val state = vm.uiState.value.performSyncState
            if (state == SyncCommandState.SUCCESS || state == SyncCommandState.FAILURE) return
            Thread.sleep(50)
        }
        fail(
            "SaveSyncAndRun transaction did not reach terminal state within ${timeoutMs}ms; " +
                "state=${vm.uiState.value.performSyncState}",
        )
    }
}
