package com.xiwei.sujian.feature.sync.data
import com.xiwei.sujian.app.state.ActiveDocumentGate
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.settings.ui.SettingsTransactionCommand
import com.xiwei.sujian.feature.sync.data.model.FullSyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
import com.xiwei.sujian.feature.sync.data.model.TargetSyncResult
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

@Suppress("TooManyFunctions")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SyncCoordinatorTest {
    companion object {
        private const val PROJECT_ID_625 = "p-625"
        private const val PREFLIGHT_TARGET = "preflight"

        private fun fullSyncSuccess(targets: List<TargetSyncResult> = emptyList()): FullSyncResult =
            FullSyncResult(
                overallStatus = SyncStatus.Success,
                targets = targets,
                totalUploaded = 0,
                totalDownloaded = 0,
                totalLocalDeletes = 0,
                totalRemoteDeletes = 0,
                totalOverwritten = 0,
                totalIgnored = 0,
                totalConflicts = 0,
                error = null,
                errorCategory = null,
                messageKey = null,
            )
    }

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

    private class FakeSyncExecution(
        val capabilityData: SyncCapabilityData = SyncCapabilityData(canRun = true),
        val overrideOk: Boolean = true,
        val performResult: BridgeResult<FullSyncResult> =
            BridgeResult.Success(fullSyncSuccess()),
        val clearOverrideOk: Boolean = true,
    ) : SyncExecutionPort {
        override suspend fun capability() = capabilityData

        override suspend fun setSecretsOverride(secrets: SyncSecrets) = overrideOk

        override suspend fun perform(
            config: SyncConfig,
            forceSync: Boolean,
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
                SyncOutcome.Completed(fullSyncSuccess()),
                SyncOutcome.Disabled,
                SyncOutcome.Unconfigured,
                SyncOutcome.Busy,
                SyncOutcome.RetryableFailure(SyncStatus.RecoverableError),
                SyncOutcome.TerminalFailure(SyncStatus.Conflict),
            )
        assertEquals(6, outcomes.distinctBy { it::class }.size)
    }

    @Test
    fun syncingStatus_mapsToTerminalFailure_protocolError() {
        val outcome = SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        assertTrue(outcome is SyncOutcome.TerminalFailure)
        assertEquals(SyncStatus.FatalError, (outcome as SyncOutcome.TerminalFailure).status)
    }

    @Test
    fun bridgeError_typedKinds_classifyCorrectly() {
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
        val unknown =
            BridgeResult.Error(
                com.xiwei.sujian.core.interop.common.ResultEnvelope.errorOf("SOME_FUTURE_CODE", "test"),
            )
        assertEquals(SyncFailureKind.Fatal, SyncFailureKind.fromBridgeError(unknown))
    }

    @Test
    fun legacyErrorCode_mappingStillCoversOldCodes() {
        assertEquals(SyncFailureKind.RetryableNetwork, SyncFailureKind.fromLegacyErrorCode("SYNC_NETWORK_UNAVAILABLE"))
        assertEquals(SyncFailureKind.RetryableIo, SyncFailureKind.fromLegacyErrorCode("IO_ERROR"))
        assertEquals(SyncFailureKind.NativeUnavailable, SyncFailureKind.fromLegacyErrorCode("NATIVE_NOT_LOADED"))
        assertEquals(SyncFailureKind.Authentication, SyncFailureKind.fromLegacyErrorCode("SYNC_AUTH_FAILED"))
        assertEquals(SyncFailureKind.Conflict, SyncFailureKind.fromLegacyErrorCode("SYNC_CONFLICT"))
        assertEquals(SyncFailureKind.Protocol, SyncFailureKind.fromLegacyErrorCode("SYNC_INCOMPLETE_TRANSACTION"))
    }

    @Test
    fun ioException_mapsToRetryableFailure() {
        val outcome = SyncOutcome.RetryableFailure(SyncStatus.RecoverableError)
        assertTrue(outcome is SyncOutcome.RetryableFailure)
        assertEquals(SyncStatus.RecoverableError, (outcome as SyncOutcome.RetryableFailure).status)
    }

    @Test
    fun repositoryException_mapsToTerminalFailure() {
        val outcome = SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        assertTrue(outcome is SyncOutcome.TerminalFailure)
        assertEquals(SyncStatus.FatalError, (outcome as SyncOutcome.TerminalFailure).status)
    }

    @Test
    fun busy_doesNotModifySyncState() {
        val outcome = SyncOutcome.Busy
        assertEquals(SyncOutcome.Busy::class, outcome::class)
    }

    @Test
    fun transactionCommandTypes_areDistinct() {
        val commands: List<SettingsTransactionCommand> =
            listOf(
                SettingsTransactionCommand.SaveAndRunSync(
                    config = SyncConfig(),
                    configRevision = 1L,
                    secrets = SyncSecrets(),
                    secretsRevision = 1L,
                    trigger = SyncTrigger.Manual,
                ),
                SettingsTransactionCommand.SaveAndRunDryRun(
                    config = SyncConfig(),
                    configRevision = 1L,
                    secrets = SyncSecrets(),
                    secretsRevision = 1L,
                ),
                SettingsTransactionCommand.SaveAndRunDiagnostics(
                    config = SyncConfig(),
                    configRevision = 1L,
                    secrets = SyncSecrets(),
                    secretsRevision = 1L,
                ),
            )
        assertEquals(3, commands.distinctBy { it::class }.size)
    }

    private fun assertTerminalFailure(outcome: SyncOutcome) {
        assertTrue("应返回 TerminalFailure，实际: $outcome", outcome is SyncOutcome.TerminalFailure)
    }

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

    private fun enabledSnapshot(): SyncProfileSnapshot =
        SyncProfileSnapshot(
            generation = 1L,
            config = SyncConfig(enabled = true, remoteUrl = "https://unit.example/repo.git"),
            secrets = SyncSecrets(token = "test-token-625"),
        )

    @Test
    fun fullSyncCompletedSignal_carriesDownloadedProjectIdsOnly() {
        val signal = FullSyncCompletedSignal(listOf(PROJECT_ID_625))
        assertEquals(listOf(PROJECT_ID_625), signal.downloadedProjectIds)
        assertEquals(signal, signal.copy(downloadedProjectIds = listOf(PROJECT_ID_625)))
        assertEquals(listOf(PROJECT_ID_625), signal.component1())
    }

    @Test
    fun runFullSync_unconfigured_doesNotEmitFullSyncCompleted() =
        runTest(UnconfinedTestDispatcher()) {
            val coordinator = createCoordinator()
            val collected = mutableListOf<FullSyncCompletedSignal>()
            val collectorJob = launch { coordinator.fullSyncCompleted.collect { collected += it } }

            val outcome =
                coordinator.runFullSync(
                    SyncTrigger.Manual,
                    snapshot =
                        SyncProfileSnapshot(
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
    fun runFullSync_disabled_doesNotEmitFullSyncCompleted() =
        runTest(UnconfinedTestDispatcher()) {
            val coordinator =
                createCoordinator(
                    syncExecution = FakeSyncExecution(capabilityData = SyncCapabilityData(canRun = false)),
                )
            val collected = mutableListOf<FullSyncCompletedSignal>()
            val collectorJob = launch { coordinator.fullSyncCompleted.collect { collected += it } }

            val outcome =
                coordinator.runFullSync(
                    SyncTrigger.Manual,
                    snapshot = enabledSnapshot(),
                )

            collectorJob.cancel()
            assertTrue("应返回 Disabled，实际: $outcome", outcome is SyncOutcome.Disabled)
            assertTrue("Disabled 不应发出信号，实际收到: $collected", collected.isEmpty())
        }

    @Test
    fun runFullSync_terminalFailure_doesNotEmitFullSyncCompleted() =
        runTest(UnconfinedTestDispatcher()) {
            val coordinator =
                createCoordinator(
                    syncExecution =
                        FakeSyncExecution(
                            performResult =
                                BridgeResult.Success(
                                    FullSyncResult(
                                        overallStatus = SyncStatus.FatalError,
                                        targets = emptyList(),
                                        totalUploaded = 0,
                                        totalDownloaded = 0,
                                        totalLocalDeletes = 0,
                                        totalRemoteDeletes = 0,
                                        totalOverwritten = 0,
                                        totalIgnored = 0,
                                        totalConflicts = 0,
                                        error = null,
                                        errorCategory = null,
                                        messageKey = null,
                                    ),
                                ),
                        ),
                )
            val collected = mutableListOf<FullSyncCompletedSignal>()
            val collectorJob = launch { coordinator.fullSyncCompleted.collect { collected += it } }

            val outcome =
                coordinator.runFullSync(
                    SyncTrigger.Manual,
                    snapshot = enabledSnapshot(),
                )

            collectorJob.cancel()
            assertTerminalFailure(outcome)
            assertTrue("TerminalFailure 不应发出信号，实际收到: $collected", collected.isEmpty())
        }

    @Test
    fun runFullSync_completed_emitsFullSyncCompleted() =
        runTest(UnconfinedTestDispatcher()) {
            val projectId = "p-625-completed"
            val coordinator =
                createCoordinator(
                    syncExecution =
                        FakeSyncExecution(
                            capabilityData = SyncCapabilityData(canRun = true),
                            performResult =
                                BridgeResult.Success(
                                    fullSyncSuccess(
                                        targets =
                                            listOf(
                                                TargetSyncResult(
                                                    targetKind = "project",
                                                    projectId = projectId,
                                                    remotePrefix = "projects/$projectId",
                                                    result =
                                                        SyncResult(
                                                            status = SyncStatus.Success,
                                                            downloadedFiles = listOf("chapter1.md"),
                                                        ),
                                                ),
                                            ),
                                    ),
                                ),
                        ),
                )
            val collected = mutableListOf<FullSyncCompletedSignal>()
            val collectorJob = launch { coordinator.fullSyncCompleted.collect { collected += it } }

            val outcome =
                coordinator.runFullSync(
                    SyncTrigger.Manual,
                    snapshot = enabledSnapshot(),
                )

            collectorJob.cancel()
            assertTrue("应返回 Completed，实际: $outcome", outcome is SyncOutcome.Completed)
            assertEquals("Completed 应发出一次信号", 1, collected.size)
            assertEquals(listOf(projectId), collected[0].downloadedProjectIds)
        }

    // ── #630 评论 5308040939 Part 1：预处理失败写同一份 Core FullSyncState ──

    /**
     * 记录式 Repository：override [SyncRepository.recordFullSyncPreflightFailure]，
     * 验证 [SyncCoordinator] 在预处理失败路径上写 Core FullSyncState 的窄接口调用。
     */
    private class RecordingSyncRepository :
        SyncRepository(
            org.robolectric.RuntimeEnvironment.getApplication(),
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    appDataRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-preflight-data",
                    projectsRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-preflight-projects",
                ),
            ),
        ) {
        val recorded: MutableList<Pair<SyncStatus, String>> = mutableListOf()

        override fun recordFullSyncPreflightFailure(
            status: SyncStatus,
            failedTarget: String,
        ) {
            recorded += status to failedTarget
        }
    }

    private class FailingBarrier :
        AppSyncDataBarrier(
            starmapBridge =
                com.xiwei.sujian.feature.starmap.data.interop.StarMapBridge(
                    com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                        appDataRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-preflight-data",
                        projectsRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-preflight-projects",
                    ),
                ),
            reloadSettings = { },
            reloadThemes = { },
            invalidateStarmapCache = { },
        ) {
        override fun flushBeforeSync(): Boolean = false
    }

    /**
     * app data barrier flush 失败 → TerminalFailure，且同时写 Core FullSyncState
     * （FatalError / "preflight"），重启后顶部红灯不被旧 Success 覆盖。
     */
    @Test
    fun preflight_appBarrierFlushFailure_recordsCoreFullSyncState() =
        runTest(UnconfinedTestDispatcher()) {
            val repo = RecordingSyncRepository()
            val syncStatusRepo = SyncStatusRepository(repo)
            val coordinator = SyncCoordinator(repo, syncStatusRepo, FailingBarrier(), FakeSyncExecution())

            val outcome =
                coordinator.runFullSync(
                    SyncTrigger.Manual,
                    snapshot = enabledSnapshot(),
                )

            assertTerminalFailure(outcome)
            assertEquals(
                "app barrier flush 失败必须写同一份 Core FullSyncState",
                listOf(SyncStatus.FatalError to PREFLIGHT_TARGET),
                repo.recorded,
            )
        }

    /**
     * 活动正文 flush 失败 → TerminalFailure，且同时写 Core FullSyncState
     * （FatalError / "preflight"）。
     */
    @Test
    fun preflight_activeDocumentFlushFailure_recordsCoreFullSyncState() =
        runTest(UnconfinedTestDispatcher()) {
            // 替换默认成功 flush 的 gate 为失败 flush
            gateRegistration?.close()
            gateRegistration = ActiveDocumentGate.register(owner = Any(), flush = { false })

            val repo = RecordingSyncRepository()
            val syncStatusRepo = SyncStatusRepository(repo)
            val coordinator = SyncCoordinator(repo, syncStatusRepo, null, FakeSyncExecution())

            val outcome =
                coordinator.runFullSync(
                    SyncTrigger.Manual,
                    snapshot = enabledSnapshot(),
                )

            assertTerminalFailure(outcome)
            assertEquals(
                "文档 flush 失败必须写同一份 Core FullSyncState",
                listOf(SyncStatus.FatalError to PREFLIGHT_TARGET),
                repo.recorded,
            )
        }

    /**
     * secrets override 失败 → TerminalFailure，且同时写 Core FullSyncState
     * （FatalError / "preflight"）。
     */
    @Test
    fun preflight_secretsOverrideFailure_recordsCoreFullSyncState() =
        runTest(UnconfinedTestDispatcher()) {
            val repo = RecordingSyncRepository()
            val syncStatusRepo = SyncStatusRepository(repo)
            val coordinator =
                SyncCoordinator(
                    repo,
                    syncStatusRepo,
                    null,
                    FakeSyncExecution(overrideOk = false),
                )

            val outcome =
                coordinator.runFullSync(
                    SyncTrigger.Manual,
                    snapshot = enabledSnapshot(),
                )

            assertTerminalFailure(outcome)
            assertEquals(
                "credentials override 失败必须写同一份 Core FullSyncState",
                listOf(SyncStatus.FatalError to PREFLIGHT_TARGET),
                repo.recorded,
            )
        }

    /**
     * #630 评论 5308040939 Part 2：聚合保留错误类型优先级 —
     * RecoverableError → RetryableFailure（Worker retry）；Fatal/Dirty/Conflict →
     * TerminalFailure（Worker failure）。mapToOutcome 必须区分这两类。
     */
    @Test
    fun runFullSync_recoverableError_mapsToRetryableFailure() =
        runTest(UnconfinedTestDispatcher()) {
            val coordinator =
                createCoordinator(
                    syncExecution =
                        FakeSyncExecution(
                            performResult =
                                BridgeResult.Success(
                                    FullSyncResult(
                                        overallStatus = SyncStatus.RecoverableError,
                                        targets = emptyList(),
                                        totalUploaded = 0,
                                        totalDownloaded = 0,
                                        totalLocalDeletes = 0,
                                        totalRemoteDeletes = 0,
                                        totalOverwritten = 0,
                                        totalIgnored = 0,
                                        totalConflicts = 0,
                                        error = "temporary network failure",
                                        errorCategory = null,
                                        messageKey = null,
                                    ),
                                ),
                        ),
                )

            val outcome =
                coordinator.runFullSync(
                    SyncTrigger.Manual,
                    snapshot = enabledSnapshot(),
                )

            assertTrue(
                "RecoverableError 应映射为 RetryableFailure（Worker 才能 retry），实际: $outcome",
                outcome is SyncOutcome.RetryableFailure,
            )
        }

    @Test
    fun runFullSync_dirtyRepoBlocked_mapsToTerminalFailure() =
        runTest(UnconfinedTestDispatcher()) {
            val coordinator =
                createCoordinator(
                    syncExecution =
                        FakeSyncExecution(
                            performResult =
                                BridgeResult.Success(
                                    FullSyncResult(
                                        overallStatus = SyncStatus.DirtyRepoBlocked,
                                        targets = emptyList(),
                                        totalUploaded = 0,
                                        totalDownloaded = 0,
                                        totalLocalDeletes = 0,
                                        totalRemoteDeletes = 0,
                                        totalOverwritten = 0,
                                        totalIgnored = 0,
                                        totalConflicts = 0,
                                        error = "remote repo dirty",
                                        errorCategory = null,
                                        messageKey = null,
                                    ),
                                ),
                        ),
                )

            val outcome =
                coordinator.runFullSync(
                    SyncTrigger.Manual,
                    snapshot = enabledSnapshot(),
                )

            assertTrue(
                "DirtyRepoBlocked 应映射为 TerminalFailure（Worker failure），实际: $outcome",
                outcome is SyncOutcome.TerminalFailure,
            )
        }

    @Test
    fun runFullSync_partialConflict_mapsToTerminalFailure() =
        runTest(UnconfinedTestDispatcher()) {
            val coordinator =
                createCoordinator(
                    syncExecution =
                        FakeSyncExecution(
                            performResult =
                                BridgeResult.Success(
                                    FullSyncResult(
                                        overallStatus = SyncStatus.PartialConflict,
                                        targets = emptyList(),
                                        totalUploaded = 0,
                                        totalDownloaded = 0,
                                        totalLocalDeletes = 0,
                                        totalRemoteDeletes = 0,
                                        totalOverwritten = 0,
                                        totalIgnored = 0,
                                        totalConflicts = 0,
                                        error = "conflict detected",
                                        errorCategory = null,
                                        messageKey = null,
                                    ),
                                ),
                        ),
                )

            val outcome =
                coordinator.runFullSync(
                    SyncTrigger.Manual,
                    snapshot = enabledSnapshot(),
                )

            assertTrue(
                "PartialConflict 应映射为 TerminalFailure（Worker failure），实际: $outcome",
                outcome is SyncOutcome.TerminalFailure,
            )
        }
}
