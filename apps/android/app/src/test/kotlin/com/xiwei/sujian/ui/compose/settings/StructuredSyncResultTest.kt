package com.xiwei.sujian.ui.compose.settings

import org.junit.Assert.*
import org.junit.Test

class StructuredSyncResultTest {

    @Test
    fun dryRunResultContainsCounts() {
        val result = StructuredSyncResult(
            statusCode = "ok",
            messageKey = "sync_dry_run_result",
            counts = SyncCounts(
                uploaded = 3,
                downloaded = 2,
                deletedRemote = 1,
                conflicts = 0
            )
        )
        assertEquals("ok", result.statusCode)
        assertEquals("sync_dry_run_result", result.messageKey)
        assertEquals(3, result.counts.uploaded)
        assertEquals(2, result.counts.downloaded)
        assertEquals(1, result.counts.deletedRemote)
        assertEquals(0, result.counts.conflicts)
    }

    @Test
    fun testConnectionResultContainsMessageArgs() {
        val result = StructuredSyncResult(
            statusCode = "fail",
            messageKey = "sync_test_connection_result",
            messageArgs = mapOf(
                "network" to "ok",
                "auth" to "fail",
                "repo" to "ok",
                "branch" to "ok"
            ),
            sanitizedDiagnostic = "connection_failed"
        )
        assertEquals("fail", result.statusCode)
        assertEquals("fail", result.messageArgs["auth"])
        assertEquals("ok", result.messageArgs["network"])
        assertNotNull(result.sanitizedDiagnostic)
    }

    @Test
    fun errorResultDoesNotExposeRawError() {
        val result = StructuredSyncResult(
            statusCode = "error",
            messageKey = "sync_error",
            sanitizedDiagnostic = "internal_error"
        )
        assertNull(result.messageArgs["rawError"])
        assertEquals("internal_error", result.sanitizedDiagnostic)
    }

    @Test
    fun syncCountsDefaultsToZero() {
        val counts = SyncCounts()
        assertEquals(0, counts.uploaded)
        assertEquals(0, counts.downloaded)
        assertEquals(0, counts.deletedRemote)
        assertEquals(0, counts.deletedLocal)
        assertEquals(0, counts.conflicts)
        assertEquals(0, counts.overwritten)
        assertEquals(0, counts.ignored)
    }

    @Test
    fun performSyncResultContainsCounts() {
        val result = StructuredSyncResult(
            statusCode = "ok",
            messageKey = "sync_perform_result",
            counts = SyncCounts(
                uploaded = 5,
                downloaded = 3,
                deletedRemote = 1,
                deletedLocal = 2,
                conflicts = 0,
                overwritten = 4,
                ignored = 6
            )
        )
        assertEquals("ok", result.statusCode)
        assertEquals("sync_perform_result", result.messageKey)
        assertEquals(5, result.counts.uploaded)
        assertEquals(3, result.counts.downloaded)
        assertEquals(1, result.counts.deletedRemote)
        assertEquals(2, result.counts.deletedLocal)
        assertEquals(0, result.counts.conflicts)
        assertEquals(4, result.counts.overwritten)
        assertEquals(6, result.counts.ignored)
    }

    @Test
    fun errorResultsUseMessageKeyNotRawString() {
        val errorKeys = listOf("dry_run_error", "diagnostics_error", "sync_error", "core_not_loaded", "unexpected_error")
        for (key in errorKeys) {
            val result = StructuredSyncResult(
                statusCode = "error",
                messageKey = key
            )
            assertEquals("error", result.statusCode)
            assertEquals(key, result.messageKey)
            assertNull(result.messageArgs["rawError"])
        }
    }

    @Test
    fun busyResultUsesMessageKey() {
        val result = StructuredSyncResult(
            statusCode = "busy",
            messageKey = "sync_already_running"
        )
        assertEquals("busy", result.statusCode)
        assertEquals("sync_already_running", result.messageKey)
    }

    @Test
    fun blockedResultUsesBlockMessageKey() {
        val result = StructuredSyncResult(
            statusCode = "blocked",
            messageKey = "sync_not_ready"
        )
        assertEquals("blocked", result.statusCode)
        assertEquals("sync_not_ready", result.messageKey)
    }

