package com.xiwei.sujian.ui.compose.theme

import android.content.Context
import com.xiwei.sujian.data.CoreSettingsEvents
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.ui.ThemePaletteHelper
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.writer_core.BuiltinThemeDto
import uniffi.writer_core.ThemePaletteRecordDto

object ThemeStore {

    private val _uiState = MutableStateFlow(ThemeUiState())
    val uiState: StateFlow<ThemeUiState> = _uiState.asStateFlow()

    private val _paletteRecords = MutableStateFlow<List<ThemePaletteRecordDto>>(emptyList())
    val paletteRecords: StateFlow<List<ThemePaletteRecordDto>> = _paletteRecords.asStateFlow()

    private var _foldDeviceClass: String? = null

    private var _settingsRepository: SettingsRepository? = null

    fun initialize(settingsRepository: SettingsRepository) {
        _settingsRepository = settingsRepository
    }

    fun setFoldDeviceClass(deviceClass: String) {
        _foldDeviceClass = deviceClass
    }

    fun reload() {
        val repo = _settingsRepository ?: return
        val settings = repo.getLocalSettings()
        val builtinTheme = resolveBuiltinTheme(repo, settings)
        val paletteRecord = resolvePaletteRecord(repo, settings)
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
        val repo = _settingsRepository ?: return
        try {
            _paletteRecords.value = repo.listPaletteRecords()
        } catch (_: Exception) {
            _paletteRecords.value = emptyList()
        }
        val current = _uiState.value
        if (current.colorSource == "saved_palette" && current.selectedPaletteId.isNotEmpty()) {
            val repo2 = _settingsRepository ?: return
            val parts = current.selectedPaletteId.split(":")
            if (parts.size == 2) {
                val record = repo2.loadPaletteRecord(parts[0], parts[1])
                if (record != null) {
                    _uiState.value = current.copy(selectedPaletteRecord = record)
                }
            }
        }
    }

    fun captureDynamicColorAndSave(context: Context) {
        val repo = _settingsRepository ?: return
        val settings = repo.getLocalSettings()
        if (!settings.dynamicColorEnabled) return
        val result = ThemePaletteHelper.extractDynamicColorSchemes(context) ?: return
        val deviceClass = _foldDeviceClass
            ?: repo.detectDeviceClassFromFoldFeature(false, context.resources?.configuration?.smallestScreenWidthDp ?: 0)
        repo.saveDynamicColorPaletteToCatalog(
            lightScheme = result.lightScheme,
            darkScheme = result.darkScheme,
            deviceClass = deviceClass,
        )
        refreshPaletteRecords()
    }

    fun updateColorSource(colorSource: String) {
        val repo = _settingsRepository ?: return
        val dynamicEnabled = colorSource == "android_dynamic"
        val settings = repo.getLocalSettings()
        val newSettings = settings.copy(
            colorSource = colorSource,
            dynamicColorEnabled = dynamicEnabled,
        )
        repo.saveLocalSettings(newSettings)
        reload()
    }

    fun updateAppearanceMode(mode: String) {
        val repo = _settingsRepository ?: return
        val settings = repo.getLocalSettings()
        val newSettings = settings.copy(appearanceMode = mode)
        repo.saveLocalSettings(newSettings)
        reload()
    }

    fun updateSelectedBuiltinThemeId(themeId: String) {
        val repo = _settingsRepository ?: return
        val settings = repo.getLocalSettings()
        val newSettings = settings.copy(
            selectedBuiltinThemeId = themeId,
            colorSource = "built_in",
        )
        repo.saveLocalSettings(newSettings)
        reload()
    }

    fun updateSelectedPaletteId(paletteId: String) {
        val repo = _settingsRepository ?: return
        val settings = repo.getLocalSettings()
        val newSettings = settings.copy(
            selectedPaletteId = paletteId,
            colorSource = "saved_palette",
        )
        repo.saveLocalSettings(newSettings)
        reload()
    }

    fun deletePaletteRecord(deviceId: String, fingerprint: String) {
        val repo = _settingsRepository ?: return
        repo.deletePaletteRecord(deviceId, fingerprint)
        val settings = repo.getLocalSettings()
        val parts = settings.selectedPaletteId.split(":")
        if (parts.size == 2 && parts[0] == deviceId && parts[1] == fingerprint) {
            val newSettings = settings.copy(
                selectedPaletteId = "",
                colorSource = "built_in",
            )
            repo.saveLocalSettings(newSettings)
        }
        refreshPaletteRecords()
        reload()
    }

    fun onSyncCompleted() {
        reload()
    }

    fun onSystemDarkModeChanged() {
        val current = _uiState.value
        if (current.isSystem) {
            _uiState.value = current
        }
    }

    private fun resolveBuiltinTheme(repo: SettingsRepository, settings: LocalSettings): BuiltinThemeDto? {
        if (settings.selectedBuiltinThemeId.isEmpty()) return null
        return try {
            repo.listBuiltinThemes().find { it.themeId == settings.selectedBuiltinThemeId }
        } catch (_: Exception) {
            null
        }
    }

    private fun resolvePaletteRecord(repo: SettingsRepository, settings: LocalSettings): ThemePaletteRecordDto? {
        if (settings.selectedPaletteId.isEmpty()) return null
        return try {
            val parts = settings.selectedPaletteId.split(":")
            if (parts.size == 2) {
                repo.loadPaletteRecord(parts[0], parts[1])
            } else {
                null
            }
        } catch (_: Exception) {
            null
        }
    }
}
