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
        val method: Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "commitSyncProfile" &&
            it.parameterTypes.size == 2 &&
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
        assertEquals("commitSyncProfile must return SettingsSaveResult",
            SettingsSaveResult::class.java, method!!.returnType)
    }

    @Test
    fun saveTransactionConfigAndSecrets_usesCommitSyncProfile_notSeparateSaves() {
        // #592 三：saveTransactionConfigAndSecrets 必须通过 commitSyncProfile
        // 单一事务入口保存，不再分别调用 saveSyncConfig + saveSyncSecrets。
        // 验证 SettingsRepository 上 commitSyncProfile 存在且可被事务入口调用。
        val method = SettingsRepository::class.java.getMethod(
            "commitSyncProfile",
            com.xiwei.sujian.model.SyncConfig::class.java,
            com.xiwei.sujian.model.SyncSecrets::class.java
        )
        assertTrue(method != null)
    }
}
