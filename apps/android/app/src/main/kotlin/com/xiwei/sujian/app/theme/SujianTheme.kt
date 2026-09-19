package com.xiwei.sujian.app.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import com.xiwei.sujian.core.designsystem.theme.ColorSource
import com.xiwei.sujian.core.designsystem.theme.SujianDarkColorScheme
import com.xiwei.sujian.core.designsystem.theme.SujianLightColorScheme
import com.xiwei.sujian.core.designsystem.theme.SujianShapes
import com.xiwei.sujian.core.designsystem.theme.SujianTheme
import com.xiwei.sujian.core.designsystem.theme.SujianTypography
import com.xiwei.sujian.core.designsystem.theme.hexToColor
import com.xiwei.sujian.core.interop.diagnostics.AppDiagnosticsEvents
import com.xiwei.sujian.core.interop.diagnostics.ThemeMaterialColorSnapshot

private fun schemeFromRecord(
    record: com.xiwei.sujian.app.theme.model.ThemePaletteRecord,
    isDark: Boolean,
): androidx.compose.material3.ColorScheme {
    val scheme = if (isDark) record.darkScheme else record.lightScheme
    val base = if (isDark) SujianDarkColorScheme else SujianLightColorScheme
    return base.copy(
        primary = hexToColor(scheme.primary),
        onPrimary = hexToColor(scheme.onPrimary),
        primaryContainer = hexToColor(scheme.primaryContainer),
        onPrimaryContainer = hexToColor(scheme.onPrimaryContainer),
        inversePrimary = hexToColor(scheme.inversePrimary),
        secondary = hexToColor(scheme.secondary),
        onSecondary = hexToColor(scheme.onSecondary),
        secondaryContainer = hexToColor(scheme.secondaryContainer),
        onSecondaryContainer = hexToColor(scheme.onSecondaryContainer),
        tertiary = hexToColor(scheme.tertiary),
        onTertiary = hexToColor(scheme.onTertiary),
        tertiaryContainer = hexToColor(scheme.tertiaryContainer),
        onTertiaryContainer = hexToColor(scheme.onTertiaryContainer),
        background = hexToColor(scheme.background),
        onBackground = hexToColor(scheme.onBackground),
        surface = hexToColor(scheme.surface),
        onSurface = hexToColor(scheme.onSurface),
        surfaceVariant = hexToColor(scheme.surfaceVariant),
        onSurfaceVariant = hexToColor(scheme.onSurfaceVariant),
        surfaceTint = hexToColor(scheme.surfaceTint),
        inverseSurface = hexToColor(scheme.inverseSurface),
        inverseOnSurface = hexToColor(scheme.inverseOnSurface),
        error = hexToColor(scheme.error),
        onError = hexToColor(scheme.onError),
        errorContainer = hexToColor(scheme.errorContainer),
        onErrorContainer = hexToColor(scheme.onErrorContainer),
        outline = hexToColor(scheme.outline),
        outlineVariant = hexToColor(scheme.outlineVariant),
        scrim = hexToColor(scheme.scrim),
        surfaceBright = hexToColor(scheme.surfaceBright),
        surfaceDim = hexToColor(scheme.surfaceDim),
        surfaceContainer = hexToColor(scheme.surfaceContainer),
        surfaceContainerHigh = hexToColor(scheme.surfaceContainerHigh),
        surfaceContainerHighest = hexToColor(scheme.surfaceContainerHighest),
        surfaceContainerLow = hexToColor(scheme.surfaceContainerLow),
        surfaceContainerLowest = hexToColor(scheme.surfaceContainerLowest),
        primaryFixed = hexToColor(scheme.primaryFixed),
        primaryFixedDim = hexToColor(scheme.primaryFixedDim),
        onPrimaryFixed = hexToColor(scheme.onPrimaryFixed),
        onPrimaryFixedVariant = hexToColor(scheme.onPrimaryFixedVariant),
        secondaryFixed = hexToColor(scheme.secondaryFixed),
        secondaryFixedDim = hexToColor(scheme.secondaryFixedDim),
        onSecondaryFixed = hexToColor(scheme.onSecondaryFixed),
        onSecondaryFixedVariant = hexToColor(scheme.onSecondaryFixedVariant),
        tertiaryFixed = hexToColor(scheme.tertiaryFixed),
        tertiaryFixedDim = hexToColor(scheme.tertiaryFixedDim),
        onTertiaryFixed = hexToColor(scheme.onTertiaryFixed),
        onTertiaryFixedVariant = hexToColor(scheme.onTertiaryFixedVariant),
    )
}

