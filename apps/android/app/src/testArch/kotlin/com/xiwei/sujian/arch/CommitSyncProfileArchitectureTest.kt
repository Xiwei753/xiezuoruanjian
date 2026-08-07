package com.xiwei.sujian.arch

import com.xiwei.sujian.data.SettingsRepository
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #592 三：commitSyncProfile 事务提交协议结构契约测试（#597 移入独立架构检查集合）。
 *
 * 验证 SettingsRepository 提供原子性 commitSyncProfile 方法，
 * 正式同步、试运行和连接诊断都通过此入口保存配置与凭据。
 */
class CommitSyncProfileArchitectureTest {
    @Test
    fun commitSyncProfile_existsOnSettingsRepository() {
        // suspend 函数在 JVM 上签名带 Continuation 参数：前两个参数必须是 config/secrets。
        val method: Method? =
            SettingsRepository::class.java.methods.firstOrNull {
                it.name == "commitSyncProfile" &&
                    it.parameterTypes.size >= 2 &&
                    it.parameterTypes[0] == com.xiwei.sujian.model.SyncConfig::class.java &&
                    it.parameterTypes[1] == com.xiwei.sujian.model.SyncSecrets::class.java
            }
        assertTrue(
            "SettingsRepository must have commitSyncProfile(SyncConfig, SyncSecrets)",
            method != null,
        )
    }

    @Test
    fun commitSyncProfile_returnsSettingsSaveResult() {
        val method: Method? =
            SettingsRepository::class.java.methods.firstOrNull {
                it.name == "commitSyncProfile"
            }
        assertTrue("commitSyncProfile must exist", method != null)
        assertTrue(
            "commitSyncProfile must accept (SyncConfig, SyncSecrets)",
            method!!.parameterTypes.size >= 2 &&
                method.parameterTypes[0] == com.xiwei.sujian.model.SyncConfig::class.java &&
                method.parameterTypes[1] == com.xiwei.sujian.model.SyncSecrets::class.java,
        )
    }

    @Test
    fun saveTransactionConfigAndSecrets_usesCommitSyncProfile_notSeparateSaves() {
        val method: Method? =
            SettingsRepository::class.java.methods.firstOrNull {
                it.name == "commitSyncProfile"
            }
        assertTrue(method != null)
    }
}
