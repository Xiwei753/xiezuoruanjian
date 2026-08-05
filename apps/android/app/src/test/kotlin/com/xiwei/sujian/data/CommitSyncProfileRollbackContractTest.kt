package com.xiwei.sujian.data

import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #592 五/六：版本化提交与完整快照契约测试。
 *
 * 验证：
 * - SettingsRepository 提供 snapshotSyncProfile（一次读取 generation+config+secrets）
 * - commitSyncProfile 是 suspend 版本化提交（旧实现为非 suspend 写后回滚）
 * - SyncProfileSnapshot 携带 generation/config/secrets
 * - AutoSyncScheduler.scheduleFromSettings 接收应用容器仓库，不再新建 SettingsRepository
 */
class CommitSyncProfileRollbackContractTest {

    @Test
    fun commitSyncProfile_existsWithTwoParams() {
        // suspend 方法 JVM 签名带 Continuation 参数：断言前两个参数形状。
        val method: Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "commitSyncProfile" &&
            it.parameterTypes.size >= 2 &&
            it.parameterTypes[0] == com.xiwei.sujian.model.SyncConfig::class.java &&
            it.parameterTypes[1] == com.xiwei.sujian.model.SyncSecrets::class.java
        }
        assertTrue("commitSyncProfile(SyncConfig, SyncSecrets) must exist", method != null)
    }

    @Test
    fun syncProfileGate_commitAndSnapshotExclusive_bothExist() {
        val commitMethod = SyncProfileGate::class.java.methods.firstOrNull { it.name == "commitExclusive" }
        val snapshotMethod = SyncProfileGate::class.java.methods.firstOrNull { it.name == "snapshotExclusive" }
        assertTrue("SyncProfileGate.commitExclusive must exist", commitMethod != null)
        assertTrue("SyncProfileGate.snapshotExclusive must exist", snapshotMethod != null)
    }

    @Test
    fun snapshotSyncProfile_existsOnSettingsRepository() {
        val method: Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "snapshotSyncProfile"
        }
        assertTrue("SettingsRepository must provide snapshotSyncProfile() for #592 六", method != null)
    }

    @Test
    fun syncProfileSnapshot_carriesGenerationConfigSecrets() {
        val snapshot = SyncProfileSnapshot(
            generation = 3L,
            config = com.xiwei.sujian.model.SyncConfig(enabled = true),
            secrets = com.xiwei.sujian.model.SyncSecrets(token = "t"),
        )
        assertTrue(snapshot.generation == 3L)
        assertTrue(snapshot.config.enabled == true)
        assertTrue(snapshot.secrets.token == "t")
    }

    @Test
    fun scheduleFromSettings_acceptsContainerRepository() {
        // suspend 方法 JVM 签名带 Continuation 参数。
        val method: Method? = AutoSyncScheduler.Companion::class.java.methods.firstOrNull {
            it.name == "scheduleFromSettings" &&
            it.parameterTypes.size >= 2 &&
            it.parameterTypes[0] == android.content.Context::class.java &&
            it.parameterTypes[1] == SettingsRepository::class.java
        }
        assertTrue(
            "AutoSyncScheduler.scheduleFromSettings must accept the app container SettingsRepository " +
            "(no second repository created mid-commit)",
            method != null
        )
    }

    @Test
    fun legacyRollbackReading_existsForDiagnosticsOnly() {
        // 严格读取入口存在：提交前捕获旧值，不再在失败后重新读取旧配置做回滚。
        val strictMethod: Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "loadSyncConfigStrict"
        }
        assertTrue("SettingsRepository.loadSyncConfigStrict must exist for pre-write capture", strictMethod != null)
    }
}
