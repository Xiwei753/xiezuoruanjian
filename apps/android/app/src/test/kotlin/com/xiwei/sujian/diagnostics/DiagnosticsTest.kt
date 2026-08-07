package com.xiwei.sujian.diagnostics

import com.xiwei.sujian.model.LocalSettings
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class DiagnosticsRedactionTest {
    @Test
    fun redactTokenEquals() {
        val input = "token=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc.def"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("token="))
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"))
    }

    @Test
    fun redactAccessTokenColon() {
        val input = "access_token: eyJhbGciOiJSUzI1NiJ9.payload.sig"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("access_token:"))
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("eyJhbGciOiJSUzI1NiJ9"))
    }

    @Test
    fun redactRefreshTokenEquals() {
        val input = "refresh_token=dGhpcyBpcyBhIHJlZnJlc2g="
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("dGhpcyBpcyBhIHJlZnJlc2g"))
    }

    @Test
    fun redactAuthorizationBearerHeader() {
        val input = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.signature"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("Authorization"))
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("eyJhbGciOiJIUzI1NiJ9"))
        assertFalse(result.contains("payload.signature"))
    }

    @Test
    fun redactAuthorizationEqualsBearer() {
        val input = "authorization=Bearer abc123def456"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("authorization"))
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("abc123def456"))
    }

    @Test
    fun redactJsonAuthorizationBearer() {
        val input = "{\"authorization\":\"Bearer abc123def456\"}"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("abc123def456"))
    }

    @Test
    fun redactBearerInline() {
        val input = "Bearer abc123def456ghi789=="
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("Bearer"))
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("abc123def456ghi789"))
    }

    @Test
    fun redactPasswordEquals() {
        val input = "password=MyS3cretP@ss!"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("password="))
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("MyS3cretP@ss!"))
    }

    @Test
    fun redactPasswdColon() {
        val input = "passwd: root123"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("root123"))
    }

    @Test
    fun redactSecretEquals() {
        val input = "secret=my_oauth_client_secret_value"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("my_oauth_client_secret_value"))
    }

    @Test
    fun redactPrivateKeyEquals() {
        val input = "private_key=MIIEvgIBADANBgkqhkiG9w0BAQEFAAS"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("MIIEvgIBADANBgkq"))
    }

    @Test
    fun redactSshPrivateKeyWithPemBlock() {
        val input =
            "ssh_private_key=-----BEGIN OPENSSH PRIVATE KEY-----\n" +
                "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCAmEwggJdAgEAAoGBANz\n" +
                "-----END OPENSSH PRIVATE KEY-----"
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains("MIIEvgIBADANBgkq"))
        assertFalse(result.contains("-----BEGIN"))
        assertFalse(result.contains("-----END"))
        assertTrue(result.contains("[REDACTED]"))
    }

    @Test
    fun redactPemPrivateKeyBlock() {
        val input =
            "-----BEGIN RSA PRIVATE KEY-----\n" +
                "MIIEpAIBAAKCAQEA0Z3VS5JJcds3xfn/ygWyF8PbnGy0AHB7\n" +
                "-----END RSA PRIVATE KEY-----"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED_PEM]"))
        assertFalse(result.contains("MIIEpAIBAAKCAQEA"))
        assertFalse(result.contains("-----BEGIN RSA PRIVATE KEY-----"))
    }

    @Test
    fun redactGhpToken() {
        val token = "ghp_" + "A".repeat(36)
        val input = "Using $token for auth"
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains(token))
        assertTrue(result.contains("[REDACTED]"))
    }

    @Test
    fun redactGhoToken() {
        val token = "gho_" + "B".repeat(36)
        val input = "User $token logged in"
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains(token))
        assertTrue(result.contains("[REDACTED]"))
    }

    @Test
    fun redactGithubPat() {
        val token = "github_pat_" + "A".repeat(82)
        val input = "token is $token"
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains(token))
        assertTrue(result.contains("[REDACTED]"))
    }

    @Test
    fun redactJsonContentField() {
        val input = "\"content\": \"This is user body text that should be redacted\""
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("This is user body text"))
    }

    @Test
    fun redactJsonTextField() {
        val input = "\"text\": \"The quick brown fox jumps over the lazy dog\""
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("quick brown fox"))
    }

    @Test
    fun redactJsonBodyField() {
        val input = "\"body\": \"Some body content here\""
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("Some body content"))
    }

    @Test
    fun redactJsonChapterField() {
        val input = "\"chapter\": \"Chapter one content with story text\""
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("Chapter one content"))
    }

    @Test
    fun redactJsonChapterContentField() {
        val input = "\"chapterContent\": \"The protagonist walked through the door\""
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("protagonist"))
    }

    @Test
    fun redactJsonTokenField() {
        val input = "\"token\": \"eyJhbGciOiJIUzI1NiJ9.abc.def\""
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("eyJhbGciOiJIUzI1NiJ9"))
    }

    @Test
    fun redactJsonPasswordField() {
        val input = "\"password\": \"SuperSecret123!\""
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("SuperSecret123"))
    }

    @Test
    fun redactJsonAuthorizationField() {
        val input = "\"authorization\": \"Bearer abc123def456\""
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("abc123def456"))
        assertFalse(result.contains("Bearer abc123def456"))
    }

    @Test
    fun redactKvContentField() {
        val input = "content=This is the user's writing content"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("user's writing"))
    }

    @Test
    fun redactKvTextField() {
        val input = "text=Hello world paragraph"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("Hello world"))
    }

    @Test
    fun redactKvBodyField() {
        val input = "body=Request body content"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("Request body"))
    }

    @Test
    fun redactDoesNotAffectTextLength() {
        val input = "textLength=1234 count=5"
        val result = DiagnosticsLogger.redact(input)
        assertEquals(input, result)
    }

    @Test
    fun redactDoesNotAffectShortNonSensitive() {
        val input = "status=ok count=5 mode=dark"
        val result = DiagnosticsLogger.redact(input)
        assertEquals(input, result)
    }

    @Test
    fun redactDoesNotAffectMetadataFields() {
        val input = "fontSize=16 lineSpacing=1.5 autoSave=true"
        val result = DiagnosticsLogger.redact(input)
        assertEquals(input, result)
    }

    @Test
    fun redactThrowableMessage() {
        val input =
            "java.lang.RuntimeException: Failed to save content=The user wrote a long novel here\n" +
                "\tat com.example.SaveService.save(SaveService.kt:42)"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("java.lang.RuntimeException"))
        assertTrue(result.contains("SaveService.kt:42"))
        assertFalse(result.contains("long novel"))
    }

    @Test
    fun redactStackTracWithPassword() {
        val input = "java.io.IOException: Auth failed password=SecretPass123\n\tat com.example.Auth.login(Auth.kt:10)"
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains("SecretPass123"))
        assertTrue(result.contains("Auth.kt:10"))
    }

    @Test
    fun redactMultiplePatternsInOneMessage() {
        val input = "token=abc123 password=secret \"content\": \"user text\""
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains("abc123"))
        assertFalse(result.contains("secret"))
        assertFalse(result.contains("user text"))
        assertTrue(result.count { it == '[' } >= 3)
    }

    @Test
    fun redactCaseInsensitive() {
        val input = "TOKEN=abc123 Password=secret SECRET=val"
        val result = DiagnosticsLogger.redact(input)
        assertFalse(result.contains("abc123"))
        assertFalse(result.contains("secret"))
        assertFalse(result.contains("val"))
    }

    @Test
    fun redactShortContentStillRedacted() {
        val input = "content=Hi"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("Hi"))
    }

    @Test
    fun redactShortTextStillRedacted() {
        val input = "\"text\": \"Hi\""
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
    }

    @Test
    fun redactChapterContentKv() {
        val input = "chapter_content=Chapter body text here"
        val result = DiagnosticsLogger.redact(input)
        assertTrue(result.contains("[REDACTED]"))
        assertFalse(result.contains("Chapter body"))
    }
}

