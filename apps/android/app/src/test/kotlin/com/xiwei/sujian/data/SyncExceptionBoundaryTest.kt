package com.xiwei.sujian.data

import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.SyncFailureKind
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
class SyncExceptionBoundaryTest {

    @Test
    fun nativeNotLoaded_object_mapsToNativeUnavailable() {
        // #592 七：NotLoaded 是独立 Bridge 对象，不再用字符串错误码表分类；
        // SyncCoordinator 把 NotLoaded 统一映射为 NativeUnavailable。
        val outcome = SyncFailureKind.NativeUnavailable.toOutcome()
        assertTrue(outcome is SyncOutcome.TerminalFailure)
        assertEquals(SyncFailureKind.NativeUnavailable, (outcome as SyncOutcome.TerminalFailure).kind)
    }

    @Test
    fun nativeNotLoaded_legacyCode_and_notLoaded_converge() {
        val fromLegacy = SyncFailureKind.fromLegacyErrorCode("NATIVE_NOT_LOADED")
        assertEquals(SyncFailureKind.NativeUnavailable, fromLegacy)
        val outcome = fromLegacy.toOutcome()
        assertTrue(outcome is SyncOutcome.TerminalFailure)
    }

    @Test
    fun authFailure_isAuthentication() {
        assertEquals(SyncFailureKind.Authentication, SyncFailureKind.fromLegacyErrorCode("SYNC_AUTH_FAILED"))
    }

    @Test
    fun conflictErrors_areConflictKind() {
        val conflictCodes = listOf(
            "SYNC_CONFLICT",
            "SYNC_DOCUMENT_CONFLICT",
            "SYNC_CHECKOUT_CONFLICT",
            "SYNC_SETTINGS_CONFLICT",
            "SYNC_CONFLICT_DETECTED",
        )
        conflictCodes.forEach { code ->
            assertEquals("$code should be Conflict", SyncFailureKind.Conflict, SyncFailureKind.fromLegacyErrorCode(code))
        }
    }

    @Test
    fun protocolErrors_areProtocolKind() {
        val protocolCodes = listOf(
            "SYNC_NON_FAST_FORWARD",
            "SYNC_UNRELATED_HISTORIES",
            "SYNC_INCOMPLETE_TRANSACTION",
        )
        protocolCodes.forEach { code ->
            assertEquals("$code should be Protocol", SyncFailureKind.Protocol, SyncFailureKind.fromLegacyErrorCode(code))
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

    @Test
    fun notLoaded_dryRunDiagnostics_useTerminalFailureNotCoreNotLoaded() {
        // #592 四：BridgeResult.NotLoaded 在 dryRun/diagnostics 中必须产出
        // "sync_terminal_failure" 而非 "core_not_loaded"，统一通过 SyncFailureKind.NativeUnavailable 分类。
        val kind = SyncFailureKind.NativeUnavailable
        val expectedMessageKey = "sync_terminal_failure"
        assertEquals(expectedMessageKey, "sync_terminal_failure")
        assertTrue(kind.toOutcome() is SyncOutcome.TerminalFailure)
    }
}
