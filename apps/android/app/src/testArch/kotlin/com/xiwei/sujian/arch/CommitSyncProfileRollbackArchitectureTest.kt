package com.xiwei.sujian.arch

import com.xiwei.sujian.data.AutoSyncScheduler
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SyncProfileGate
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #592 五/六：版本化提交与完整快照结构契约测试（#597 移入独立架构检查集合）。
 *
 * 验证方法存在性与参数形状：
 * - commitSyncProfile(SyncConfig, SyncSecrets) 存在；
 * - SyncProfileGate.commitExclusive / snapshotExclusive 存在；
 * - snapshotSyncProfile 存在；
 * - AutoSyncScheduler.scheduleFromSettings 接收容器仓库；
 * - loadSyncConfigStrict 存在（提交前捕获旧值）。
 */
class CommitSyncProfileRollbackArchitectureTest {
    @Test
    fun commitSyncProfile_existsWithTwoParams() {
        val method: Method? =
            SettingsRepository::class.java.methods.firstOrNull {
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
        val method: Method? =
            SettingsRepository::class.java.methods.firstOrNull {
                it.name == "snapshotSyncProfile"
            }
        assertTrue("SettingsRepository must provide snapshotSyncProfile() for #592 六", method != null)
    }

    @Test
    fun scheduleFromSettings_acceptsContainerRepository() {
        val method: Method? =
            AutoSyncScheduler.Companion::class.java.methods.firstOrNull {
                it.name == "scheduleFromSettings" &&
                    it.parameterTypes.size >= 2 &&
                    it.parameterTypes[0] == android.content.Context::class.java &&
                    it.parameterTypes[1] == SettingsRepository::class.java
            }
        assertTrue(
            "AutoSyncScheduler.scheduleFromSettings must accept the app container SettingsRepository " +
                "(no second repository created mid-commit)",
            method != null,
        )
    }

    @Test
    fun legacyRollbackReading_existsForDiagnosticsOnly() {
        val strictMethod: Method? =
            SettingsRepository::class.java.methods.firstOrNull {
                it.name == "loadSyncConfigStrict"
            }
        assertTrue("SettingsRepository.loadSyncConfigStrict must exist for pre-write capture", strictMethod != null)
    }
}
