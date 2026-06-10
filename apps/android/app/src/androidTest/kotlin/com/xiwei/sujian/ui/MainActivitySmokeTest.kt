package com.xiwei.sujian.ui

import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/**
 * MainActivitySmokeTest — MainActivity 启动烟雾测试
 *
 * 验证应用的主界面是否能成功启动且不发生奔溃。
 */
@RunWith(AndroidJUnit4::class)
class MainActivitySmokeTest {

    @Test
    fun testMainActivityLaunch() {
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            assertNotNull("ActivityScenario should launch MainActivity", scenario)
        }
    }
}
