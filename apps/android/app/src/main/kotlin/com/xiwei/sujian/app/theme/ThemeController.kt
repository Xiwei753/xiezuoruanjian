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
import com.xiwei.sujian.core.interop.settings.SettingsRepository
import kotlinx.coroutines.flow.StateFlow

class ThemeController(private val settingsRepository: SettingsRepository) {
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

@Composable
fun rememberThemeController(
    context: Context,
    settingsRepository: SettingsRepository,
): ThemeController {
    val controller = remember { ThemeController(settingsRepository) }

    DisposableEffect(Unit) {
        ThemeStore.initialize(settingsRepository)
        onDispose { }
    }

    val lifecycleOwner = LocalLifecycleOwner.current
    val deps = com.xiwei.sujian.app.LocalSujianAppDependencies.current
    DisposableEffect(lifecycleOwner) {
        val observer =
            LifecycleEventObserver { _, event ->
                if (event == Lifecycle.Event.ON_RESUME) {
                    val syncState = deps.syncStatusRepository.state.value
                    if (syncState == com.xiwei.sujian.feature.sync.model.SyncIndicatorState.Synced) {
                        ThemeStore.onSyncCompleted()
                    }
                    controller.reload()
                    val uiState = ThemeStore.uiState.value
                    if (uiState.colorSource == "android_dynamic" && uiState.dynamicColorEnabled) {
                        ThemeStore.captureDynamicColorAndSave(context)
                    }
                }
            }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }

    LaunchedEffect(Unit) {
        com.xiwei.sujian.core.interop.settings.CoreSettingsEvents.settingsChanged.collect {
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
        val isDark = (
            configuration?.uiMode?.and(Configuration.UI_MODE_NIGHT_MASK)
                == Configuration.UI_MODE_NIGHT_YES
        )
        ThemeStore.onSystemDarkModeChanged(isDark)
        val uiState = ThemeStore.uiState.value
        if (uiState.colorSource == "android_dynamic" && uiState.dynamicColorEnabled) {
            ThemeStore.captureDynamicColorAndSave(context)
        }
        onDispose { }
    }

    return controller
}
