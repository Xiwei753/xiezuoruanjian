package com.xiwei.sujian.ui.compose.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

private fun hexToColor(hex: String): Color {
    if (hex.length < 7) return Color.Unspecified
    return try {
        val r = hex.substring(1, 3).toInt(16)
        val g = hex.substring(3, 5).toInt(16)
        val b = hex.substring(5, 7).toInt(16)
        Color(r, g, b)
    } catch (_: Exception) {
        Color.Unspecified
    }
}

private fun schemeFromRecord(
    record: uniffi.writer_core.ThemePaletteRecordDto,
    isDark: Boolean
): androidx.compose.material3.ColorScheme {
    val scheme = if (isDark) record.darkScheme else record.lightScheme
    return androidx.compose.material3.ColorScheme(
        primary = hexToColor(scheme.primary),
        onPrimary = hexToColor(scheme.on_primary),
        primaryContainer = hexToColor(scheme.primary_container),
        onPrimaryContainer = hexToColor(scheme.on_primary_container),
        inversePrimary = hexToColor(scheme.inverse_primary),
        secondary = hexToColor(scheme.secondary),
        onSecondary = hexToColor(scheme.on_secondary),
        secondaryContainer = hexToColor(scheme.secondary_container),
        onSecondaryContainer = hexToColor(scheme.on_secondary_container),
        tertiary = hexToColor(scheme.tertiary),
        onTertiary = hexToColor(scheme.on_tertiary),
        tertiaryContainer = hexToColor(scheme.tertiary_container),
        onTertiaryContainer = hexToColor(scheme.on_tertiary_container),
        background = hexToColor(scheme.background),
        onBackground = hexToColor(scheme.on_background),
        surface = hexToColor(scheme.surface),
        onSurface = hexToColor(scheme.on_surface),
        surfaceVariant = hexToColor(scheme.surface_variant),
        onSurfaceVariant = hexToColor(scheme.on_surface_variant),
        surfaceTint = hexToColor(scheme.surface_tint),
        inverseSurface = hexToColor(scheme.inverse_surface),
        inverseOnSurface = hexToColor(scheme.inverse_on_surface),
        error = hexToColor(scheme.error),
        onError = hexToColor(scheme.on_error),
        errorContainer = hexToColor(scheme.error_container),
        onErrorContainer = hexToColor(scheme.on_error_container),
        outline = hexToColor(scheme.outline),
        outlineVariant = hexToColor(scheme.outline_variant),
        scrim = hexToColor(scheme.scrim),
        surfaceBright = hexToColor(scheme.surface_bright),
        surfaceDim = hexToColor(scheme.surface_dim),
        surfaceContainer = hexToColor(scheme.surface_container),
        surfaceContainerHigh = hexToColor(scheme.surface_container_high),
        surfaceContainerHighest = hexToColor(scheme.surface_container_highest),
        surfaceContainerLow = hexToColor(scheme.surface_container_low),
        surfaceContainerLowest = hexToColor(scheme.surface_container_lowest),
    )
}

private fun schemeFromBuiltin(
    theme: uniffi.writer_core.BuiltinThemeDto,
    isDark: Boolean
): androidx.compose.material3.ColorScheme {
    val scheme = if (isDark) theme.darkScheme else theme.lightScheme
    return androidx.compose.material3.ColorScheme(
        primary = hexToColor(scheme.primary),
        onPrimary = hexToColor(scheme.on_primary),
        primaryContainer = hexToColor(scheme.primary_container),
        onPrimaryContainer = hexToColor(scheme.on_primary_container),
        inversePrimary = hexToColor(scheme.inverse_primary),
        secondary = hexToColor(scheme.secondary),
        onSecondary = hexToColor(scheme.on_secondary),
        secondaryContainer = hexToColor(scheme.secondary_container),
        onSecondaryContainer = hexToColor(scheme.on_secondary_container),
        tertiary = hexToColor(scheme.tertiary),
        onTertiary = hexToColor(scheme.on_tertiary),
        tertiaryContainer = hexToColor(scheme.tertiary_container),
        onTertiaryContainer = hexToColor(scheme.on_tertiary_container),
        background = hexToColor(scheme.background),
        onBackground = hexToColor(scheme.on_background),
        surface = hexToColor(scheme.surface),
        onSurface = hexToColor(scheme.on_surface),
        surfaceVariant = hexToColor(scheme.surface_variant),
        onSurfaceVariant = hexToColor(scheme.on_surface_variant),
        surfaceTint = hexToColor(scheme.surface_tint),
        inverseSurface = hexToColor(scheme.inverse_surface),
        inverseOnSurface = hexToColor(scheme.inverse_on_surface),
        error = hexToColor(scheme.error),
        onError = hexToColor(scheme.on_error),
        errorContainer = hexToColor(scheme.error_container),
        onErrorContainer = hexToColor(scheme.on_error_container),
        outline = hexToColor(scheme.outline),
        outlineVariant = hexToColor(scheme.outline_variant),
        scrim = hexToColor(scheme.scrim),
        surfaceBright = hexToColor(scheme.surface_bright),
        surfaceDim = hexToColor(scheme.surface_dim),
        surfaceContainer = hexToColor(scheme.surface_container),
        surfaceContainerHigh = hexToColor(scheme.surface_container_high),
        surfaceContainerHighest = hexToColor(scheme.surface_container_highest),
        surfaceContainerLow = hexToColor(scheme.surface_container_low),
        surfaceContainerLowest = hexToColor(scheme.surface_container_lowest),
    )
}

