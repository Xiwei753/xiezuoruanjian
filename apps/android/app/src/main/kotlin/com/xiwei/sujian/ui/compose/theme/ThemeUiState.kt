package com.xiwei.sujian.ui.compose.theme

import uniffi.writer_core.BuiltinThemeDto
import uniffi.writer_core.ThemePaletteRecordDto

data class ThemeUiState(
    val appearanceMode: String = "system",
    val colorSource: String = "built_in",
    val dynamicColorEnabled: Boolean = false,
    val selectedBuiltinThemeId: String = "",
    val selectedPaletteId: String = "",
    val selectedBuiltinTheme: BuiltinThemeDto? = null,
    val selectedPaletteRecord: ThemePaletteRecordDto? = null,
) {
    val isDark: Boolean
        get() = appearanceMode == "dark"

    val isLight: Boolean
        get() = appearanceMode == "light"

    val isSystem: Boolean
        get() = appearanceMode != "light" && appearanceMode != "dark"

    val resolvedColorSource: ColorSource
        get() = when (colorSource) {
            "android_dynamic" -> ColorSource.ANDROID_DYNAMIC
            "saved_palette" -> {
                if (selectedPaletteRecord != null) ColorSource.SAVED_PALETTE
                else ColorSource.BUILT_IN
            }
            "built_in" -> ColorSource.BUILT_IN
            else -> ColorSource.BUILT_IN
        }
}
