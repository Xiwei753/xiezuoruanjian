package com.xiwei.sujian.ui.compose.theme

import com.xiwei.sujian.designsystem.theme.ColorSource
import com.xiwei.sujian.model.BuiltinTheme
import com.xiwei.sujian.model.ThemePaletteRecord

data class ThemeUiState(
    val appearanceMode: String = "system",
    val colorSource: String = "built_in",
    val dynamicColorEnabled: Boolean = false,
    val selectedBuiltinThemeId: String = "",
    val selectedPaletteId: String = "",
    val selectedBuiltinTheme: BuiltinTheme? = null,
    val selectedPaletteRecord: ThemePaletteRecord? = null,
    val paletteRecords: List<ThemePaletteRecord> = emptyList(),
    val systemIsDark: Boolean = false,
) {
    val isDark: Boolean
        get() =
            when (appearanceMode) {
                "dark" -> true
                "light" -> false
                else -> systemIsDark
            }

    val isLight: Boolean
        get() = appearanceMode == "light"

    val isSystem: Boolean
        get() = appearanceMode != "light" && appearanceMode != "dark"

    val resolvedColorSource: ColorSource
        get() =
            when (colorSource) {
                "android_dynamic" -> ColorSource.ANDROID_DYNAMIC
                "saved_palette" -> {
                    if (selectedPaletteRecord != null) {
                        ColorSource.SAVED_PALETTE
                    } else {
                        ColorSource.BUILT_IN
                    }
                }
                "built_in" -> ColorSource.BUILT_IN
                else -> ColorSource.BUILT_IN
            }
}
