package com.xiwei.sujian.arch

import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.GenerationSecretsReadResult
import org.junit.Assert.assertNotNull
import org.junit.Test

/**
 * #595 四：legacy 槽凭据读取类型化结构契约测试（#597 移入独立架构检查集合）。
 *
 * 验证 loadLegacySyncSecretsTyped 存在且返回 GenerationSecretsReadResult。
 */
class LegacySecretsTypedReadArchitectureTest {

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
}
