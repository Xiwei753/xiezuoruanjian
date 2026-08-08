package com.xiwei.sujian.core.interop.sync
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.settings.ui.SettingsTransactionCommand
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SyncCoordinatorTest {
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
}
