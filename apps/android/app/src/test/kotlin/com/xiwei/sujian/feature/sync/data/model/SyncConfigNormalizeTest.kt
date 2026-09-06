package com.xiwei.sujian.feature.sync.data.model

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * SyncConfigNormalizeTest — 同步配置规范化测试
 *
 * 测试 SyncConfig.normalize() 方法的默认值填充逻辑。
 */
class SyncConfigNormalizeTest {
    @Test
    fun normalize_usesGithub_whenActiveProviderIsNull() {
        val normalized = SyncConfig(activeProvider = null).normalize()
        assertEquals("github", normalized.activeProvider)
    }
}
