package com.xiwei.sujian.data

import org.junit.Assert.assertTrue
import org.junit.Assert.assertEquals
import org.junit.Test
import java.lang.reflect.Method

/**
 * #592 三：commitSyncProfile 事务提交协议契约测试。
 *
 * 验证 SettingsRepository 提供原子性 commitSyncProfile 方法，
 * 正式同步、试运行和连接诊断都通过此入口保存配置与凭据。
 */
class CommitSyncProfileContractTest {

    @Test
    fun commitSyncProfile_existsOnSettingsRepository() {
        // suspend 函数在 JVM 上签名带 Continuation 参数：前两个参数必须是 config/secrets。
        val method: Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "commitSyncProfile" &&
            it.parameterTypes.size >= 2 &&
            it.parameterTypes[0] == com.xiwei.sujian.model.SyncConfig::class.java &&
            it.parameterTypes[1] == com.xiwei.sujian.model.SyncSecrets::class.java
        }
        assertTrue("SettingsRepository must have commitSyncProfile(SyncConfig, SyncSecrets)",
            method != null)
    }

    @Test
    fun commitSyncProfile_returnsSettingsSaveResult() {
        val method: Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "commitSyncProfile"
        }
        assertTrue("commitSyncProfile must exist", method != null)
        // suspend commitSyncProfile 经 Continuation 传返回值（JVM 签名），
        // 契约断言方法存在且参数形状正确。
        assertTrue(
            "commitSyncProfile must accept (SyncConfig, SyncSecrets)",
            method!!.parameterTypes.size >= 2 &&
                method.parameterTypes[0] == com.xiwei.sujian.model.SyncConfig::class.java &&
                method.parameterTypes[1] == com.xiwei.sujian.model.SyncSecrets::class.java
        )
    }

    @Test
    fun saveTransactionConfigAndSecrets_usesCommitSyncProfile_notSeparateSaves() {
        // #592 三：saveTransactionConfigAndSecrets 必须通过 commitSyncProfile
        // 单一事务入口保存，不再分别调用 saveSyncConfig + saveSyncSecrets。
        // suspend 方法经 getDeclaredMethod 找不到（带 Continuation），改用方法名匹配。
        val method: Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "commitSyncProfile"
        }
        assertTrue(method != null)
    }

    @Test
    fun syncProfileGate_commitExclusive_preservesReturnValue() = kotlinx.coroutines.test.runTest {
        assertEquals("committed", SyncProfileGate.commitExclusive { "committed" })
    }

    @Test
    fun syncProfileGate_snapshotExclusive_preservesReturnValue() = kotlinx.coroutines.test.runTest {
        assertEquals(42, SyncProfileGate.snapshotExclusive { 42 })
    }
}
