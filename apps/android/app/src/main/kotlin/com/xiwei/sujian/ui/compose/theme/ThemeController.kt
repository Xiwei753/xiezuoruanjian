package com.xiwei.sujian.ui.compose.theme

import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.remember
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.ui.ThemePaletteHelper
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.writer_core.BuiltinThemeDto
import uniffi.writer_core.ThemePaletteRecordDto

class ThemeController(private val settingsRepository: SettingsRepository) {

    private val _uiState = MutableStateFlow(ThemeUiState())
    val uiState: StateFlow<ThemeUiState> = _uiState.asStateFlow()

    private val _paletteRecords = MutableStateFlow<List<ThemePaletteRecordDto>>(emptyList())
    val paletteRecords: StateFlow<List<ThemePaletteRecordDto>> = _paletteRecords.asStateFlow()

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
        refreshPaletteRecords()
    }

    fun refreshPaletteRecords() {
        try {
            _paletteRecords.value = settingsRepository.listPaletteRecords()
        } catch (_: Exception) {
            _paletteRecords.value = emptyList()
        }
    }

    fun captureDynamicColorAndSave(context: Context) {
        val settings = settingsRepository.getLocalSettings()
        if (!settings.dynamicColorEnabled) return
        val result = ThemePaletteHelper.extractDynamicColorSchemes(context) ?: return
        settingsRepository.saveDynamicColorPaletteToCatalog(
            lightScheme = result.lightScheme,
            darkScheme = result.darkScheme,
        )
        refreshPaletteRecords()
    }

    fun updateColorSource(colorSource: String) {
        val dynamicEnabled = colorSource == "android_dynamic"
        val settings = settingsRepository.getLocalSettings()
        val newSettings = settings.copy(
            colorSource = colorSource,
            dynamicColorEnabled = dynamicEnabled,
        )
        settingsRepository.saveLocalSettings(newSettings)
        reload()
    }

    fun updateAppearanceMode(mode: String) {
        val settings = settingsRepository.getLocalSettings()
        val newSettings = settings.copy(appearanceMode = mode)
        settingsRepository.saveLocalSettings(newSettings)
        reload()
    }

    fun updateSelectedBuiltinThemeId(themeId: String) {
        val settings = settingsRepository.getLocalSettings()
        val newSettings = settings.copy(
            selectedBuiltinThemeId = themeId,
            colorSource = "built_in",
        )
        settingsRepository.saveLocalSettings(newSettings)
        reload()
    }

    fun updateSelectedPaletteId(paletteId: String) {
        val settings = settingsRepository.getLocalSettings()
        val newSettings = settings.copy(
            selectedPaletteId = paletteId,
            colorSource = "saved_palette",
        )
        settingsRepository.saveLocalSettings(newSettings)
        reload()
    }

    fun deletePaletteRecord(deviceId: String, fingerprint: String) {
        settingsRepository.deletePaletteRecord(deviceId, fingerprint)
        val settings = settingsRepository.getLocalSettings()
        val parts = settings.selectedPaletteId.split(":")
        if (parts.size == 2 && parts[0] == deviceId && parts[1] == fingerprint) {
            val newSettings = settings.copy(
                selectedPaletteId = "",
                colorSource = "built_in",
            )
            settingsRepository.saveLocalSettings(newSettings)
        }
        refreshPaletteRecords()
        reload()
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
