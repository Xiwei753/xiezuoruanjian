package com.xiwei.sujian.data

import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 十：secrets override 操作作用域行为测试。
 *
 * 结构契约（方法存在性）已移入
 * [com.xiwei.sujian.arch.SyncSecretsOverrideScopedArchitectureTest]；本文件只保留运行时行为：
 * - 无 native 环境下 strict 设置必须返回 false（调用方终止操作）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SyncSecretsOverrideScopedTest {

    @Test
    fun setSyncSecretsOverrideStrict_failsWhenNativeMissing() {
        val repo = SettingsRepository(
            androidx.test.core.app.ApplicationProvider.getApplicationContext(),
            AppServiceBridge(WriterAppServiceHolder("/tmp/sujian_test_workspace_595")),
        )
        assertFalse(
            "Strict override set must fail (not silently succeed) when native is unavailable",
            repo.setSyncSecretsOverrideStrict(com.xiwei.sujian.model.SyncSecrets(token = "token-a")),
        )
    }
}
