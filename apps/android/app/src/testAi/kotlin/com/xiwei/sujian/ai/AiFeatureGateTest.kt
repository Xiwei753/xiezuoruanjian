package com.xiwei.sujian.ai

import com.xiwei.sujian.BuildConfig
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * AI flavor 功能开关契约测试。

 * 该测试只存在于 `testAi` 源集，验证 AI 变体的 BuildConfig.FLAVOR 等于 "ai"，
 * 保证 flavor isolation 没有被构建脚本意外破坏（AGENTS.md：从根因修改，
 * 不允许旁路或第二套状态机）。
 *
 * testAiDebugUnitTest 通过 Gradle 官方测试过滤
 * `includeTestsMatching("com.xiwei.sujian.ai.*")` 限定只运行本包测试，
 * 并设置 `isFailOnNoMatchingTests = true` 防止过滤被静默掏空。
 */
class AiFeatureGateTest {
    @Test
    fun flavor_is_ai() {
        // BuildConfig.FLAVOR 由 AGP 按 flavorDimensions "ai" 生成，
        // ai 变体下值为 "ai"，noAi 变体下值为 "noAi"。
        assertTrue(
            "AI flavor 的 BuildConfig.FLAVOR 必须为 'ai'，实际为 ${BuildConfig.FLAVOR}",
            "ai" == BuildConfig.FLAVOR,
        )
    }

    @Test
    fun flavor_is_not_noAi() {
        // 反向断言：防止 flavor 配置被错误地回退到 noAi。
        assertFalse(
            "AI flavor 不应等于 'noAi'，实际为 ${BuildConfig.FLAVOR}",
            "noAi" == BuildConfig.FLAVOR,
        )
    }
}
