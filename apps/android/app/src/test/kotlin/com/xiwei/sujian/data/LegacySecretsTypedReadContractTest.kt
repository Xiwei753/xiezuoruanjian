package com.xiwei.sujian.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 四：legacy 槽凭据读取的类型化契约测试。
 *
 * 旧缺陷：snapshotSyncProfile 的 legacy 分支用 `loadSyncSecretsStrict() ?: SyncSecrets()`，
 * 原生库未加载、安全存储读取失败、解密失败全部被转换为空凭据，最后返回 NotConfigured —
 * 用户看到“未配置 token”，实际是读取失败。
 *
 * 修复：loadLegacySyncSecretsTyped() 返回 GenerationSecretsReadResult —
 * 读取失败（含 NativeUnavailable）返回 Failed，只有读取成功且 token 为空才返回 NotConfigured。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class LegacySecretsTypedReadContractTest {

    private fun createRepo(): SettingsRepository {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        return SettingsRepository(context)
    }

    @Test
    fun loadLegacySyncSecretsTyped_existsAndReturnsTypedResult() {
        val method = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "loadLegacySyncSecretsTyped" &&
                it.returnType == GenerationSecretsReadResult::class.java
        }
        assertNotNull(
            "loadLegacySyncSecretsTyped must return GenerationSecretsReadResult",
            method,
        )
    }

    @Test
    fun nativeUnavailable_isFailedNotNotConfigured() {
        // 测试环境无 native 库：BridgeResult.NotLoaded → Failed(NativeUnavailable)。
        // 旧实现返回 null → 被转换成 SyncSecrets() → NotConfigured（错误）。
        val result = createRepo().loadLegacySyncSecretsTyped()
        assertTrue(
            "原生库未加载必须返回 Failed(NativeUnavailable)，不得伪装成 NotConfigured，实际: $result",
            result is GenerationSecretsReadResult.Failed &&
                (result as GenerationSecretsReadResult.Failed).kind == SyncFailureKind.NativeUnavailable,
        )
    }

    @Test
    fun snapshotSyncProfile_neverMasksReadFailureAsNotConfigured() {
        // 测试环境无 native 库：config/凭据读取全部失败。snapshotSyncProfile 必须
        // 返回 Failed — 旧实现会把 legacy 凭据失败转成 SyncSecrets() 后返回
        // NotConfigured（用户看到“未配置 token”，实际是读取失败）。
        val result = kotlinx.coroutines.runBlocking {
            SyncProfileGate.snapshotExclusive {
                createRepo().snapshotSyncProfile()
            }
        }
        assertTrue(
            "读取失败必须返回 SyncProfileReadResult.Failed，不得伪装成 NotConfigured，实际: $result",
            result is SyncProfileReadResult.Failed,
        )
        assertTrue(
            "读取失败不是 NotConfigured",
            result !is SyncProfileReadResult.NotConfigured,
        )
    }
}
