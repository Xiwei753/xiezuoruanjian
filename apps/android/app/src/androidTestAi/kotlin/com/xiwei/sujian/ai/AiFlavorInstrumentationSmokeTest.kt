package com.xiwei.sujian.ai

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.BuildConfig
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

/**
 * AI flavor 设备侧冒烟测试（androidTestAi 源集）。

 * 仅在 ai 变体接入测试设备时运行，验证 BuildConfig.FLAVOR 在设备侧仍为 "ai"，
 * 与 testAi 单元测试形成上下互补。该测试不依赖任何 Core 或 Bridge 状态，
 * 只校验构建配置契约。
 */
@RunWith(AndroidJUnit4::class)
class AiFlavorInstrumentationSmokeTest {
    @Test
    fun flavor_is_ai_on_device() {
        assertEquals("ai", BuildConfig.FLAVOR)
    }
}
