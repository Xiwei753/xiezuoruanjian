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
import com.xiwei.sujian.feature.sync.data.SyncStatusRepository
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState
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

    fun onSyncCompleted() {
        store.onSyncCompleted()
    }

    fun onSystemDarkModeChanged(isDark: Boolean) {
        store.onSystemDarkModeChanged(isDark)
    }
}

/**
 * ON_RESUME 主题刷新逻辑（#609 一）。
 *
 * 同步状态必须来自调用方注入的 [syncStatusRepository]，不得读取
 * `LocalSujianAppDependencies`：主题控制器在 CompositionLocalProvider
 * 建立之前初始化，反向依赖 CompositionLocal 会导致启动必现崩溃
 * （`No SujianAppDependencies provided`）。抽成纯函数便于 JVM 单测。
 */
internal fun handleThemeControllerOnResume(
    controller: ThemeController,
    syncStatusRepository: SyncStatusRepository,
    context: Context,
) {
    val syncState = syncStatusRepository.state.value
    if (syncState == SyncIndicatorState.Synced) {
        ThemeStore.onSyncCompleted()
    }
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
    syncStatusRepository: SyncStatusRepository,
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
                    handleThemeControllerOnResume(controller, syncStatusRepository, context)
                }
            }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }

    LaunchedEffect(Unit) {
        com.xiwei.sujian.feature.settings.data.CoreSettingsEvents.settingsChanged.collect {
            ThemeStore.onSyncCompleted()
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
