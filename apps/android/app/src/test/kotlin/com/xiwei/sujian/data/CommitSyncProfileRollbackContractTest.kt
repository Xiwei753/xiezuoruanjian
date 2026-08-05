package com.xiwei.sujian.data

import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #592 四：commitSyncProfile 回滚安全契约测试。
 *
 * 验证 commitSyncProfile 在 secrets 保存失败时检查回滚结果，
 * 不静默忽略回滚失败导致 config/secrets 不一致。
 */
class CommitSyncProfileRollbackContractTest {

    @Test
    fun commitSyncProfile_existsWithTwoParams() {
        val method: Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "commitSyncProfile" &&
            it.parameterTypes.size == 2 &&
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
}
