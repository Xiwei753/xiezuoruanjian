@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.core.interop.diagnostics

import com.xiwei.sujian.BuildConfig
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #623 评论 3 / #670 评论 5651060802：DiagnosticsInterop.BuildIdentity 构建身份
 * 与日志文件名分界契约测试。
 *
 * 旧的 `core.diagnostics.DiagnosticsBuildIdentity` 已删除，构建身份现在由
 * [DiagnosticsInterop.BuildIdentity] 提供，供 crash handler 和导出 manifest 使用。
 */
class DiagnosticsBuildIdentityTest {
    @Test
    fun fromBuildConfig_populatesAllFieldsFromBuildConfig() {
        val identity = DiagnosticsInterop.buildIdentity()
        assertEquals(BuildConfig.VERSION_NAME, identity.versionName)
        assertEquals(BuildConfig.VERSION_CODE, identity.versionCode)
        assertEquals(BuildConfig.GIT_COMMIT_SHA, identity.gitCommitSha)
        assertEquals(BuildConfig.FLAVOR, identity.flavor)
        assertEquals(BuildConfig.BUILD_TYPE, identity.buildType)
        assertEquals(BuildConfig.APPLICATION_ID, identity.applicationId)
    }

    @Test
    fun buildKey_hasExpectedFormat() {
        val identity = DiagnosticsInterop.buildIdentity()
        val expected = "v${identity.versionCode}-${identity.gitCommitSha}-${identity.flavor}-${identity.buildType}"
        assertEquals(expected, identity.buildKey)
    }

    @Test
    fun buildKey_startsWithVersionCodePrefix() {
        val identity = DiagnosticsInterop.buildIdentity()
        assertTrue(
            "buildKey should start with 'v' + versionCode, got ${identity.buildKey}",
            identity.buildKey.startsWith("v${identity.versionCode}-"),
        )
    }

    @Test
    fun buildKey_containsAllIdentityComponents() {
        val identity = DiagnosticsInterop.buildIdentity()
        assertTrue("buildKey should contain gitCommitSha", identity.buildKey.contains(identity.gitCommitSha))
        assertTrue("buildKey should contain flavor", identity.buildKey.contains(identity.flavor))
        assertTrue("buildKey should contain buildType", identity.buildKey.contains(identity.buildType))
    }

    @Test
    fun differentIdentities_produceDifferentBuildKeys() {
        val a = DiagnosticsInterop.BuildIdentity("1.0", 1, "abc1234", "noAi", "debug", "com.example")
        val b = DiagnosticsInterop.BuildIdentity("1.0", 2, "abc1234", "noAi", "debug", "com.example")
        assertNotEquals("Different versionCode should produce different buildKey", a.buildKey, b.buildKey)

        val c = DiagnosticsInterop.BuildIdentity("1.0", 1, "def5678", "noAi", "debug", "com.example")
        assertNotEquals("Different gitCommitSha should produce different buildKey", a.buildKey, c.buildKey)

        val d = DiagnosticsInterop.BuildIdentity("1.0", 1, "abc1234", "ai", "debug", "com.example")
        assertNotEquals("Different flavor should produce different buildKey", a.buildKey, d.buildKey)

        val e = DiagnosticsInterop.BuildIdentity("1.0", 1, "abc1234", "noAi", "release", "com.example")
        assertNotEquals("Different buildType should produce different buildKey", a.buildKey, e.buildKey)
    }
}
