package com.xiwei.writerapp.model

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * SyncConfigNormalizeTest — 同步配置规范化测试
 *
 * 测试 SyncConfig.normalize() 方法的默认值填充逻辑。
 */
class SyncConfigNormalizeTest {
    @Test
    fun normalize_usesGithubApi_whenBackendTypeIsNull() {
        val normalized = SyncConfig(backendType = null).normalize()
        assertEquals(BackendType.GithubApi, normalized.backendType)
    }
}
