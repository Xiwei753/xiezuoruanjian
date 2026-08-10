package com.xiwei.sujian.app.theme

import android.content.Context
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.sync.data.SyncRepository
import com.xiwei.sujian.feature.sync.data.SyncStatusRepository
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState
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
 * 依赖（含 syncStatusRepository），不再反向依赖 CompositionLocal。
 *
 * 正：不提供 LocalSujianAppDependencies 也能完成组合并返回控制器；
 * 反：若实现重新引入 CompositionLocal 读取，本测试直接抛异常失败。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ThemeControllerDependenciesTest {
    @get:Rule
    val composeRule = createComposeRule()

    private lateinit var settingsRepository: SettingsRepository
    private lateinit var themeRepository: ThemeRepository
    private lateinit var syncStatusRepository: SyncStatusRepository

    @Before
    fun setUp() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val dir = java.nio.file.Files.createTempDirectory("sujian_theme_deps_test_").toString()
        val bridge = AppServiceBridge(WriterAppServiceHolder(dir, dir))
        settingsRepository = SettingsRepository(context, bridge)
        themeRepository = ThemeRepository(context, bridge)
        syncStatusRepository = SyncStatusRepository(SyncRepository(context, bridge))
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
                        syncStatusRepository = syncStatusRepository,
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
    fun onResume_refreshesThemeFromInjectedSyncStatusRepository() {
        // 注入的仓库置为 Synced：Activity 进入 RESUMED 时 ON_RESUME 观察者
        // 必须通过注入的 syncStatusRepository 读到 Synced 并触发 ThemeStore 刷新。
        syncStatusRepository.notifySyncSuccess()
        assertEquals(SyncIndicatorState.Synced, syncStatusRepository.state.value)

        composeRule.setContent {
            rememberThemeController(
                context = LocalContext.current,
                settingsRepository = settingsRepository,
                themeRepository = themeRepository,
                syncStatusRepository = syncStatusRepository,
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