private fun schemeFromBuiltin(
    theme: com.xiwei.sujian.app.theme.model.BuiltinTheme,
    isDark: Boolean,
): androidx.compose.material3.ColorScheme {
    val scheme = if (isDark) theme.darkScheme else theme.lightScheme
    val base = if (isDark) SujianDarkColorScheme else SujianLightColorScheme
    return base.copy(
        primary = hexToColor(scheme.primary),
        onPrimary = hexToColor(scheme.onPrimary),
        primaryContainer = hexToColor(scheme.primaryContainer),
        onPrimaryContainer = hexToColor(scheme.onPrimaryContainer),
        inversePrimary = hexToColor(scheme.inversePrimary),
        secondary = hexToColor(scheme.secondary),
        onSecondary = hexToColor(scheme.onSecondary),
        secondaryContainer = hexToColor(scheme.secondaryContainer),
        onSecondaryContainer = hexToColor(scheme.onSecondaryContainer),
        tertiary = hexToColor(scheme.tertiary),
        onTertiary = hexToColor(scheme.onTertiary),
        tertiaryContainer = hexToColor(scheme.tertiaryContainer),
        onTertiaryContainer = hexToColor(scheme.onTertiaryContainer),
        background = hexToColor(scheme.background),
        onBackground = hexToColor(scheme.onBackground),
        surface = hexToColor(scheme.surface),
        onSurface = hexToColor(scheme.onSurface),
        surfaceVariant = hexToColor(scheme.surfaceVariant),
        onSurfaceVariant = hexToColor(scheme.onSurfaceVariant),
        surfaceTint = hexToColor(scheme.surfaceTint),
        inverseSurface = hexToColor(scheme.inverseSurface),
        inverseOnSurface = hexToColor(scheme.inverseOnSurface),
        error = hexToColor(scheme.error),
        onError = hexToColor(scheme.onError),
        errorContainer = hexToColor(scheme.errorContainer),
        onErrorContainer = hexToColor(scheme.onErrorContainer),
        outline = hexToColor(scheme.outline),
        outlineVariant = hexToColor(scheme.outlineVariant),
        scrim = hexToColor(scheme.scrim),
        surfaceBright = hexToColor(scheme.surfaceBright),
        surfaceDim = hexToColor(scheme.surfaceDim),
        surfaceContainer = hexToColor(scheme.surfaceContainer),
        surfaceContainerHigh = hexToColor(scheme.surfaceContainerHigh),
        surfaceContainerHighest = hexToColor(scheme.surfaceContainerHighest),
        surfaceContainerLow = hexToColor(scheme.surfaceContainerLow),
        surfaceContainerLowest = hexToColor(scheme.surfaceContainerLowest),
        primaryFixed = hexToColor(scheme.primaryFixed),
        primaryFixedDim = hexToColor(scheme.primaryFixedDim),
        onPrimaryFixed = hexToColor(scheme.onPrimaryFixed),
        onPrimaryFixedVariant = hexToColor(scheme.onPrimaryFixedVariant),
        secondaryFixed = hexToColor(scheme.secondaryFixed),
        secondaryFixedDim = hexToColor(scheme.secondaryFixedDim),
        onSecondaryFixed = hexToColor(scheme.onSecondaryFixed),
        onSecondaryFixedVariant = hexToColor(scheme.onSecondaryFixedVariant),
        tertiaryFixed = hexToColor(scheme.tertiaryFixed),
        tertiaryFixedDim = hexToColor(scheme.tertiaryFixedDim),
        onTertiaryFixed = hexToColor(scheme.onTertiaryFixed),
        onTertiaryFixedVariant = hexToColor(scheme.onTertiaryFixedVariant),
    )
}

