package com.xiwei.sujian.core.interop.sync

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #600 评论 #4 问题三：AppSyncProfileReadResult sealed 层级行为测试。
 *
 * 镜像 [GenerationSecretsReadResultTest]（作品级）的测试模式，验证应用级
 * 同步配置完整快照读取的类型化结果：
 * - Found/NotConfigured 携带 [AppSyncProfileSnapshot]；
 * - Failed 携带 [SyncFailureKind] 和 message；
 * - 与 [SyncProfileReadResult]（作品级）结构对称但独立（不共享子类型）。
 */
class AppSyncProfileReadResultTest {
    @Test
    fun appSyncProfileReadResult_hasFoundNotConfiguredFailedSubtypes() {
        val snapshot =
            AppSyncProfileSnapshot(
                1L,
                com.xiwei.sujian.core.model.SyncConfig(),
                com.xiwei.sujian.core.model.SyncSecrets(),
            )
        val found: AppSyncProfileReadResult = AppSyncProfileReadResult.Found(snapshot)
        val notConfigured: AppSyncProfileReadResult = AppSyncProfileReadResult.NotConfigured(snapshot)
        val failed: AppSyncProfileReadResult = AppSyncProfileReadResult.Failed(SyncFailureKind.Fatal, "err")
        assertTrue("Found must be AppSyncProfileReadResult", found is AppSyncProfileReadResult.Found)
        assertTrue(
            "NotConfigured must be AppSyncProfileReadResult",
            notConfigured is AppSyncProfileReadResult.NotConfigured,
        )
        assertTrue("Failed must be AppSyncProfileReadResult", failed is AppSyncProfileReadResult.Failed)
    }

    @Test
    fun appSyncProfileReadResult_Failed_carriesSyncFailureKind() {
        val failed = AppSyncProfileReadResult.Failed(SyncFailureKind.NativeUnavailable, "msg")
        assertEquals(SyncFailureKind.NativeUnavailable, failed.kind)
        assertEquals("msg", failed.message)
    }

    @Test
    fun appSyncProfileReadResult_foundAndNotConfigured_carrySnapshot() {
        val snapshot =
            AppSyncProfileSnapshot(
                2L,
                com.xiwei.sujian.core.model.SyncConfig(enabled = true),
                com.xiwei.sujian.core.model.SyncSecrets(token = "t"),
            )
        val found = AppSyncProfileReadResult.Found(snapshot)
        val notConfigured = AppSyncProfileReadResult.NotConfigured(snapshot)
        assertTrue("Found must carry snapshot", found.snapshot == snapshot)
        assertTrue("NotConfigured must carry snapshot", notConfigured.snapshot == snapshot)
        assertEquals(2L, found.snapshot.generation)
        assertEquals("t", found.snapshot.secrets.token)
    }

    @Test
    fun appSyncProfileSnapshot_carriesGenerationConfigSecrets() {
        val snapshot =
            AppSyncProfileSnapshot(
                generation = 5L,
                config = com.xiwei.sujian.core.model.SyncConfig(enabled = true),
                secrets = com.xiwei.sujian.core.model.SyncSecrets(token = "app-token"),
            )
        assertTrue(snapshot.generation == 5L)
        assertTrue(snapshot.config.enabled == true)
        assertTrue(snapshot.secrets.token == "app-token")
    }

    @Test
    fun appSyncProfileReadResult_isDistinctFromProjectSyncProfileReadResult() {
        // #600 评论 #4 问题三：应用级与作品级 ReadResult 是独立 sealed interface，
        // 不共享子类型 — 避免泛型化扩散。
        val appSnapshot =
            AppSyncProfileSnapshot(
                1L,
                com.xiwei.sujian.core.model.SyncConfig(),
                com.xiwei.sujian.core.model.SyncSecrets(),
            )
        val projectSnapshot =
            ProjectSyncProfileSnapshot(
                1L,
                com.xiwei.sujian.core.model.SyncConfig(),
                com.xiwei.sujian.core.model.SyncSecrets(),
            )
        val appFound = AppSyncProfileReadResult.Found(appSnapshot)
        val projectFound = SyncProfileReadResult.Found(projectSnapshot)
        // 类型不同 — AppSyncProfileReadResult.Found 不是 SyncProfileReadResult.Found
        assertTrue("App Found must not be project SyncProfileReadResult", appFound !is SyncProfileReadResult)
        assertTrue("Project Found must not be AppSyncProfileReadResult", projectFound !is AppSyncProfileReadResult)
    }
}
