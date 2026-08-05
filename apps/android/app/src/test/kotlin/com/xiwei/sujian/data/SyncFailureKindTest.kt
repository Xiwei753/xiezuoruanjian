package com.xiwei.sujian.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #592 四：SyncFailureKind 契约测试 — 验证统一用户提示映射和异常边界分类。
 */
class SyncFailureKindTest {

    @Test
    fun messageKey_allKindsReturnUniqueNonEmptyStrings() {
        val keys = SyncFailureKind.entries.map { it.messageKey() }
        keys.forEach { key ->
            assertTrue("messageKey should be non-empty: $key", key.isNotEmpty())
        }
        assertEquals("All 8 kinds should have unique messageKeys",
            8, keys.distinct().size)
    }

    @Test
    fun messageKey_noLegacyDryRunOrDiagnosticsError() {
        SyncFailureKind.entries.forEach { kind ->
            val key = kind.messageKey()
            assertNotEquals("Should not use legacy dry_run_error", "dry_run_error", key)
            assertNotEquals("Should not use legacy diagnostics_error", "diagnostics_error", key)
        }
    }

    @Test
    fun fromErrorCode_unknownCodeReturnsFatal() {
        assertEquals(SyncFailureKind.Fatal, SyncFailureKind.fromErrorCode("UNKNOWN_ERROR"))
        assertEquals(SyncFailureKind.Fatal, SyncFailureKind.fromErrorCode(null))
        assertEquals(SyncFailureKind.Fatal, SyncFailureKind.fromErrorCode(""))
    }

    @Test
    fun fatalOutcome_isTerminalNotRetryable() {
        val outcome = SyncFailureKind.Fatal.toOutcome()
        assertTrue("Fatal should be TerminalFailure", outcome is SyncOutcome.TerminalFailure)
    }

    @Test
    fun retryableNetworkOutcome_isRetryable() {
        val outcome = SyncFailureKind.RetryableNetwork.toOutcome()
        assertTrue("RetryableNetwork should be RetryableFailure", outcome is SyncOutcome.RetryableFailure)
    }

    @Test
    fun retryableIoOutcome_isRetryable() {
        val outcome = SyncFailureKind.RetryableIo.toOutcome()
        assertTrue("RetryableIo should be RetryableFailure", outcome is SyncOutcome.RetryableFailure)
    }

    @Test
    fun allTerminalFailures_areNotRetryable() {
        val terminalKinds = listOf(
            SyncFailureKind.Authentication,
            SyncFailureKind.Conflict,
            SyncFailureKind.DirtyRepository,
            SyncFailureKind.Protocol,
            SyncFailureKind.NativeUnavailable,
            SyncFailureKind.Fatal,
        )
        terminalKinds.forEach { kind ->
            val outcome = kind.toOutcome()
            assertTrue("$kind should be TerminalFailure, got $outcome",
                outcome is SyncOutcome.TerminalFailure)
        }
    }
}
