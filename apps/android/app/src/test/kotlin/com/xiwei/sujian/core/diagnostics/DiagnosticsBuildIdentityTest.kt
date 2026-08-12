@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.core.diagnostics

import com.xiwei.sujian.BuildConfig
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #623 评论 3：DiagnosticsBuildIdentity 构建身份与日志文件名分界契约测试。
 */
class DiagnosticsBuildIdentityTest {
    @Test
    fun fromBuildConfig_populatesAllFieldsFromBuildConfig() {
        val identity = DiagnosticsBuildIdentity.fromBuildConfig()
        assertEquals(BuildConfig.VERSION_NAME, identity.versionName)
        assertEquals(BuildConfig.VERSION_CODE, identity.versionCode)
        assertEquals(BuildConfig.GIT_COMMIT_SHA, identity.gitCommitSha)
        assertEquals(BuildConfig.FLAVOR, identity.flavor)
        assertEquals(BuildConfig.BUILD_TYPE, identity.buildType)
        assertEquals(BuildConfig.APPLICATION_ID, identity.applicationId)
    }

    @Test
    fun buildKey_hasExpectedFormat() {
        val identity = DiagnosticsBuildIdentity.fromBuildConfig()
        val expected = "v${identity.versionCode}-${identity.gitCommitSha}-${identity.flavor}-${identity.buildType}"
        assertEquals(expected, identity.buildKey)
    }

    @Test
    fun buildKey_startsWithVersionCodePrefix() {
        val identity = DiagnosticsBuildIdentity.fromBuildConfig()
        assertTrue(
            "buildKey should start with 'v' + versionCode, got ${identity.buildKey}",
            identity.buildKey.startsWith("v${identity.versionCode}-"),
        )
    }

    @Test
    fun buildKey_containsAllIdentityComponents() {
        val identity = DiagnosticsBuildIdentity.fromBuildConfig()
        assertTrue("buildKey should contain gitCommitSha", identity.buildKey.contains(identity.gitCommitSha))
        assertTrue("buildKey should contain flavor", identity.buildKey.contains(identity.flavor))
        assertTrue("buildKey should contain buildType", identity.buildKey.contains(identity.buildType))
    }

    @Test
    fun differentIdentities_produceDifferentBuildKeys() {
        val a = DiagnosticsBuildIdentity("1.0", 1, "abc1234", "noAi", "debug", "com.example")
        val b = DiagnosticsBuildIdentity("1.0", 2, "abc1234", "noAi", "debug", "com.example")
        assertNotEquals("Different versionCode should produce different buildKey", a.buildKey, b.buildKey)

        val c = DiagnosticsBuildIdentity("1.0", 1, "def5678", "noAi", "debug", "com.example")
        assertNotEquals("Different gitCommitSha should produce different buildKey", a.buildKey, c.buildKey)

        val d = DiagnosticsBuildIdentity("1.0", 1, "abc1234", "ai", "debug", "com.example")
        assertNotEquals("Different flavor should produce different buildKey", a.buildKey, d.buildKey)

        val e = DiagnosticsBuildIdentity("1.0", 1, "abc1234", "noAi", "release", "com.example")
        assertNotEquals("Different buildType should produce different buildKey", a.buildKey, e.buildKey)
    }
}
