package com.xiwei.sujian.arch

import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SyncBridge
import com.xiwei.sujian.data.AppServiceBridge
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 十：secrets override 操作作用域结构契约测试（#597 移入独立架构检查集合）。
 *
 * 验证方法存在性与旧入口已删除：
 * - setSyncSecretsOverrideStrict(SyncSecrets): Boolean 存在；
 * - clearSyncSecretsOverride 在 Repository/Bridge/Core 上存在；
 * - 旧 swallow-failure setSyncSecretsOverride 已删除；
 * - UniFFI 绑定暴露 clearSyncSecretsOverride。
 */
class SyncSecretsOverrideScopedArchitectureTest {

    @Test
    fun setSyncSecretsOverrideStrict_existsAndReturnsBoolean() {
        val method: java.lang.reflect.Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "setSyncSecretsOverrideStrict" &&
            it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == com.xiwei.sujian.model.SyncSecrets::class.java &&
            it.returnType == Boolean::class.javaPrimitiveType
        }
        assertNotNull(
            "SettingsRepository.setSyncSecretsOverrideStrict(SyncSecrets): Boolean must exist — " +
            "failure must abort the operation",
            method,
        )
    }

    @Test
    fun clearSyncSecretsOverride_existsOnBridgeAndRepository() {
        val repoMethod: java.lang.reflect.Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "clearSyncSecretsOverride"
        }
        assertNotNull(
            "SettingsRepository.clearSyncSecretsOverride must exist — override must be cleared " +
            "after the operation (#595 十)",
            repoMethod,
        )
        val bridgeMethod: java.lang.reflect.Method? = SyncBridge::class.java.methods.firstOrNull {
            it.name == "clearSyncSecretsOverride"
        }
        assertNotNull("SyncBridge.clearSyncSecretsOverride must exist", bridgeMethod)
        val coreMethod: java.lang.reflect.Method? = AppServiceBridge::class.java.methods.firstOrNull {
            it.name == "clearSyncSecretsOverride"
        }
        assertNotNull("AppServiceBridge.clearSyncSecretsOverride must exist", coreMethod)
    }

    @Test
    fun runSync_noLongerCallsLegacySetSyncSecretsOverride() {
        val legacyMethod: java.lang.reflect.Method? = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "setSyncSecretsOverride" && it.parameterTypes.size == 1
        }
        assertTrue(
            "Legacy swallow-failure setSyncSecretsOverride must be removed (#595 十)",
            legacyMethod == null,
        )
    }

    @Test
    fun uniffiBinding_hasClearSyncSecretsOverride() {
        val serviceMethods = uniffi.writer_core.WriterAppService::class.java.methods.filter {
            it.name == "clearSyncSecretsOverride"
        }
        assertEquals(
            "UniFFI binding must expose clearSyncSecretsOverride (UDL contract)",
            1,
            serviceMethods.size,
        )
    }
}
