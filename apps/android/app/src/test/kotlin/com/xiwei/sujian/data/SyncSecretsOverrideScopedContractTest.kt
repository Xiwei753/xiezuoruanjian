package com.xiwei.sujian.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 十：secrets override 必须是操作作用域参数契约测试。
 *
 * 旧缺陷：
 * - override 在取得同步独占锁（runExclusive）之前写入 — 两个同步同时触发时
 *   可能发生 A 写 token A、B 写 token B、A 实际使用 token B；
 * - setSyncSecretsOverride 吞掉失败并返回 Unit，失败后仍继续同步；
 * - override 没有在操作结束后清除（Core refresh_secrets_override 在已有
 *   override 时不会重新读取磁盘）；
 * - DryRun/Diagnostics 不设置自己的 snapshot override，继续使用上次正式同步
 *   留下的旧 token。
 *
 * 修复：override 只在 SyncSession.runExclusive 内设置，失败立即终止，
 * 结束后 finally 清除；正式同步、自动同步、DryRun、Diagnostics 共用同一执行器。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SyncSecretsOverrideScopedContractTest {

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
    fun setSyncSecretsOverrideStrict_failsWhenNativeMissing() {
        // 无 native 环境：strict 设置必须返回 false（调用方终止操作），
        // 不再像旧实现那样吞掉失败继续同步。
        val repo = SettingsRepository(
            androidx.test.core.app.ApplicationProvider.getApplicationContext(),
            AppServiceBridge(WriterAppServiceHolder("/tmp/sujian_test_workspace_595")),
        )
        assertFalse(
            "Strict override set must fail (not silently succeed) when native is unavailable",
            repo.setSyncSecretsOverrideStrict(com.xiwei.sujian.model.SyncSecrets(token = "token-a")),
        )
    }

    @Test
    fun runSync_noLongerCallsLegacySetSyncSecretsOverride() {
        // 旧入口必须删除：所有 override 写入走 setSyncSecretsOverrideStrict。
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
        // 生成绑定（构建时由 UDL 重新生成）必须包含 clearSyncSecretsOverride。
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
