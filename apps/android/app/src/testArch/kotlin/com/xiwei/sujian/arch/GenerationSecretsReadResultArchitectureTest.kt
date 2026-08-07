package com.xiwei.sujian.arch

import com.xiwei.sujian.data.GenerationSecretsReadResult
import com.xiwei.sujian.data.SettingsRepository
import org.junit.Assert.assertNotNull
import org.junit.Test

/**
 * #595 五：generation 凭据类型化读取结构契约测试（#597 移入独立架构检查集合）。
 *
 * 验证方法存在性与返回类型：
 * - loadSyncSecretsForGeneration 返回 GenerationSecretsReadResult；
 * - deleteSyncSecretsForGeneration 存在。
 */
class GenerationSecretsReadResultArchitectureTest {
    @Test
    fun loadSyncSecretsForGeneration_returnsGenerationSecretsReadResult() {
        val method =
            SettingsRepository::class.java.methods.firstOrNull {
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
        val method =
            SettingsRepository::class.java.methods.firstOrNull {
                it.name == "deleteSyncSecretsForGeneration" && it.parameterTypes.size == 1
            }
        assertNotNull(
            "SettingsRepository must expose deleteSyncSecretsForGeneration (#595 五)",
            method,
        )
    }
}
