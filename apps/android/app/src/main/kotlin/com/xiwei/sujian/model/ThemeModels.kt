package com.xiwei.sujian.model

/**
 * App 层主题 DTO — UI 层通过这些类型间接访问 UniFFI 绑定。
 *
 * 这些类型是 `uniffi.writer_core.*Dto` 的 app 层镜像，UI 层不得直接引用 UniFFI 绑定。
 * 映射在 [com.xiwei.sujian.data.ThemeDtoMapper] 中完成，Repository/Store 层负责转换。
 */

/**
 * 主题配色方案 — 所有颜色字段为 hex 字符串（如 "#FF0000"）。
 *
 * 对应 UniFFI: `uniffi.writer_core.ThemeColorSchemeDto`
 */
data class ThemeColorScheme(
    val primary: String,
    val onPrimary: String,
    val primaryContainer: String,
    val onPrimaryContainer: String,
    val inversePrimary: String,
    val secondary: String,
    val onSecondary: String,
    val secondaryContainer: String,
    val onSecondaryContainer: String,
    val tertiary: String,
    val onTertiary: String,
    val tertiaryContainer: String,
    val onTertiaryContainer: String,
    val background: String,
    val onBackground: String,
    val surface: String,
    val onSurface: String,
    val surfaceVariant: String,
    val onSurfaceVariant: String,
    val surfaceTint: String,
    val surfaceDim: String,
    val surfaceBright: String,
    val surfaceContainerLowest: String,
    val surfaceContainerLow: String,
    val surfaceContainer: String,
    val surfaceContainerHigh: String,
    val surfaceContainerHighest: String,
    val inverseSurface: String,
    val inverseOnSurface: String,
    val error: String,
    val onError: String,
    val errorContainer: String,
    val onErrorContainer: String,
    val outline: String,
    val outlineVariant: String,
    val scrim: String,
    val primaryFixed: String,
    val primaryFixedDim: String,
    val onPrimaryFixed: String,
    val onPrimaryFixedVariant: String,
    val secondaryFixed: String,
    val secondaryFixedDim: String,
    val onSecondaryFixed: String,
    val onSecondaryFixedVariant: String,
    val tertiaryFixed: String,
    val tertiaryFixedDim: String,
    val onTertiaryFixed: String,
    val onTertiaryFixedVariant: String,
)

/**
 * 内置主题 — 包含主题 ID、名称和亮/暗配色方案。
 *
 * 对应 UniFFI: `uniffi.writer_core.BuiltinThemeDto`
 */
data class BuiltinTheme(
    val themeId: String,
    val name: String,
    val lightScheme: ThemeColorScheme,
    val darkScheme: ThemeColorScheme,
)

/**
 * 调色板记录 — 用户保存的调色板快照。
 *
 * 对应 UniFFI: `uniffi.writer_core.ThemePaletteRecordDto`
 */
data class ThemePaletteRecord(
    val schemaVersion: UInt,
    val paletteId: String,
    val paletteFingerprint: String,
    val source: String,
    val sourcePlatform: String,
    val sourceDeviceId: String,
    val sourceDeviceClass: String,
    val capturedAtMs: Long,
    val variant: String,
    val lightScheme: ThemeColorScheme,
    val darkScheme: ThemeColorScheme,
)