class LocalSettingsDiagnosticsDefaultTest {
    @Test
    fun diagnosticsDefaultsToEnabled() {
        val settings = LocalSettings()
        assertTrue(settings.diagnosticsEnabled)
        assertTrue(settings.diagnosticsVerbose)
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
        EditorEventRingBuffer.record(
            mapOf(
                "event" to "text_change",
                "text" to "sensitive user content",
                "content" to "more content",
                "body" to "body text",
                "chapter" to "chapter text",
                "chapter_content" to "chapter content",
                "chapterContent" to "chapter content camelCase",
                "password" to "pass123",
                "token" to "tok123",
                "access_token" to "at123",
                "refresh_token" to "rt123",
                "authorization" to "auth123",
                "secret" to "sec123",
                "private_key" to "pk123",
                "ssh_private_key" to "sshpvk123",
                "count" to 5,
            ),
        )
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(1, snapshot.size)
        assertNull(snapshot[0]["text"])
        assertNull(snapshot[0]["content"])
        assertNull(snapshot[0]["body"])
        assertNull(snapshot[0]["chapter"])
        assertNull(snapshot[0]["chapter_content"])
        assertNull(snapshot[0]["chapterContent"])
        assertNull(snapshot[0]["password"])
        assertNull(snapshot[0]["token"])
        assertNull(snapshot[0]["access_token"])
        assertNull(snapshot[0]["refresh_token"])
        assertNull(snapshot[0]["authorization"])
        assertNull(snapshot[0]["secret"])
        assertNull(snapshot[0]["private_key"])
        assertNull(snapshot[0]["ssh_private_key"])
        assertEquals(5, snapshot[0]["count"])
        EditorEventRingBuffer.setEnabled(false)
    }

    @Test
    fun ringBufferRespectsMaxSize() {
        EditorEventRingBuffer.setEnabled(true)
        EditorEventRingBuffer.clear()
        for (i in 1..1250) {
            EditorEventRingBuffer.record(mapOf("event" to "test", "i" to i))
        }
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertTrue(snapshot.size <= 1000)
        assertEquals(1000, snapshot.size)
        // 环形缓冲保留最新事件。
        assertEquals(1250, snapshot.last()["i"])
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
