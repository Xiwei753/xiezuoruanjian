package com.xiwei.sujian.diagnostics

import com.xiwei.sujian.model.LocalSettings
import org.junit.Assert.*
import org.junit.Test

class DiagnosticsRedactionTest {

    @Test
    fun redactToken() {
        val input = "token=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"))
    }

    @Test
    fun redactAccessToken() {
        val input = "access_token: my_secret_token_value"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("my_secret_token_value"))
    }

    @Test
    fun redactAuthorization() {
        val input = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.signature"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("eyJhbGciOiJIUzI1NiJ9"))
    }

    @Test
    fun redactPassword() {
        val input = "password=MyS3cretP@ss!"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("MyS3cretP@ss!"))
    }

    @Test
    fun redactSshPrivateKey() {
        val input = "ssh_private_key=-----BEGIN OPENSSH PRIVATE KEY-----\nMIIEvgIBADANBgkq\n-----END OPENSSH PRIVATE KEY-----"
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains("MIIEvgIBADANBgkq"))
        assertTrue(result.contains("[REDACTED]"))
    }

    @Test
    fun redactGhpToken() {
        val input = "Using ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 for auth"
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"))
    }

    @Test
    fun redactGithubPat() {
        val input = "token is github_pat_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij0123456789ABCDEFGHIJ0123456789abcdefghij"
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains("github_pat_"))
    }

    @Test
    fun redactBearerToken() {
        val input = "Bearer abc123def456ghi789=="
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains("abc123def456ghi789"))
    }

    @Test
    fun redactLongContentField() {
        val input = "\"content\": \"This is a very long content string that should be redacted because it looks like user body text that should never appear in logs\""
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
    }

    @Test
    fun redactDoesNotAffectShortContent() {
        val input = "status=ok count=5"
        val result = DiagnosticsLogger.redact(input)
        assertEquals(input, result)
    }

    @Test
    fun redactSecret() {
        val input = "secret=my_oauth_secret"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("my_oauth_secret"))
    }

    @Test
    fun redactPasswd() {
        val input = "passwd=root123"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("root123"))
    }
}

class LocalSettingsDiagnosticsDefaultTest {

    @Test
    fun diagnosticsDefaultsToDisabled() {
        val settings = LocalSettings()
        assertFalse(settings.diagnosticsEnabled)
        assertFalse(settings.diagnosticsVerbose)
    }

    @Test
    fun diagnosticsCanBeEnabled() {
        val settings = LocalSettings(diagnosticsEnabled = true, diagnosticsVerbose = true)
        assertTrue(settings.diagnosticsEnabled)
        assertTrue(settings.diagnosticsVerbose)
    }

    @Test
    fun diagnosticsCopyPreservesFields() {
        val base = LocalSettings(diagnosticsEnabled = true, diagnosticsVerbose = false)
        val copied = base.copy(diagnosticsVerbose = true)
        assertTrue(copied.diagnosticsEnabled)
        assertTrue(copied.diagnosticsVerbose)
    }
}

class EditorEventRingBufferTest {

    @Test
    fun ringBufferDoesNotRecordWhenDisabled() {
        EditorEventRingBuffer.setEnabled(false)
        EditorEventRingBuffer.record(mapOf("event" to "test"))
        assertTrue(EditorEventRingBuffer.getSnapshot().isEmpty())
    }

    @Test
    fun ringBufferRecordsWhenEnabled() {
        EditorEventRingBuffer.setEnabled(true)
        EditorEventRingBuffer.clear()
        EditorEventRingBuffer.record(mapOf("event" to "touch", "x" to 100))
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(1, snapshot.size)
        assertEquals("touch", snapshot[0]["event"])
        assertEquals(100, snapshot[0]["x"])
        EditorEventRingBuffer.setEnabled(false)
    }

    @Test
    fun ringBufferStripsSensitiveKeys() {
        EditorEventRingBuffer.setEnabled(true)
        EditorEventRingBuffer.clear()
        EditorEventRingBuffer.record(mapOf(
            "event" to "text_change",
            "text" to "sensitive user content",
            "content" to "more content",
            "body" to "body text",
            "chapter" to "chapter text",
            "count" to 5
        ))
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(1, snapshot.size)
        assertNull(snapshot[0]["text"])
        assertNull(snapshot[0]["content"])
        assertNull(snapshot[0]["body"])
        assertNull(snapshot[0]["chapter"])
        assertEquals(5, snapshot[0]["count"])
        EditorEventRingBuffer.setEnabled(false)
    }

    @Test
    fun ringBufferRespectsMaxSize() {
        EditorEventRingBuffer.setEnabled(true)
        EditorEventRingBuffer.clear()
        for (i in 1..250) {
            EditorEventRingBuffer.record(mapOf("event" to "test", "i" to i))
        }
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertTrue(snapshot.size <= 200)
        EditorEventRingBuffer.setEnabled(false)
    }

    @Test
    fun ringBufferClearsOnDisable() {
        EditorEventRingBuffer.setEnabled(true)
        EditorEventRingBuffer.record(mapOf("event" to "test"))
        EditorEventRingBuffer.setEnabled(false)
        assertTrue(EditorEventRingBuffer.getSnapshot().isEmpty())
    }
}
