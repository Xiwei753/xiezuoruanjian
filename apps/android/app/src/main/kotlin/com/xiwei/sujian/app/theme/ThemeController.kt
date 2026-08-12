package com.xiwei.sujian.app.theme

import android.content.Context
import android.content.res.Configuration
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import kotlinx.coroutines.flow.StateFlow

class ThemeController(
    private val settingsRepository: SettingsRepository,
    private val themeRepository: ThemeRepository,
) {
    private val store: ThemeStore = ThemeStore

    val uiState: StateFlow<ThemeUiState>
        get() = store.uiState

    val paletteRecords: StateFlow<List<com.xiwei.sujian.app.theme.model.ThemePaletteRecord>>
        get() = store.paletteRecords

    fun reload() {
        store.reload()
    }

    fun refreshPaletteRecords() {
        store.refreshPaletteRecords()
    }

    fun captureDynamicColorAndSave(context: Context) {
        store.captureDynamicColorAndSave(context)
    }

    fun updateColorSource(colorSource: String) {
        store.updateColorSource(colorSource)
    }

    fun updateAppearanceMode(mode: String) {
        store.updateAppearanceMode(mode)
    }

    fun updateSelectedBuiltinThemeId(themeId: String) {
        store.updateSelectedBuiltinThemeId(themeId)
    }

    fun updateSelectedPaletteId(paletteId: String) {
        store.updateSelectedPaletteId(paletteId)
    }

    fun deletePaletteRecord(
        deviceId: String,
        fingerprint: String,
    ) {
        store.deletePaletteRecord(deviceId, fingerprint)
    }

    fun onSystemDarkModeChanged(isDark: Boolean) {
        store.onSystemDarkModeChanged(isDark)
    }
}

/**
 * ON_RESUME 主题刷新逻辑（#609 一 / #618 三）。
 *
 * 刷新不读取 `LocalSujianAppDependencies`：主题控制器在 CompositionLocalProvider
 * 建立之前初始化，反向依赖 CompositionLocal 会导致启动必现崩溃
 * （`No SujianAppDependencies provided`）。抽成纯函数便于 JVM 单测。
 *
 * #618 三：旧实现先按同步状态调用 `ThemeStore.onSyncCompleted()`（其内部就是
 * [ThemeController.reload]），再无条件 [ThemeController.reload] —— 一次 ON_RESUME
 * 执行两次完整主题解析（读盘 + 调色板解析 + theme.resolve 日志）。同步分支是纯
 * 冗余（两者动作完全相同），已与 `onSyncCompleted` 别名一起删除：恢复时只刷新一次。
 * 外部同步把主题目录拉回来的刷新只走 themeCatalogChanged（见 rememberThemeController）。
 */
internal fun handleThemeControllerOnResume(
    controller: ThemeController,
    context: Context,
) {
    controller.reload()
    val uiState = ThemeStore.uiState.value
    if (uiState.colorSource == "android_dynamic" && uiState.dynamicColorEnabled) {
        ThemeStore.captureDynamicColorAndSave(context)
    }
}

/**
 * 系统配置变化（夜间模式）时的主题刷新逻辑，抽成纯函数便于 JVM 单测。
 */
internal fun handleThemeControllerConfigurationChanged(context: Context) {
    val configuration = context.resources?.configuration
    val isDark = (
        configuration?.uiMode?.and(Configuration.UI_MODE_NIGHT_MASK)
            == Configuration.UI_MODE_NIGHT_YES
    )
    ThemeStore.onSystemDarkModeChanged(isDark)
    val uiState = ThemeStore.uiState.value
    if (uiState.colorSource == "android_dynamic" && uiState.dynamicColorEnabled) {
        ThemeStore.captureDynamicColorAndSave(context)
    }
}

@Composable
fun rememberThemeController(
    context: Context,
    settingsRepository: SettingsRepository,
    themeRepository: ThemeRepository,
): ThemeController {
    val controller = remember { ThemeController(settingsRepository, themeRepository) }

    DisposableEffect(Unit) {
        ThemeStore.initialize(themeRepository, settingsRepository)
        onDispose { }
    }

    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer =
            LifecycleEventObserver { _, event ->
                if (event == Lifecycle.Event.ON_RESUME) {
                    handleThemeControllerOnResume(controller, context)
                }
            }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }

    LaunchedEffect(Unit) {
        // #618 三：只监听外部主题目录变化（同步把调色板目录拉回来）才刷新主题；
        // 本机设置保存不再触发主题重载（本机主题字段走 affectsTheme → ThemeStore.reload()）。
        // 旧代码在这里同时调用 ThemeStore.onSyncCompleted() 与 controller.reload()
        // （两者都是完整主题解析），一次事件重复解析两次；现在只刷新一次。
        com.xiwei.sujian.feature.settings.data.CoreSettingsEvents.themeCatalogChanged.collect {
            controller.reload()
        }
    }

    DisposableEffect(Unit) {
        controller.reload()
        onDispose { }
    }

    val configuration = context.resources?.configuration
    DisposableEffect(configuration) {
        handleThemeControllerConfigurationChanged(context)
        onDispose { }
    }

    return controller
}
