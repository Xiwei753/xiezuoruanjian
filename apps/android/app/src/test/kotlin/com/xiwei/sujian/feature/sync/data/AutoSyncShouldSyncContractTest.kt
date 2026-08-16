package com.xiwei.sujian.feature.sync.data

import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.work.AutoSyncScheduler
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #600 评论 #3 问题三：AutoSyncWorker 遍历作品时用 shouldSync 决定哪些作品需要同步。
 *
 * 本测试固定 shouldSync 纯函数的契约 — AutoSyncWorker.doWork 对每个作品读取
 * snapshotSyncProfile() 后用 shouldSync(snapshot.config, snapshot.secrets)
 * 判定是否跳过。不同作品的 config/secrets 互不干扰（per-project 隔离）。
 */
class AutoSyncShouldSyncContractTest {
    private fun config(
        enabled: Boolean? = null,
        autoSync: Boolean? = null,
        remoteUrl: String? = null,
    ) = SyncConfig(enabled = enabled, autoSync = autoSync, remoteUrl = remoteUrl)

    private fun secrets(token: String? = null) = SyncSecrets(token = token)

    @Test
    fun disabledConfig_skipsSync() {
        assertFalse(
            AutoSyncScheduler.shouldSync(
                config(enabled = false, autoSync = true, remoteUrl = "u"),
                secrets("t"),
            ),
        )
    }

    @Test
    fun autoSyncOff_skipsSync() {
        assertFalse(
            AutoSyncScheduler.shouldSync(
                config(enabled = true, autoSync = false, remoteUrl = "u"),
                secrets("t"),
            ),
        )
    }

    @Test
    fun noRemoteUrl_skipsSync() {
        assertFalse(
            AutoSyncScheduler.shouldSync(
                config(enabled = true, autoSync = true, remoteUrl = null),
                secrets("t"),
            ),
        )
        assertFalse(
            AutoSyncScheduler.shouldSync(
                config(enabled = true, autoSync = true, remoteUrl = ""),
                secrets("t"),
            ),
        )
    }

    @Test
    fun noToken_skipsSync() {
        assertFalse(
            AutoSyncScheduler.shouldSync(
                config(enabled = true, autoSync = true, remoteUrl = "u"),
                secrets(null),
            ),
        )
        assertFalse(
            AutoSyncScheduler.shouldSync(
                config(enabled = true, autoSync = true, remoteUrl = "u"),
                secrets(""),
            ),
        )
    }

    @Test
    fun fullyConfigured_syncs() {
        assertTrue(
            AutoSyncScheduler.shouldSync(
                config(enabled = true, autoSync = true, remoteUrl = "https://example.com/r.git"),
                secrets("token-xyz"),
            ),
        )
    }

    @Test
    fun differentProjectsIndependentConfig() {
        // #600 评论 #3 问题二：两个作品的 config 独立判定 —
        // 作品 A 已配置，作品 B 未配置，shouldSync 对 A 返回 true、对 B 返回 false。
        val configA = config(enabled = true, autoSync = true, remoteUrl = "https://a.git")
        val secretsA = secrets("token-a")
        val configB = config(enabled = false, autoSync = true, remoteUrl = "https://b.git")
        val secretsB = secrets("token-b")
        assertTrue(AutoSyncScheduler.shouldSync(configA, secretsA))
        assertFalse(AutoSyncScheduler.shouldSync(configB, secretsB))
    }
}
