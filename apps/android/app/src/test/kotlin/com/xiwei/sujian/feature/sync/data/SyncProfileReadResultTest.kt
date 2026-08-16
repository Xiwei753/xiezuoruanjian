package com.xiwei.sujian.feature.sync.data

import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #630 评论 #1：SyncProfileReadResult sealed 层级行为测试。
 *
 * 验证全局同步配置完整快照读取的类型化结果：
 * - Found/NotConfigured 携带 [SyncProfileSnapshot]；
 * - Failed 携带 [SyncFailureKind] 和 message。
 */
class SyncProfileReadResultTest {
    @Test
    fun appSyncProfileReadResult_hasFoundNotConfiguredFailedSubtypes() {
        val snapshot =
            SyncProfileSnapshot(
                1L,
                com.xiwei.sujian.feature.sync.data.model.SyncConfig(),
                com.xiwei.sujian.feature.sync.data.model.SyncSecrets(),
            )
        val found: SyncProfileReadResult = SyncProfileReadResult.Found(snapshot)
        val notConfigured: SyncProfileReadResult = SyncProfileReadResult.NotConfigured(snapshot)
        val failed: SyncProfileReadResult = SyncProfileReadResult.Failed(SyncFailureKind.Fatal, "err")
        assertTrue("Found must be SyncProfileReadResult", found is SyncProfileReadResult.Found)
        assertTrue(
            "NotConfigured must be SyncProfileReadResult",
            notConfigured is SyncProfileReadResult.NotConfigured,
        )
        assertTrue("Failed must be SyncProfileReadResult", failed is SyncProfileReadResult.Failed)
    }

    @Test
    fun appSyncProfileReadResult_Failed_carriesSyncFailureKind() {
        val failed = SyncProfileReadResult.Failed(SyncFailureKind.NativeUnavailable, "msg")
        assertEquals(SyncFailureKind.NativeUnavailable, failed.kind)
        assertEquals("msg", failed.message)
    }

    @Test
    fun appSyncProfileReadResult_foundAndNotConfigured_carrySnapshot() {
        val snapshot =
            SyncProfileSnapshot(
                2L,
                com.xiwei.sujian.feature.sync.data.model.SyncConfig(enabled = true),
                com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "t"),
            )
        val found = SyncProfileReadResult.Found(snapshot)
        val notConfigured = SyncProfileReadResult.NotConfigured(snapshot)
        assertTrue("Found must carry snapshot", found.snapshot == snapshot)
        assertTrue("NotConfigured must carry snapshot", notConfigured.snapshot == snapshot)
        assertEquals(2L, found.snapshot.generation)
        assertEquals("t", found.snapshot.secrets.token)
    }

    @Test
    fun appSyncProfileSnapshot_carriesGenerationConfigSecrets() {
        val snapshot =
            SyncProfileSnapshot(
                generation = 5L,
                config = com.xiwei.sujian.feature.sync.data.model.SyncConfig(enabled = true),
                secrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "app-token"),
            )
        assertTrue(snapshot.generation == 5L)
        assertTrue(snapshot.config.enabled == true)
        assertTrue(snapshot.secrets.token == "app-token")
    }
}
