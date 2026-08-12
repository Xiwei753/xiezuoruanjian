package com.xiwei.sujian.app.theme

import android.content.Context
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * #609 一：启动崩溃回归测试。
 *
 * 旧实现里 `rememberThemeController` 在 `LocalSujianAppDependencies` 建立
 * CompositionLocalProvider 之前读取 `.current`，首次进入 UI 必抛
 * `No SujianAppDependencies provided`。修复后主题控制器只消费显式注入的
 * 依赖（settings/theme 两个仓库），不再反向依赖 CompositionLocal。
 *
 * 正：不提供 LocalSujianAppDependencies 也能完成组合并返回控制器；
 * 反：若实现重新引入 CompositionLocal 读取，本测试直接抛异常失败。
 *
 * #618 三：同步状态不再参与主题刷新（旧代码的 Synced 分支与无条件 reload
 * 动作相同，是重复解析；`onSyncCompleted` 别名与 `syncStatusRepository`
 * 注入一并删除），ON_RESUME 只触发一次从注入仓库的完整主题解析。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ThemeControllerDependenciesTest {
    @get:Rule
    val composeRule = createComposeRule()

    private lateinit var settingsRepository: SettingsRepository
    private lateinit var themeRepository: ThemeRepository

    @Before
    fun setUp() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val dir = java.nio.file.Files.createTempDirectory("sujian_theme_deps_test_").toString()
        val bridge = AppServiceBridge(WriterAppServiceHolder(dir, dir))
        settingsRepository = SettingsRepository(context, bridge)
        themeRepository = ThemeRepository(context, bridge)
    }

    @Test
    fun rememberThemeController_composesWithoutLocalSujianAppDependencies() {
        var controller: ThemeController? = null
        var failure: Throwable? = null
        try {
            composeRule.setContent {
                controller =
                    rememberThemeController(
                        context = LocalContext.current,
                        settingsRepository = settingsRepository,
                        themeRepository = themeRepository,
                    )
            }
            composeRule.waitForIdle()
        } catch (t: Throwable) {
            failure = t
        }
        if (failure != null) {
            fail("组合期不得读取 LocalSujianAppDependencies（未提供时必须不崩溃），实际异常: $failure")
        }
        assertNotNull("必须返回 ThemeController 实例", controller)
    }

    @Test
    fun onResume_refreshesThemeFromInjectedSettingsRepository() {
        // Activity 进入 RESUMED 时 ON_RESUME 观察者通过注入的 settingsRepository
        // 触发一次完整主题刷新（#618 三：不再依赖同步状态，也不做重复解析）。
        composeRule.setContent {
            rememberThemeController(
                context = LocalContext.current,
                settingsRepository = settingsRepository,
                themeRepository = themeRepository,
            )
        }
        composeRule.waitForIdle()

        // ON_RESUME → handleThemeControllerOnResume → controller.reload()，
        // uiState 应来自注入的 settingsRepository（默认外观模式）。
        val uiState = ThemeStore.uiState.value
        assertEquals(
            settingsRepository.getLocalSettings().appearanceMode,
            uiState.appearanceMode,
        )
    }
}
