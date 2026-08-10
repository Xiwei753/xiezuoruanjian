package com.xiwei.sujian.app.theme

import android.content.Context
import com.xiwei.sujian.app.theme.model.BuiltinTheme
import com.xiwei.sujian.app.theme.model.ThemePaletteRecord
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

object ThemeStore {
    private const val COLOR_SOURCE_BUILT_IN = "built_in"
    private const val COLOR_SOURCE_ANDROID_DYNAMIC = "android_dynamic"

    private val _uiState = MutableStateFlow(ThemeUiState())
    val uiState: StateFlow<ThemeUiState> = _uiState.asStateFlow()

    private val _paletteRecords = MutableStateFlow<List<ThemePaletteRecord>>(emptyList())
    val paletteRecords: StateFlow<List<ThemePaletteRecord>> = _paletteRecords.asStateFlow()

    private var _foldDeviceClass: String? = null
    private var _systemIsDark: Boolean = false

    private var _themeRepository: ThemeRepository? = null
    private var _settingsRepository: SettingsRepository? = null

    fun initialize(
        themeRepository: ThemeRepository,
        settingsRepository: SettingsRepository,
    ) {
        _themeRepository = themeRepository
        _settingsRepository = settingsRepository
    }

    fun setFoldDeviceClass(deviceClass: String) {
        _foldDeviceClass = deviceClass
    }

    fun reload() {
        val settingsRepo = _settingsRepository ?: return
        val themeRepo = _themeRepository ?: return
        var settings = settingsRepo.getLocalSettings()

        // 规范化旧异常组合：Android 12+ 上，历史默认 built_in 且未选任何主题/调色板 → 迁移到 android_dynamic
        if (shouldMigrateToDynamicColor(settings)) {
            settings =
                settings.copy(
                    colorSource = COLOR_SOURCE_ANDROID_DYNAMIC,
                    dynamicColorEnabled = true,
                )
            settingsRepo.saveLocalSettings(settings)
        }

        val builtinTheme = resolveBuiltinTheme(themeRepo, settings)
        val paletteRecord = resolvePaletteRecord(themeRepo, settings)
        val sysDark = _systemIsDark
        _uiState.value =
            ThemeUiState(
                appearanceMode = settings.appearanceMode,
                colorSource = settings.colorSource,
                dynamicColorEnabled = settings.dynamicColorEnabled,
                selectedBuiltinThemeId = settings.selectedBuiltinThemeId,
                selectedPaletteId = settings.selectedPaletteId,
                selectedBuiltinTheme = builtinTheme,
                selectedPaletteRecord = paletteRecord,
                systemIsDark = sysDark,
            )
        refreshPaletteRecords()
    }

    fun refreshPaletteRecords() {
        val themeRepo = _themeRepository ?: return
        try {
            _paletteRecords.value = themeRepo.listPaletteRecords()
        } catch (_: Exception) {
            _paletteRecords.value = emptyList()
        }
        val current = _uiState.value
        if (current.colorSource == "saved_palette" && current.selectedPaletteId.isNotEmpty()) {
            val parts = current.selectedPaletteId.split(":")
            if (parts.size == 2) {
                val record = themeRepo.loadPaletteRecord(parts[0], parts[1])
                if (record != null) {
                    _uiState.value = current.copy(selectedPaletteRecord = record)
                }
            }
        }
    }

    fun captureDynamicColorAndSave(context: Context) {
        val settingsRepo = _settingsRepository ?: return
        val themeRepo = _themeRepository ?: return
        val settings = settingsRepo.getLocalSettings()
        if (settings.colorSource != COLOR_SOURCE_ANDROID_DYNAMIC) return
        val result = ThemePaletteHelper.extractDynamicColorSchemes(context) ?: return
        val deviceClass =
            _foldDeviceClass
                ?: themeRepo.detectDeviceClassFromFoldFeature(
                    false,
                    context.resources?.configuration?.smallestScreenWidthDp ?: 0,
                )
        themeRepo.saveDynamicColorPaletteToCatalog(
            lightScheme = result.lightScheme,
            darkScheme = result.darkScheme,
            deviceClass = deviceClass,
        )
        refreshPaletteRecords()
    }

    fun updateColorSource(colorSource: String) {
        val repo = _settingsRepository ?: return
        val settings = repo.getLocalSettings()
        val newSettings =
            settings.copy(
                colorSource = colorSource,
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
        val newSettings =
            settings.copy(
                selectedBuiltinThemeId = themeId,
                colorSource = COLOR_SOURCE_BUILT_IN,
            )
        repo.saveLocalSettings(newSettings)
        reload()
    }

    fun updateSelectedPaletteId(paletteId: String) {
        val repo = _settingsRepository ?: return
        val settings = repo.getLocalSettings()
        val newSettings =
            settings.copy(
                selectedPaletteId = paletteId,
                colorSource = "saved_palette",
            )
        repo.saveLocalSettings(newSettings)
        reload()
    }

    fun deletePaletteRecord(
        deviceId: String,
        fingerprint: String,
    ) {
        val settingsRepo = _settingsRepository ?: return
        val themeRepo = _themeRepository ?: return
        themeRepo.deletePaletteRecord(deviceId, fingerprint)
        val settings = settingsRepo.getLocalSettings()
        val parts = settings.selectedPaletteId.split(":")
        if (parts.size == 2 && parts[0] == deviceId && parts[1] == fingerprint) {
            val newSettings =
                settings.copy(
                    selectedPaletteId = "",
                    colorSource = COLOR_SOURCE_BUILT_IN,
                )
            settingsRepo.saveLocalSettings(newSettings)
        }
        refreshPaletteRecords()
        reload()
    }

    fun onSyncCompleted() {
        reload()
    }

    fun onSystemDarkModeChanged(isDark: Boolean) {
        _systemIsDark = isDark
        val current = _uiState.value
        if (current.isSystem) {
            _uiState.value = current.copy(systemIsDark = isDark)
        }
    }

    private fun shouldMigrateToDynamicColor(settings: LocalSettings): Boolean =
        settings.colorSource == COLOR_SOURCE_BUILT_IN &&
            settings.selectedBuiltinThemeId.isEmpty() &&
            settings.selectedPaletteId.isEmpty() &&
            android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.S

    private fun resolveBuiltinTheme(
        themeRepo: ThemeRepository,
        settings: LocalSettings,
    ): BuiltinTheme? {
        if (settings.selectedBuiltinThemeId.isEmpty()) return null
        return try {
            themeRepo.listBuiltinThemes().find { it.themeId == settings.selectedBuiltinThemeId }
        } catch (_: Exception) {
            null
        }
    }

    private fun resolvePaletteRecord(
        themeRepo: ThemeRepository,
        settings: LocalSettings,
    ): ThemePaletteRecord? {
        if (settings.selectedPaletteId.isEmpty()) return null
        return try {
            val parts = settings.selectedPaletteId.split(":")
            if (parts.size == 2) {
                themeRepo.loadPaletteRecord(parts[0], parts[1])
            } else {
                null
            }
        } catch (_: Exception) {
            null
        }
    }
}
