package com.xiwei.sujian.data

import org.junit.Assert.assertEquals
import com.xiwei.sujian.data.SyncOutcome
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
        assertEquals("All kinds should have unique messageKeys (incl. #595 三 document failures)",
            SyncFailureKind.entries.size, keys.distinct().size)
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
    fun fromLegacyErrorCode_unknownCodeReturnsFatal() {
        assertEquals(SyncFailureKind.Fatal, SyncFailureKind.fromLegacyErrorCode("UNKNOWN_ERROR"))
        assertEquals(SyncFailureKind.Fatal, SyncFailureKind.fromLegacyErrorCode(null))
        assertEquals(SyncFailureKind.Fatal, SyncFailureKind.fromLegacyErrorCode(""))
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

    /**
     * #592 三：toOutcome 必须保留具体 kind，使正式同步、试运行、连接诊断
     * 全部通过 kind.messageKey() 获得同一用户提示映射。
     */
    @Test
    fun toOutcome_preservesKindForAllKinds() {
        SyncFailureKind.entries.forEach { kind ->
            val outcome = kind.toOutcome()
            val recoveredKind = when (outcome) {
                is SyncOutcome.RetryableFailure -> outcome.kind
                is SyncOutcome.TerminalFailure -> outcome.kind
                else -> null
            }
            assertEquals("toOutcome must preserve kind for $kind", kind, recoveredKind)
        }
    }

    @Test
    fun fromSyncStatus_mapsCoreStatusesToCorrectKinds() {
        assertEquals(SyncFailureKind.RetryableNetwork,
            SyncFailureKind.fromSyncStatus(com.xiwei.sujian.model.SyncStatus.RecoverableError))
        assertEquals(SyncFailureKind.Fatal,
            SyncFailureKind.fromSyncStatus(com.xiwei.sujian.model.SyncStatus.Error))
        assertEquals(SyncFailureKind.Conflict,
            SyncFailureKind.fromSyncStatus(com.xiwei.sujian.model.SyncStatus.Conflict))
        assertEquals(SyncFailureKind.Conflict,
            SyncFailureKind.fromSyncStatus(com.xiwei.sujian.model.SyncStatus.PartialConflict))
        assertEquals(SyncFailureKind.DirtyRepository,
            SyncFailureKind.fromSyncStatus(com.xiwei.sujian.model.SyncStatus.DirtyRepoBlocked))
        assertEquals(SyncFailureKind.Fatal,
            SyncFailureKind.fromSyncStatus(com.xiwei.sujian.model.SyncStatus.FatalError))
    }

    @Test
    fun isTransientReadFailure_onlyNetworkIoNativeAreRetryable() {
        // #595 四：快照读取失败对 WorkManager 的映射 — 只有临时网络/IO/原生库
        // 故障交给退避重试；Fatal/协议/凭据/冲突类按确定性失败处理。
        assertTrue(SyncFailureKind.RetryableNetwork.isTransientReadFailure())
        assertTrue(SyncFailureKind.RetryableIo.isTransientReadFailure())
        assertTrue(SyncFailureKind.NativeUnavailable.isTransientReadFailure())
        for (kind in listOf(
            SyncFailureKind.Authentication,
            SyncFailureKind.Conflict,
            SyncFailureKind.DirtyRepository,
            SyncFailureKind.Protocol,
            SyncFailureKind.Fatal,
        )) {
            assertTrue("$kind 不是临时读取故障，不得交给 WorkManager 重试",
                !kind.isTransientReadFailure())
        }
    }
}
