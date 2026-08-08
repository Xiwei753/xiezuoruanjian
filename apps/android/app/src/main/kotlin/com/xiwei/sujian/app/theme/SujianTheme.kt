package com.xiwei.sujian.app.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import com.xiwei.sujian.core.designsystem.theme.ColorSource
import com.xiwei.sujian.core.designsystem.theme.SujianDarkColorScheme
import com.xiwei.sujian.core.designsystem.theme.SujianLightColorScheme
import com.xiwei.sujian.core.designsystem.theme.SujianShapes
import com.xiwei.sujian.core.designsystem.theme.SujianTheme
import com.xiwei.sujian.core.designsystem.theme.SujianTypography
import com.xiwei.sujian.core.designsystem.theme.hexToColor

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
    uiState: ThemeUiState = ThemeUiState(),
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

    val colorScheme =
        remember(uiState, isDark) {
            when (uiState.resolvedColorSource) {
                ColorSource.ANDROID_DYNAMIC -> {
                    if (uiState.dynamicColorEnabled && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                        if (isDark) {
                            androidx.compose.material3.dynamicDarkColorScheme(context)
                        } else {
                            androidx.compose.material3.dynamicLightColorScheme(context)
                        }
                    } else {
                        if (isDark) SujianDarkColorScheme else SujianLightColorScheme
                    }
                }
                ColorSource.SAVED_PALETTE -> {
                    val record = uiState.selectedPaletteRecord
                    if (record != null) {
                        schemeFromRecord(record, isDark)
                    } else if (isDark) {
                        SujianDarkColorScheme
                    } else {
                        SujianLightColorScheme
                    }
                }
                ColorSource.BUILT_IN -> {
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
        }

    SujianTheme(
        colorScheme = colorScheme,
        typography = SujianTypography,
        shapes = SujianShapes,
        content = content,
    )
}
