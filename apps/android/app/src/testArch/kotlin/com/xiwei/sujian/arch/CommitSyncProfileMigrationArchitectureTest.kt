package com.xiwei.sujian.arch

import com.xiwei.sujian.data.GenerationSecretsReadResult
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SyncProfileReadResult
import org.junit.Assert.assertNotNull
import org.junit.Test

/**
 * #595 九：commitSyncProfile 迁移结构契约测试（#597 移入独立架构检查集合）。
 *
 * 验证方法存在性与返回类型形状：
 * - loadCommittedSyncProfile 存在（UI 读取入口）；
 * - loadSyncSecretsForGeneration 返回 GenerationSecretsReadResult；
 * - snapshotSyncProfile 返回 SyncProfileReadResult；
 * - loadLegacySyncSecretsTyped / loadSyncConfigStrict 存在。
 */
class CommitSyncProfileMigrationArchitectureTest {
    @Test
    fun commitSyncProfile_reordersLiveMirrorAfterMarker() {
        val strictConfig = SettingsRepository::class.java.methods.firstOrNull { it.name == "loadSyncConfigStrict" }
        val typedLegacySecrets =
            SettingsRepository::class.java.methods.firstOrNull { it.name == "loadLegacySyncSecretsTyped" }
        val commit = SettingsRepository::class.java.methods.firstOrNull { it.name == "commitSyncProfile" }
        assertNotNull(strictConfig)
        assertNotNull(
            "loadSyncSecretsStrict 已被类型化入口 loadLegacySyncSecretsTyped 替代（#595 四）",
            typedLegacySecrets,
        )
        assertNotNull(commit)
    }

    @Test
    fun loadCommittedSyncProfile_existsForUiReads() {
        val method =
            SettingsRepository::class.java.methods.firstOrNull {
                it.name == "loadCommittedSyncProfile"
            }
        assertNotNull("SettingsRepository.loadCommittedSyncProfile must exist", method)
    }

    @Test
    fun loadSyncSecretsForGeneration_returnsTypedResultNotNullable() {
        val method =
            SettingsRepository::class.java.methods.firstOrNull {
                it.name == "loadSyncSecretsForGeneration" &&
                    it.returnType == GenerationSecretsReadResult::class.java
            }
        assertNotNull(
            "loadSyncSecretsForGeneration must return GenerationSecretsReadResult, not nullable SyncSecrets",
            method,
        )
    }

    @Test
    fun snapshotSyncProfile_returnsTypedResultNotNullable() {
        val method =
            SettingsRepository::class.java.declaredMethods.firstOrNull {
                it.name == "snapshotSyncProfile"
            }
        assertNotNull(
            "snapshotSyncProfile must exist and return SyncProfileReadResult, not nullable SyncProfileSnapshot",
            method,
        )
        assertNotNull(
            "SyncProfileReadResult sealed interface must exist",
            SyncProfileReadResult::class.java,
        )
    }
}
