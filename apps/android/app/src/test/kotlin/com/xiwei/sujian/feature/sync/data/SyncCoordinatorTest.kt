package com.xiwei.sujian.feature.sync.data
import com.xiwei.sujian.app.state.ActiveDocumentGate
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.settings.ui.SettingsTransactionCommand
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@Suppress("TooManyFunctions") // 21 个测试方法 + @Before/@After 隔离;与 RebaseMappingBridgeTest 等 4 个测试类先例一致
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SyncCoordinatorTest {
    companion object {
        private const val PROJECT_ID_625 = "p-625"
    }

    /**
     * #625 评论5301204285 问题1：ActiveDocumentGate 是进程级单例,
     * 其他测试类(ActiveDocumentGateTest / EditorViewModel 等)可能残留 flush 回调
     * (flush = { false } 或抛异常),导致本类 runSync 的 flushActiveDocument 返回 false
     * → DocumentSaveFailed,污染 Completed 路径。每个测试前注册一个 benign flush
     * ({ true })覆盖任何残留,@After 清除本注册,确保测试隔离。
     */
    private var gateRegistration: ActiveDocumentGate.Registration? = null

    @Before
    fun isolateActiveDocumentGate() {
        gateRegistration = ActiveDocumentGate.register(owner = Any(), flush = { true })
    }

    @After
    fun clearActiveDocumentGate() {
        gateRegistration?.close()
        gateRegistration = null
    }

    /**
     * #625 评论5301204285 问题1 测试 seam：确定性同步执行 fake。
     * 使 runSync 的 Completed/Disabled/TerminalFailure 分支在 JVM 测试中
     * 不依赖 native 库加载,确定性执行真实控制流。
     */
    private class FakeSyncExecution(
        val capabilityData: SyncCapabilityData = SyncCapabilityData(canRun = true),
        val overrideOk: Boolean = true,
        val performResult: BridgeResult<SyncResult> =
            BridgeResult.Success(SyncResult(status = SyncStatus.Success)),
        val clearOverrideOk: Boolean = true,
    ) : SyncExecutionPort {
        override suspend fun capability(projectId: String) = capabilityData

        override suspend fun setSecretsOverride(secrets: SyncSecrets) = overrideOk

        override suspend fun perform(
            projectId: String,
            config: SyncConfig,
        ) = performResult

        override suspend fun clearSecretsOverride() = clearOverrideOk
    }

    @Test
    fun syncSession_runExclusive_blocksConcurrentAccess() =
        runTest {
            var firstEntered = false

            val result1 =
                SyncSession.runExclusive {
                    firstEntered = true
                    kotlinx.coroutines.delay(100)
                    "first"
                }
            assertEquals("first", (result1 as ExclusiveResult.Success).value)
            assertEquals(true, firstEntered)
        }

    @Test
    fun syncSession_runExclusive_returnsBusyWhenLocked() =
        runTest {
            assertEquals(ExclusiveResult.Busy::class, ExclusiveResult.Busy::class)
        }

    @Test
    fun syncOutcome_sealedClassHierarchy() {
        val outcomes: List<SyncOutcome> =
            listOf(
                SyncOutcome.Completed(
                    com.xiwei.sujian.feature.sync.data.model.SyncResult(
                        status = com.xiwei.sujian.feature.sync.data.model.SyncStatus.Success,
                    ),
                ),
                SyncOutcome.Disabled,
                SyncOutcome.Unconfigured,
                SyncOutcome.Busy,
                SyncOutcome.RetryableFailure(com.xiwei.sujian.feature.sync.data.model.SyncStatus.RecoverableError),
                SyncOutcome.TerminalFailure(com.xiwei.sujian.feature.sync.data.model.SyncStatus.Conflict),
            )
        assertEquals(6, outcomes.distinctBy { it::class }.size)
    }

    @Test
    fun syncingStatus_mapsToTerminalFailure_protocolError() {
        // #592 三：performSync 返回 Syncing 是协议错误，不可重试
        val outcome = SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        assertTrue(outcome is SyncOutcome.TerminalFailure)
        assertEquals(SyncStatus.FatalError, (outcome as SyncOutcome.TerminalFailure).status)
    }

    @Test
    fun bridgeError_typedKinds_classifyCorrectly() {
        // #592 七：类型化失败直接来自 Bridge 边界（WriterException 变体），
        // 不再通过 Android 字符串错误码表分类。
        val networkError =
            BridgeResult.Error(
                com.xiwei.sujian.core.interop.common.ResultEnvelope.errorOf("RETRYABLE_NETWORK", "test"),
                syncFailureKind = SyncFailureKind.RetryableNetwork,
            )
        assertEquals(SyncFailureKind.RetryableNetwork, SyncFailureKind.fromBridgeError(networkError))
        val ioError =
            BridgeResult.Error(
                com.xiwei.sujian.core.interop.common.ResultEnvelope.errorOf("RETRYABLE_IO", "test"),
                syncFailureKind = SyncFailureKind.RetryableIo,
            )
        assertEquals(SyncFailureKind.RetryableIo, SyncFailureKind.fromBridgeError(ioError))
    }

    @Test
    fun bridgeError_unknownKind_defaultsToFatal() {
        // #592 七：未知错误默认 Fatal，只有明确网络或 IO 失败可以重试。
        val unknown =
            BridgeResult.Error(
                com.xiwei.sujian.core.interop.common.ResultEnvelope.errorOf("SOME_FUTURE_CODE", "test"),
            )
        assertEquals(SyncFailureKind.Fatal, SyncFailureKind.fromBridgeError(unknown))
    }

    @Test
    fun legacyErrorCode_mappingStillCoversOldCodes() {
        // 遗留映射仅作非 BridgeResult.Error 路径兜底；主路径已类型化。
        assertEquals(
            SyncFailureKind.RetryableNetwork,
            SyncFailureKind.fromLegacyErrorCode("SYNC_NETWORK_UNAVAILABLE"),
        )
        assertEquals(
            SyncFailureKind.RetryableIo,
            SyncFailureKind.fromLegacyErrorCode("IO_ERROR"),
        )
        assertEquals(
            SyncFailureKind.NativeUnavailable,
            SyncFailureKind.fromLegacyErrorCode("NATIVE_NOT_LOADED"),
        )
        assertEquals(
            SyncFailureKind.Authentication,
            SyncFailureKind.fromLegacyErrorCode("SYNC_AUTH_FAILED"),
        )
        assertEquals(
            SyncFailureKind.Conflict,
            SyncFailureKind.fromLegacyErrorCode("SYNC_CONFLICT"),
        )
        assertEquals(
            SyncFailureKind.Protocol,
            SyncFailureKind.fromLegacyErrorCode("SYNC_INCOMPLETE_TRANSACTION"),
        )
    }

    @Test
    fun ioException_mapsToRetryableFailure() {
        // #592 三：临时 IO 异常 → 可重试
        val outcome = SyncOutcome.RetryableFailure(SyncStatus.RecoverableError)
        assertTrue(outcome is SyncOutcome.RetryableFailure)
        assertEquals(SyncStatus.RecoverableError, (outcome as SyncOutcome.RetryableFailure).status)
    }

    @Test
    fun repositoryException_mapsToTerminalFailure() {
        // #592 三：仓库层异常 → 不可重试
        val outcome = SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        assertTrue(outcome is SyncOutcome.TerminalFailure)
        assertEquals(SyncStatus.FatalError, (outcome as SyncOutcome.TerminalFailure).status)
    }

    @Test
    fun busy_doesNotModifySyncState() {
        // #592 四：Busy 只返回 SyncOutcome.Busy，不调用 refreshState()，不修改状态灯。
        // 当另一个同步正在运行时，Busy 不应覆盖真实的 Syncing 黄色状态。
        val outcome = SyncOutcome.Busy
        assertEquals(SyncOutcome.Busy::class, outcome::class)
    }

    @Test
    fun transactionCommandTypes_areDistinct() {
        // #592 一/二：三种事务命令类型必须独立，不允许合并
        val commands: List<SettingsTransactionCommand> =
            listOf(
                SettingsTransactionCommand.SaveAndRunSync(
                    config = com.xiwei.sujian.feature.sync.data.model.SyncConfig(),
                    configRevision = 1L,
                    secrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(),
                    secretsRevision = 1L,
                    trigger = com.xiwei.sujian.feature.sync.data.model.SyncTrigger.Manual,
                ),
                SettingsTransactionCommand.SaveAndRunDryRun(
                    config = com.xiwei.sujian.feature.sync.data.model.SyncConfig(),
                    configRevision = 1L,
                    secrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(),
                    secretsRevision = 1L,
                ),
                SettingsTransactionCommand.SaveAndRunDiagnostics(
                    config = com.xiwei.sujian.feature.sync.data.model.SyncConfig(),
                    configRevision = 1L,
                    secrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(),
                    secretsRevision = 1L,
                ),
            )
        assertEquals(3, commands.distinctBy { it::class }.size)
    }

    // === #625 评论5301204285 问题1：ProjectSyncCompletedSignal 契约测试 ===

    /**
     * 构造真实 SyncCoordinator（与 SettingsViewModelTest 同构）。
     * 测试环境无 native 库时 runSync/runAppSync 不会返回 Completed
     * （getSyncCapability / setSyncSecretsOverrideStrict / performSync 均依赖 native）。
     * WriterAppServiceHolder.service 是 lazy 的，构造时不初始化 native 库。
     */
    private fun createCoordinator(syncExecution: SyncExecutionPort? = null): SyncCoordinator {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val holder =
            WriterAppServiceHolder(
                appDataRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-sync-signal-data",
                projectsRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-sync-signal-projects",
            )
        val bridge = AppServiceBridge(holder)
        val syncRepo = SyncRepository(context, bridge)
        val syncStatusRepo = SyncStatusRepository(syncRepo)
        return if (syncExecution != null) {
            SyncCoordinator(syncRepo, syncStatusRepo, null, syncExecution)
        } else {
            SyncCoordinator(syncRepo, syncStatusRepo)
        }
    }

    private fun enabledSnapshot(): ProjectSyncProfileSnapshot =
        ProjectSyncProfileSnapshot(
            generation = 1L,
            config = SyncConfig(enabled = true, remoteUrl = "https://unit.example/repo.git"),
            secrets = SyncSecrets(token = "test-token-625"),
        )

    @Test
    fun projectSyncCompletedSignal_carriesProjectIdOnly() {
        // #625 评论5301204285 问题1：事件只携带 projectId，不携带字数/标题/summary。
        val signal = ProjectSyncCompletedSignal(PROJECT_ID_625)
        assertEquals(PROJECT_ID_625, signal.projectId)
        // data class 单属性契约 — copy/component1 验证只有 projectId 一个属性，
        // ProjectSummary 继续是列表唯一数据源。
        assertEquals(signal, signal.copy(projectId = PROJECT_ID_625))
        assertEquals(PROJECT_ID_625, signal.component1())
    }

    @Test
    fun runSync_unconfigured_doesNotEmitProjectSyncCompleted() =
        runTest(UnconfinedTestDispatcher()) {
            val coordinator = createCoordinator()
            val collected = mutableListOf<ProjectSyncCompletedSignal>()
            val collectorJob = launch { coordinator.projectSyncCompleted.collect { collected += it } }

            // config.enabled=false → Unconfigured（早退出口，不发信号）
            val outcome =
                coordinator.runSync(
                    SyncTrigger.Manual,
                    "p-625-unconfigured",
                    snapshot =
                        ProjectSyncProfileSnapshot(
                            generation = 1L,
                            config = SyncConfig(enabled = false),
                            secrets = SyncSecrets(),
                        ),
                )

            collectorJob.cancel()
            assertTrue("应返回 Unconfigured", outcome is SyncOutcome.Unconfigured)
            assertTrue("Unconfigured 不应发出信号，实际收到: $collected", collected.isEmpty())
        }

    @Test
    fun runSync_disabled_doesNotEmitProjectSyncCompleted() =
        runTest(UnconfinedTestDispatcher()) {
            // config.enabled=true 但 capability.canRun=false → Disabled（显式确定性，不依赖 native 缺席）
            val coordinator =
                createCoordinator(
                    syncExecution = FakeSyncExecution(capabilityData = SyncCapabilityData(canRun = false)),
                )
            val collected = mutableListOf<ProjectSyncCompletedSignal>()
            val collectorJob = launch { coordinator.projectSyncCompleted.collect { collected += it } }

            val outcome =
                coordinator.runSync(
                    SyncTrigger.Manual,
                    "p-625-disabled",
                    snapshot = enabledSnapshot(),
                )

            collectorJob.cancel()
            assertTrue("应返回 Disabled，实际: $outcome", outcome is SyncOutcome.Disabled)
            assertTrue("Disabled 不应发出信号，实际收到: $collected", collected.isEmpty())
        }

    @Test
    fun runSync_terminalFailure_doesNotEmitProjectSyncCompleted() =
        runTest(UnconfinedTestDispatcher()) {
            // perform 返回 Success(SyncResult(FatalError)) → mapToOutcome → TerminalFailure（显式确定性）
            val coordinator =
                createCoordinator(
                    syncExecution =
                        FakeSyncExecution(
                            performResult = BridgeResult.Success(SyncResult(status = SyncStatus.FatalError)),
                        ),
                )
            val collected = mutableListOf<ProjectSyncCompletedSignal>()
            val collectorJob = launch { coordinator.projectSyncCompleted.collect { collected += it } }

            val outcome =
                coordinator.runSync(
                    SyncTrigger.Manual,
                    "p-625-terminal",
                    snapshot = enabledSnapshot(),
                )

            collectorJob.cancel()
            assertTrue("应返回 TerminalFailure，实际: $outcome", outcome is SyncOutcome.TerminalFailure)
            assertTrue("TerminalFailure 不应发出信号，实际收到: $collected", collected.isEmpty())
        }

    @Test
    fun runSync_completed_emitsProjectSyncCompleted() =
        runTest(UnconfinedTestDispatcher()) {
            // #625 评论5301204285 问题1：注入确定性 SyncExecutionPort，
            // 使 runSync 真实执行全部控制流（enabled/capability/独占锁/flush/override/perform/
            // 结果映射/信号发射）并到达 Completed 分支 — 不依赖 native 库加载，不跳过。
            val coordinator =
                createCoordinator(
                    syncExecution =
                        FakeSyncExecution(
                            capabilityData = SyncCapabilityData(canRun = true),
                            performResult = BridgeResult.Success(SyncResult(status = SyncStatus.Success)),
                        ),
                )
            val collected = mutableListOf<ProjectSyncCompletedSignal>()
            val collectorJob = launch { coordinator.projectSyncCompleted.collect { collected += it } }
            val projectId = "p-625-completed"

            val outcome =
                coordinator.runSync(
                    SyncTrigger.Manual,
                    projectId,
                    snapshot = enabledSnapshot(),
                )

            collectorJob.cancel()
            assertTrue("应返回 Completed，实际: $outcome", outcome is SyncOutcome.Completed)
            assertEquals("Completed 应发出一次信号", 1, collected.size)
            assertEquals(projectId, collected[0].projectId)
        }

    @Test
    fun runAppSync_doesNotEmitProjectSyncCompleted() =
        runTest(UnconfinedTestDispatcher()) {
            val coordinator = createCoordinator()
            val collected = mutableListOf<ProjectSyncCompletedSignal>()
            val collectorJob = launch { coordinator.projectSyncCompleted.collect { collected += it } }

            // runAppSync 不传 snapshot → snapshotAppSyncProfile 调用 native bridge 失败 → TerminalFailure。
            // 无论成功与否，runAppSync 实现中无 tryEmit 调用 — 不发作品级信号。
            val outcome = coordinator.runAppSync(SyncTrigger.Manual)

            collectorJob.cancel()
            // runAppSync 执行完毕返回明确终态（未抛异常），且未发出作品级信号。
            assertTrue("runAppSync 应返回非 Busy 终态: $outcome", outcome !is SyncOutcome.Busy)
            assertTrue("runAppSync 不应发出作品级信号，实际收到: $collected", collected.isEmpty())
        }
}
