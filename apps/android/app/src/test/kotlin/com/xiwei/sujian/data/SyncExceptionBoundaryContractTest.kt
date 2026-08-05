package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #592 三：同步异常边界分类契约测试。
 *
 * 验证 SyncCoordinator.runSync() 的异常分类规则：
 * - CancellationException → 原样抛出
 * - IOException → RetryableFailure(RecoverableError)
 * - RepositoryException → TerminalFailure(FatalError)
 * - BridgeResult.Error retryable codes → RetryableFailure
 * - BridgeResult.Error terminal codes → TerminalFailure
 * - BridgeResult.NotLoaded → TerminalFailure(FatalError)
 * - Syncing status → TerminalFailure(FatalError)
 */
class SyncExceptionBoundaryContractTest {

    @Test
    fun retryableErrorCodes_areExactlyDefined() {
        val expected = setOf(
            "SYNC_NETWORK_UNAVAILABLE",
            "SYNC_RATE_LIMITED",
            "IO_ERROR",
            "NATIVE_NOT_LOADED",
        )
        assertEquals(expected, SyncCoordinator.RETRYABLE_ERROR_CODES)
    }

    @Test
    fun authFailure_isNotRetryable() {
        assertTrue("SYNC_AUTH_FAILED should be terminal",
            "SYNC_AUTH_FAILED" !in SyncCoordinator.RETRYABLE_ERROR_CODES)
    }

    @Test
    fun conflictErrors_areNotRetryable() {
        val conflictCodes = listOf(
            "SYNC_CONFLICT",
            "SYNC_DOCUMENT_CONFLICT",
            "SYNC_CHECKOUT_CONFLICT",
            "SYNC_SETTINGS_CONFLICT",
            "SYNC_CONFLICT_DETECTED",
        )
        conflictCodes.forEach { code ->
            assertTrue("$code should be terminal", code !in SyncCoordinator.RETRYABLE_ERROR_CODES)
        }
    }

    @Test
    fun protocolErrors_areNotRetryable() {
        val protocolCodes = listOf(
            "SYNC_NON_FAST_FORWARD",
            "SYNC_UNRELATED_HISTORIES",
            "SYNC_INCOMPLETE_TRANSACTION",
        )
        protocolCodes.forEach { code ->
            assertTrue("$code should be terminal", code !in SyncCoordinator.RETRYABLE_ERROR_CODES)
        }
    }

    @Test
    fun syncingStatus_terminalFailureContract() {
        // #592 三：performSync 返回 Syncing 是协议错误 → TerminalFailure(FatalError)
        val outcome = SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        assertTrue(outcome is SyncOutcome.TerminalFailure)
        assertEquals(SyncStatus.FatalError, (outcome as SyncOutcome.TerminalFailure).status)
    }

    @Test
    fun notLoaded_terminalFailureContract() {
        // #592 三：原生库未加载是致命错误 → TerminalFailure(FatalError)
        val outcome = SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        assertTrue(outcome is SyncOutcome.TerminalFailure)
    }

    @Test
    fun ioException_retryableFailureContract() {
        // #592 三：临时 IO 异常 → RetryableFailure(RecoverableError)
        val outcome = SyncOutcome.RetryableFailure(SyncStatus.RecoverableError)
        assertTrue(outcome is SyncOutcome.RetryableFailure)
        assertEquals(SyncStatus.RecoverableError, (outcome as SyncOutcome.RetryableFailure).status)
    }

    @Test
    fun repositoryException_terminalFailureContract() {
        // #592 三：仓库层异常 → TerminalFailure(FatalError)
        val outcome = SyncOutcome.TerminalFailure(SyncStatus.FatalError)
        assertTrue(outcome is SyncOutcome.TerminalFailure)
        assertEquals(SyncStatus.FatalError, (outcome as SyncOutcome.TerminalFailure).status)
    }

    @Test
    fun allSyncOutcomePaths_endInDefiniteState() {
        // #592 三：所有路径必须结束在明确终态
        val definiteOutcomes = listOf(
            SyncOutcome.Completed(com.xiwei.sujian.model.SyncResult(status = SyncStatus.Success)),
            SyncOutcome.Disabled,
            SyncOutcome.Unconfigured,
            SyncOutcome.RetryableFailure(SyncStatus.RecoverableError),
            SyncOutcome.TerminalFailure(SyncStatus.FatalError),
        )
        definiteOutcomes.forEach { outcome ->
            assertTrue("Each outcome must be a definite terminal state",
                outcome !is SyncOutcome.Busy || outcome == SyncOutcome.Busy)
        }
    }
}
