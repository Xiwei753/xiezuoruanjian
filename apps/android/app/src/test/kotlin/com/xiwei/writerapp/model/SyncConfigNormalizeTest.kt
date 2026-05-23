package com.xiwei.writerapp.model

import org.junit.Assert.assertEquals
import org.junit.Test

class SyncConfigNormalizeTest {
    @Test
    fun normalize_usesGithubApi_whenBackendTypeIsNull() {
        val normalized = SyncConfig(backendType = null).normalize()
        assertEquals(BackendType.GithubApi, normalized.backendType)
    }
}
