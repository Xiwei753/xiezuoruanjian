package com.xiwei.sujian.feature.sync.data

import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 五：generation 凭据类型化读取行为测试。
 *
 * 结构契约（方法存在性）已移入
 * [com.xiwei.sujian.arch.GenerationSecretsReadResultArchitectureTest]；本文件只保留运行时行为：
 * - GenerationSecretsReadResult / SyncProfileReadResult sealed 层级子类存在且携带数据；
 * - Failed 携带 SyncFailureKind 和 message。
 */
class GenerationSecretsReadResultTest {
    @Test
    fun generationSecretsReadResult_hasFoundNotConfiguredFailedSubtypes() {
        val found: GenerationSecretsReadResult =
            GenerationSecretsReadResult.Found(
                com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "t"),
            )
        val notConfigured: GenerationSecretsReadResult = GenerationSecretsReadResult.NotConfigured
        val failed: GenerationSecretsReadResult =
            GenerationSecretsReadResult.Failed(
                SyncFailureKind.Fatal,
                "err",
            )
        assertTrue("Found must be GenerationSecretsReadResult", found is GenerationSecretsReadResult.Found)
        assertTrue(
            "NotConfigured must be GenerationSecretsReadResult",
            notConfigured is GenerationSecretsReadResult.NotConfigured,
        )
        assertTrue("Failed must be GenerationSecretsReadResult", failed is GenerationSecretsReadResult.Failed)
    }

    @Test
    fun syncProfileReadResult_hasFoundNotConfiguredFailedSubtypes() {
        val snapshot =
            ProjectSyncProfileSnapshot(
                1L,
                com.xiwei.sujian.feature.sync.data.model.SyncConfig(),
                com.xiwei.sujian.feature.sync.data.model.SyncSecrets(),
            )
        val found: SyncProfileReadResult = SyncProfileReadResult.Found(snapshot)
        val notConfigured: SyncProfileReadResult = SyncProfileReadResult.NotConfigured(snapshot)
        val failed: SyncProfileReadResult = SyncProfileReadResult.Failed(SyncFailureKind.Fatal, "err")
        assertTrue("Found must be SyncProfileReadResult", found is SyncProfileReadResult.Found)
        assertTrue("NotConfigured must be SyncProfileReadResult", notConfigured is SyncProfileReadResult.NotConfigured)
        assertTrue("Failed must be SyncProfileReadResult", failed is SyncProfileReadResult.Failed)
    }

    @Test
    fun generationSecretsReadResult_Failed_carriesSyncFailureKind() {
        val failed = GenerationSecretsReadResult.Failed(SyncFailureKind.NativeUnavailable, "msg")
        assertEquals(SyncFailureKind.NativeUnavailable, failed.kind)
        assertEquals("msg", failed.message)
    }

    @Test
    fun syncProfileReadResult_Failed_carriesSyncFailureKind() {
        val snapshot =
            ProjectSyncProfileSnapshot(
                1L,
                com.xiwei.sujian.feature.sync.data.model.SyncConfig(),
                com.xiwei.sujian.feature.sync.data.model.SyncSecrets(),
            )
        val found = SyncProfileReadResult.Found(snapshot)
        val notConfigured = SyncProfileReadResult.NotConfigured(snapshot)
        val failed = SyncProfileReadResult.Failed(SyncFailureKind.Fatal, "err")
        assertTrue("Found must carry snapshot", found.snapshot == snapshot)
        assertTrue("NotConfigured must carry snapshot", notConfigured.snapshot == snapshot)
        assertEquals(SyncFailureKind.Fatal, failed.kind)
        assertEquals("err", failed.message)
    }
}
