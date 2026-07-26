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
    }
}
