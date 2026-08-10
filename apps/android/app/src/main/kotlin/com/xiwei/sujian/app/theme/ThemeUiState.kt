package com.xiwei.sujian.app.theme

import com.xiwei.sujian.app.theme.model.BuiltinTheme
import com.xiwei.sujian.app.theme.model.ThemePaletteRecord
import com.xiwei.sujian.core.designsystem.theme.ColorSource

data class ThemeUiState(
    val appearanceMode: String = "system",
    val colorSource: String = "built_in",
    /**
     * 旧数据兼容字段。Android 渲染不再以此作为第二事实；
     * 写入时始终由 [colorSource] 派生（android_dynamic → true，其余 → false）。
     */
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