private val SujianDefaultLightScheme = lightColorScheme(
    primary = Color(0xFF006493),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFC9E6FF),
    onPrimaryContainer = Color(0xFF001E2F),
    inversePrimary = Color(0xFF87CEFF),
    secondary = Color(0xFF50606E),
    onSecondary = Color(0xFFFFFFFF),
    secondaryContainer = Color(0xFFD3E5F5),
    onSecondaryContainer = Color(0xFF0C1D29),
    tertiary = Color(0xFF65587B),
    onTertiary = Color(0xFFFFFFFF),
    tertiaryContainer = Color(0xFFEBDDFF),
    onTertiaryContainer = Color(0xFF201634),
    background = Color(0xFFF6FAFE),
    onBackground = Color(0xFF171C1F),
    surface = Color(0xFFF6FAFE),
    onSurface = Color(0xFF171C1F),
    surfaceVariant = Color(0xFFDDE3EA),
    onSurfaceVariant = Color(0xFF41484D),
    surfaceTint = Color(0xFF006493),
    surfaceDim = Color(0xFFD7DADE),
    surfaceBright = Color(0xFFF6FAFE),
    surfaceContainerLowest = Color(0xFFFFFFFF),
    surfaceContainerLow = Color(0xFFF0F4F8),
    surfaceContainer = Color(0xFFEBEEF2),
    surfaceContainerHigh = Color(0xFFE5E8EC),
    surfaceContainerHighest = Color(0xFFDFE3E7),
    inverseSurface = Color(0xFF2C3134),
    inverseOnSurface = Color(0xFFECF0F4),
    error = Color(0xFFBA1A1A),
    onError = Color(0xFFFFFFFF),
    errorContainer = Color(0xFFFFDAD6),
    onErrorContainer = Color(0xFF410002),
    outline = Color(0xFF71787D),
    outlineVariant = Color(0xFFC1C7CE),
    scrim = Color(0xFF000000),
)

private val SujianDefaultDarkScheme = darkColorScheme(
    primary = Color(0xFF87CEFF),
    onPrimary = Color(0xFF00344D),
    primaryContainer = Color(0xFF004B6E),
    onPrimaryContainer = Color(0xFFC9E6FF),
    inversePrimary = Color(0xFF006493),
    secondary = Color(0xFFB7C9D8),
    onSecondary = Color(0xFF22323F),
    secondaryContainer = Color(0xFF384956),
    onSecondaryContainer = Color(0xFFD3E5F5),
    tertiary = Color(0xFFCFC0E7),
    onTertiary = Color(0xFF362E4A),
    tertiaryContainer = Color(0xFF4D4462),
    onTertiaryContainer = Color(0xFFEBDDFF),
    background = Color(0xFF0F1417),
    onBackground = Color(0xFFDFE3E7),
    surface = Color(0xFF0F1417),
    onSurface = Color(0xFFDFE3E7),
    surfaceVariant = Color(0xFF41484D),
    onSurfaceVariant = Color(0xFFC1C7CE),
    surfaceTint = Color(0xFF87CEFF),
    surfaceDim = Color(0xFF0F1417),
    surfaceBright = Color(0xFF353A3D),
    surfaceContainerLowest = Color(0xFF0A0F12),
    surfaceContainerLow = Color(0xFF171C1F),
    surfaceContainer = Color(0xFF1C2023),
    surfaceContainerHigh = Color(0xFF262B2E),
    surfaceContainerHighest = Color(0xFF313539),
    inverseSurface = Color(0xFFDFE3E7),
    inverseOnSurface = Color(0xFF2C3134),
    error = Color(0xFFFFB4AB),
    onError = Color(0xFF690005),
    errorContainer = Color(0xFF93000A),
    onErrorContainer = Color(0xFFFFDAD6),
    outline = Color(0xFF8B9198),
    outlineVariant = Color(0xFF41484D),
    scrim = Color(0xFF000000),
)

enum class ColorSource {
    BUILT_IN,
    ANDROID_DYNAMIC,
    SAVED_PALETTE
}

@Composable
fun SujianTheme(
    uiState: ThemeUiState = ThemeUiState(),
    content: @Composable () -> Unit
) {
    val systemDark = isSystemInDarkTheme()
    val isDark = when {
        uiState.isDark -> true
        uiState.isLight -> false
        else -> systemDark
    }
    val context = LocalContext.current

    val colorScheme = remember(uiState, isDark) {
        when (uiState.resolvedColorSource) {
            ColorSource.ANDROID_DYNAMIC -> {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    if (isDark) dynamicDarkColorScheme(context)
                    else dynamicLightColorScheme(context)
                } else {
                    if (isDark) SujianDefaultDarkScheme else SujianDefaultLightScheme
                }
            }
            ColorSource.SAVED_PALETTE -> {
                val record = uiState.selectedPaletteRecord
                if (record != null) {
                    schemeFromRecord(record, isDark)
                } else {
                    if (isDark) SujianDefaultDarkScheme else SujianDefaultLightScheme
                }
            }
            ColorSource.BUILT_IN -> {
                val builtin = uiState.selectedBuiltinTheme
                if (builtin != null) {
                    schemeFromBuiltin(builtin, isDark)
                } else {
                    if (isDark) SujianDefaultDarkScheme else SujianDefaultLightScheme
                }
            }
        }
    }

    MaterialTheme(
        colorScheme = colorScheme,
        content = content
    )
}
