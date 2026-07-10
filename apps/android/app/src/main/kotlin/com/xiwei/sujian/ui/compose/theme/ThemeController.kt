package com.xiwei.sujian.ui.compose.theme

import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.model.LocalSettings
import uniffi.writer_core.BuiltinThemeDto
import uniffi.writer_core.ThemePaletteRecordDto

class ThemeController(private val settingsRepository: SettingsRepository) {

    private val _uiState = mutableStateOf(ThemeUiState())
    val uiState: State<ThemeUiState> = _uiState

    fun reload() {
        val settings = settingsRepository.getLocalSettings()
        val builtinTheme = resolveBuiltinTheme(settings)
        val paletteRecord = resolvePaletteRecord(settings)
        _uiState.value = ThemeUiState(
            appearanceMode = settings.appearanceMode,
            colorSource = settings.colorSource,
            dynamicColorEnabled = settings.dynamicColorEnabled,
            selectedBuiltinThemeId = settings.selectedBuiltinThemeId,
            selectedPaletteId = settings.selectedPaletteId,
            selectedBuiltinTheme = builtinTheme,
            selectedPaletteRecord = paletteRecord,
        )
    }

    private fun resolveBuiltinTheme(settings: LocalSettings): BuiltinThemeDto? {
        if (settings.selectedBuiltinThemeId.isEmpty()) return null
        return try {
            settingsRepository.listBuiltinThemes().find { it.themeId == settings.selectedBuiltinThemeId }
        } catch (_: Exception) {
            null
        }
    }

    private fun resolvePaletteRecord(settings: LocalSettings): ThemePaletteRecordDto? {
        if (settings.selectedPaletteId.isEmpty()) return null
        return try {
            val parts = settings.selectedPaletteId.split(":")
            if (parts.size == 2) {
                settingsRepository.loadPaletteRecord(parts[0], parts[1])
            } else {
                null
            }
        } catch (_: Exception) {
            null
        }
    }
}

@Composable
fun rememberThemeController(context: Context): ThemeController {
    val settingsRepository = remember { SettingsRepository(context) }
    val controller = remember { ThemeController(settingsRepository) }

    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                controller.reload()
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

    return controller
}
