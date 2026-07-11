package com.xiwei.sujian.ui.compose.theme

import android.content.Context
import android.content.res.Configuration
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.remember
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.xiwei.sujian.data.SettingsRepository
import kotlinx.coroutines.flow.StateFlow

class ThemeController(private val settingsRepository: SettingsRepository) {

    private val store: ThemeStore = ThemeStore

    val uiState: StateFlow<ThemeUiState>
        get() = store.uiState

    val paletteRecords: StateFlow<List<uniffi.writer_core.ThemePaletteRecordDto>>
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

    fun deletePaletteRecord(deviceId: String, fingerprint: String) {
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
fun rememberThemeController(context: Context): ThemeController {
    val settingsRepository = remember { SettingsRepository(context) }
    val controller = remember { ThemeController(settingsRepository) }

    DisposableEffect(Unit) {
        ThemeStore.initialize(settingsRepository)
        onDispose { }
    }

    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                if (com.xiwei.sujian.data.CoreSettingsEvents.consumeChanged()) {
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

    DisposableEffect(Unit) {
        controller.reload()
        onDispose { }
    }

    val configuration = context.resources?.configuration
    DisposableEffect(configuration) {
        val isDark = (configuration?.uiMode?.and(Configuration.UI_MODE_NIGHT_MASK)
            == Configuration.UI_MODE_NIGHT_YES)
        ThemeStore.onSystemDarkModeChanged(isDark)
        val uiState = ThemeStore.uiState.value
        if (uiState.colorSource == "android_dynamic" && uiState.dynamicColorEnabled) {
            ThemeStore.captureDynamicColorAndSave(context)
        }
        onDispose { }
    }

    return controller
}
