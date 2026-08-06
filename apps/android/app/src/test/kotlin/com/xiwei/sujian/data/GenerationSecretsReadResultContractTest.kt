package com.xiwei.sujian.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 五：generation 凭据类型化读取契约测试。
 *
 * 旧缺陷：loadSyncSecretsForGeneration 返回 nullable SyncSecrets?，把"没有 token"
 * 和"读取失败"压成同一个 null。snapshotSyncProfile 在 hasCommittedProfile 时
 * 把读取失败回退为 SyncSecrets()（空凭据），安全存储损坏被当成"未配置"。
 *
 * 修复：引入 GenerationSecretsReadResult / SyncProfileReadResult sealed interface，
 * 类型化区分 Found / NotConfigured / Failed。
 * #595 五：toConfigSecretsOrNull 已在生产路径删除 — 调用方直接消费类型化结果。
 */
class GenerationSecretsReadResultContractTest {

    @Test
    fun generationSecretsReadResult_hasFoundNotConfiguredFailedSubtypes() {
        // #595 五：直接引用子类验证它们存在 — 编译时即保证类型安全。
        val found: GenerationSecretsReadResult = GenerationSecretsReadResult.Found(
            com.xiwei.sujian.model.SyncSecrets(token = "t")
        )
        val notConfigured: GenerationSecretsReadResult = GenerationSecretsReadResult.NotConfigured
        val failed: GenerationSecretsReadResult = GenerationSecretsReadResult.Failed(
            SyncFailureKind.Fatal, "err"
        )
        assertTrue("Found must be GenerationSecretsReadResult", found is GenerationSecretsReadResult.Found)
        assertTrue("NotConfigured must be GenerationSecretsReadResult", notConfigured is GenerationSecretsReadResult.NotConfigured)
        assertTrue("Failed must be GenerationSecretsReadResult", failed is GenerationSecretsReadResult.Failed)
    }

    @Test
    fun syncProfileReadResult_hasFoundNotConfiguredFailedSubtypes() {
        // #595 五：直接引用子类验证它们存在 — 编译时即保证类型安全。
        val snapshot = SyncProfileSnapshot(
            1L,
            com.xiwei.sujian.model.SyncConfig(),
            com.xiwei.sujian.model.SyncSecrets(),
        )
        val found: SyncProfileReadResult = SyncProfileReadResult.Found(snapshot)
        val notConfigured: SyncProfileReadResult = SyncProfileReadResult.NotConfigured(snapshot)
        val failed: SyncProfileReadResult = SyncProfileReadResult.Failed(SyncFailureKind.Fatal, "err")
        assertTrue("Found must be SyncProfileReadResult", found is SyncProfileReadResult.Found)
        assertTrue("NotConfigured must be SyncProfileReadResult", notConfigured is SyncProfileReadResult.NotConfigured)
        assertTrue("Failed must be SyncProfileReadResult", failed is SyncProfileReadResult.Failed)
    }

    @Test
    fun loadSyncSecretsForGeneration_returnsGenerationSecretsReadResult() {
        val method = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "loadSyncSecretsForGeneration" &&
            it.returnType == GenerationSecretsReadResult::class.java
        }
        assertNotNull(
            "loadSyncSecretsForGeneration must return GenerationSecretsReadResult, not nullable SyncSecrets",
            method,
        )
    }

    @Test
    fun deleteSyncSecretsForGeneration_existsOnSettingsRepository() {
        // #595 五：SettingsRepository 提供旧 generation 凭据删除（Long 参数，
        // 无 inline-class 名称 mangling）。
        val method = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "deleteSyncSecretsForGeneration" && it.parameterTypes.size == 1
        }
        assertNotNull(
            "SettingsRepository must expose deleteSyncSecretsForGeneration (#595 五)",
            method,
        )
    }

    @Test
    fun generationSecretsReadResult_Failed_carriesSyncFailureKind() {
        val failed = GenerationSecretsReadResult.Failed(SyncFailureKind.NativeUnavailable, "msg")
        assertEquals(SyncFailureKind.NativeUnavailable, failed.kind)
        assertEquals("msg", failed.message)
    }

    @Test
    fun syncProfileReadResult_Failed_carriesSyncFailureKind() {
        val snapshot = SyncProfileSnapshot(
            1L,
            com.xiwei.sujian.model.SyncConfig(),
            com.xiwei.sujian.model.SyncSecrets(),
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