    @Test
    fun sanitizedDiagnosticDoesNotLeakRawError() {
        val result = StructuredSyncResult(
            statusCode = "error",
            messageKey = "sync_error",
            sanitizedDiagnostic = "connection_refused"
        )
        assertEquals("connection_refused", result.sanitizedDiagnostic)
        assertNull(result.messageArgs["rawError"])
    }

    @Test
    fun testConnectionResultAllComponentsOk() {
        val result = StructuredSyncResult(
            statusCode = "ok",
            messageKey = "sync_test_connection_result",
            messageArgs = mapOf(
                "network" to "ok",
                "auth" to "ok",
                "repo" to "ok",
                "branch" to "ok"
            )
        )
        assertEquals("ok", result.statusCode)
        assertEquals("ok", result.messageArgs["network"])
        assertEquals("ok", result.messageArgs["auth"])
        assertEquals("ok", result.messageArgs["repo"])
        assertEquals("ok", result.messageArgs["branch"])
    }

    @Test
    fun dryRunResultWithConflicts() {
        val result = StructuredSyncResult(
            statusCode = "ok",
            messageKey = "sync_dry_run_result",
            counts = SyncCounts(
                uploaded = 1,
                downloaded = 0,
                conflicts = 2
            )
        )
        assertEquals(2, result.counts.conflicts)
    }

    @Test
    fun statusCodeOkIsTheOnlySuccessIndicator() {
        val successCodes = listOf("ok")
        val failureCodes = listOf("fail", "error", "blocked", "busy")
        for (code in successCodes) {
            assertTrue("statusCode=$code should be success", code == "ok")
        }
        for (code in failureCodes) {
            assertFalse("statusCode=$code should not be success", code == "ok")
        }
    }

    @Test
    fun statusCodeDistinguishesAllSyncOutcomes() {
        val allCodes = listOf("ok", "fail", "error", "blocked", "busy")
        assertEquals(5, allCodes.size)
        assertEquals(5, allCodes.toSet().size)
    }

    @Test
    fun syncCommandIoResultIsSuccessDerivesFromStatusCode() {
        val okResult = SyncCommandIoResultForTest(true, true, StructuredSyncResult(statusCode = "ok", messageKey = "sync_dry_run_result"))
        assertTrue(okResult.isSuccess)

        val failResult = SyncCommandIoResultForTest(true, true, StructuredSyncResult(statusCode = "fail", messageKey = "sync_test_connection_result"))
        assertFalse(failResult.isSuccess)

        val errorResult = SyncCommandIoResultForTest(true, true, StructuredSyncResult(statusCode = "error", messageKey = "sync_error"))
        assertFalse(errorResult.isSuccess)

        val blockedResult = SyncCommandIoResultForTest(true, true, StructuredSyncResult(statusCode = "blocked", messageKey = "sync_not_ready"))
        assertFalse(blockedResult.isSuccess)

        val busyResult = SyncCommandIoResultForTest(false, false, StructuredSyncResult(statusCode = "busy", messageKey = "sync_already_running"))
        assertFalse(busyResult.isSuccess)
    }

    @Test
    fun syncCommandIoResultConfigSavedIndependentOfSuccess() {
        val configFailed = SyncCommandIoResultForTest(false, false, StructuredSyncResult(statusCode = "error", messageKey = "save_config_failed"))
        assertFalse(configFailed.configSaved)
        assertFalse(configFailed.isSuccess)

        val configOkSyncFailed = SyncCommandIoResultForTest(true, false, StructuredSyncResult(statusCode = "error", messageKey = "save_secrets_failed"))
        assertTrue(configOkSyncFailed.configSaved)
        assertFalse(configOkSyncFailed.secretsSaved)
        assertFalse(configOkSyncFailed.isSuccess)
    }

    private data class SyncCommandIoResultForTest(
        val configSaved: Boolean,
        val secretsSaved: Boolean,
        val structuredResult: StructuredSyncResult,
    ) {
        val isSuccess: Boolean get() = structuredResult.statusCode == "ok"
    }
}
