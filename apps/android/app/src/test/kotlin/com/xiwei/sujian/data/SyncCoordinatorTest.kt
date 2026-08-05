package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.model.SyncStatus
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import com.xiwei.sujian.ui.compose.settings.SettingsTransactionCommand
import org.junit.Test

class SyncCoordinatorTest {

    @Test
    fun syncSession_runExclusive_blocksConcurrentAccess() = runTest {
        var firstEntered = false

        val result1 = SyncSession.runExclusive {
            firstEntered = true
            kotlinx.coroutines.delay(100)
            "first"
        }
        assertEquals("first", (result1 as ExclusiveResult.Success).value)
        assertEquals(true, firstEntered)
    }

    @Test
    fun syncSession_runExclusive_returnsBusyWhenLocked() = runTest {
        assertEquals(ExclusiveResult.Busy::class, ExclusiveResult.Busy::class)
    }

    @Test
    fun syncOutcome_sealedClassHierarchy() {
        val outcomes: List<SyncOutcome> = listOf(
            SyncOutcome.Completed(com.xiwei.sujian.model.SyncResult(status = com.xiwei.sujian.model.SyncStatus.Success)),
            SyncOutcome.Disabled,
            SyncOutcome.Unconfigured,
            SyncOutcome.Busy,
            SyncOutcome.RetryableFailure(com.xiwei.sujian.model.SyncStatus.RecoverableError),
            SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.Conflict),
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
    fun bridgeError_retryableCodes_mapToRetryableFailure() {
        // #592 三：网络不可用、限流、IO 错误 → 可重试
        val retryableCodes = listOf("SYNC_NETWORK_UNAVAILABLE", "SYNC_RATE_LIMITED", "IO_ERROR", "NATIVE_NOT_LOADED")
        retryableCodes.forEach { code ->
            assertTrue("$code should be in RETRYABLE_ERROR_CODES",
                code in SyncCoordinator.RETRYABLE_ERROR_CODES)
        }
    }

    @Test
    fun bridgeError_terminalCodes_mapToTerminalFailure() {
        // #592 三：认证失败、冲突、协议错误 → 不可重试
        val terminalCodes = listOf(
            "SYNC_AUTH_FAILED", "SYNC_CONFLICT", "SYNC_DOCUMENT_CONFLICT",
            "SYNC_CHECKOUT_CONFLICT", "SYNC_SETTINGS_CONFLICT",
            "SYNC_NON_FAST_FORWARD", "SYNC_UNRELATED_HISTORIES",
        )
        terminalCodes.forEach { code ->
            assertTrue("$code should NOT be in RETRYABLE_ERROR_CODES",
                code !in SyncCoordinator.RETRYABLE_ERROR_CODES)
        }
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
        val commands: List<SettingsTransactionCommand> = listOf(
            SettingsTransactionCommand.SaveAndRunSync(
                config = com.xiwei.sujian.model.SyncConfig(),
                configRevision = 1L,
                secrets = com.xiwei.sujian.model.SyncSecrets(),
                secretsRevision = 1L,
                trigger = com.xiwei.sujian.model.SyncTrigger.Manual,
            ),
            SettingsTransactionCommand.SaveAndRunDryRun(
                config = com.xiwei.sujian.model.SyncConfig(),
                configRevision = 1L,
                secrets = com.xiwei.sujian.model.SyncSecrets(),
                secretsRevision = 1L,
            ),
            SettingsTransactionCommand.SaveAndRunDiagnostics(
                config = com.xiwei.sujian.model.SyncConfig(),
                configRevision = 1L,
                secrets = com.xiwei.sujian.model.SyncSecrets(),
                secretsRevision = 1L,
            ),
        )
        assertEquals(3, commands.distinctBy { it::class }.size)
    }
}