@Composable
fun SujianTheme(
    uiState: ThemeUiState,
    content: @Composable () -> Unit,
) {
    val systemDark = isSystemInDarkTheme()
    val isDark =
        when {
            uiState.isDark -> true
            uiState.isLight -> false
            else -> systemDark
        }
    val context = LocalContext.current

    val colorScheme: ColorScheme =
        when (uiState.resolvedColorSource) {
            ColorSource.ANDROID_DYNAMIC -> {
                // 动态色不与静态主题共用长期缓存 — Issue #698 评论 5697617362。
                // 每次重组都按当前 context + 深浅模式直接从系统拿最新 palette。
                // uiState（含 dynamicColorRevision）变化触发本 Composable 重组，
                // 壁纸颜色变化后能拿到新颜色。
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    if (isDark) {
                        androidx.compose.material3.dynamicDarkColorScheme(context)
                    } else {
                        androidx.compose.material3.dynamicLightColorScheme(context)
                    }
                } else {
                    if (isDark) SujianDarkColorScheme else SujianLightColorScheme
                }
            }
            ColorSource.SAVED_PALETTE ->
                remember(uiState.selectedPaletteRecord, isDark) {
                    val record = uiState.selectedPaletteRecord
                    if (record != null) {
                        schemeFromRecord(record, isDark)
                    } else if (isDark) {
                        SujianDarkColorScheme
                    } else {
                        SujianLightColorScheme
                    }
                }
            ColorSource.BUILT_IN ->
                remember(uiState.selectedBuiltinTheme, isDark) {
                    val builtin = uiState.selectedBuiltinTheme
                    if (builtin != null) {
                        schemeFromBuiltin(builtin, isDark)
                    } else if (isDark) {
                        SujianDarkColorScheme
                    } else {
                        SujianLightColorScheme
                    }
                }
        }

    // 低频颜色诊断 — Issue #698 评论 5697617362。
    // theme.resolve 只证明设置层选择；本事件记录最终塞进 MaterialTheme 的关键颜色，
    // 用于真机判断"设置层正确但 MaterialTheme 还是旧颜色"还是"MaterialTheme 已换但页面写死颜色"。
    // 只在 source/isDark/revision/关键颜色值变化时记录，避免高频噪声。
    val diagnosticSource = uiState.resolvedColorSource
    val diagnosticRevision = uiState.dynamicColorRevision
    LaunchedEffect(
        diagnosticSource,
        isDark,
        diagnosticRevision,
        colorScheme.primary,
        colorScheme.primaryContainer,
        colorScheme.surface,
        colorScheme.surfaceContainer,
        colorScheme.onSurface,
    ) {
        AppDiagnosticsEvents.themeMaterialColors(
            source = diagnosticSource.name.lowercase(),
            isDark = isDark,
            colors =
                ThemeMaterialColorSnapshot(
                    primary = colorToHex(colorScheme.primary),
                    primaryContainer = colorToHex(colorScheme.primaryContainer),
                    surface = colorToHex(colorScheme.surface),
                    surfaceContainer = colorToHex(colorScheme.surfaceContainer),
                    onSurface = colorToHex(colorScheme.onSurface),
                ),
            revision = diagnosticRevision,
        )
    }

    SujianTheme(
        colorScheme = colorScheme,
        typography = SujianTypography,
        shapes = SujianShapes,
        content = content,
    )
}

private fun colorToHex(color: androidx.compose.ui.graphics.Color): String {
    val argb = color.toArgb()
    val alpha = (argb shr 24) and 0xFF
    val red = (argb shr 16) and 0xFF
    val green = (argb shr 8) and 0xFF
    val blue = argb and 0xFF
    return if (alpha == 255) {
        String.format("#%02X%02X%02X", red, green, blue)
    } else {
        String.format("#%02X%02X%02X%02X", alpha, red, green, blue)
    }
}
