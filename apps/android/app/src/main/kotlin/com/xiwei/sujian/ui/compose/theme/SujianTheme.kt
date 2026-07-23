package com.xiwei.sujian.ui.compose.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import com.xiwei.sujian.designsystem.theme.ColorSource
import com.xiwei.sujian.designsystem.theme.SujianDarkColorScheme
import com.xiwei.sujian.designsystem.theme.SujianLightColorScheme
import com.xiwei.sujian.designsystem.theme.SujianTheme
import com.xiwei.sujian.designsystem.theme.hexToColor

private fun schemeFromRecord(
    record: uniffi.writer_core.ThemePaletteRecordDto,
    isDark: Boolean
): androidx.compose.material3.ColorScheme {
    val scheme = if (isDark) record.darkScheme else record.lightScheme
    return androidx.compose.material3.ColorScheme(
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
    )
}

private fun schemeFromBuiltin(
    theme: uniffi.writer_core.BuiltinThemeDto,
    isDark: Boolean
): androidx.compose.material3.ColorScheme {
    val scheme = if (isDark) theme.darkScheme else theme.lightScheme
    return androidx.compose.material3.ColorScheme(
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
    )
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
                if (uiState.dynamicColorEnabled && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    if (isDark) androidx.compose.material3.dynamicDarkColorScheme(context)
                    else androidx.compose.material3.dynamicLightColorScheme(context)
                } else {
                    if (isDark) SujianDarkColorScheme else SujianLightColorScheme
                }
            }
            ColorSource.SAVED_PALETTE -> {
                val record = uiState.selectedPaletteRecord
                if (record != null) schemeFromRecord(record, isDark)
                else if (isDark) SujianDarkColorScheme else SujianLightColorScheme
            }
            ColorSource.BUILT_IN -> {
                val builtin = uiState.selectedBuiltinTheme
                if (builtin != null) schemeFromBuiltin(builtin, isDark)
                else if (isDark) SujianDarkColorScheme else SujianLightColorScheme
            }
        }
    }

    SujianTheme(
        colorScheme = colorScheme,
        content = content,
    )
}
