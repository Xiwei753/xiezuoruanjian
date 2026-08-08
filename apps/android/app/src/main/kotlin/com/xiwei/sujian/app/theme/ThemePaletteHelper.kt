package com.xiwei.sujian.app.theme

import android.content.Context
import android.os.Build
import com.xiwei.sujian.app.theme.model.ThemeColorScheme
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

object ThemePaletteHelper {
    private const val TAG = "ThemePaletteHelper"

    fun extractDynamicColorSchemes(context: Context): DynamicColorResult? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return null
        return try {
            val lightScheme = androidx.compose.material3.dynamicLightColorScheme(context)
            val darkScheme = androidx.compose.material3.dynamicDarkColorScheme(context)
            val lightDto = colorSchemeToAppDto(lightScheme)
            val darkDto = colorSchemeToAppDto(darkScheme)
            DynamicColorResult(lightScheme = lightDto, darkScheme = darkDto)
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "Failed to extract dynamic color schemes", e)
            null
        }
    }

    private fun colorSchemeToAppDto(scheme: androidx.compose.material3.ColorScheme): ThemeColorScheme {
        return ThemeColorScheme(
            primary = colorToHex(scheme.primary),
            onPrimary = colorToHex(scheme.onPrimary),
            primaryContainer = colorToHex(scheme.primaryContainer),
            onPrimaryContainer = colorToHex(scheme.onPrimaryContainer),
            inversePrimary = colorToHex(scheme.inversePrimary),
            secondary = colorToHex(scheme.secondary),
            onSecondary = colorToHex(scheme.onSecondary),
            secondaryContainer = colorToHex(scheme.secondaryContainer),
            onSecondaryContainer = colorToHex(scheme.onSecondaryContainer),
            tertiary = colorToHex(scheme.tertiary),
            onTertiary = colorToHex(scheme.onTertiary),
            tertiaryContainer = colorToHex(scheme.tertiaryContainer),
            onTertiaryContainer = colorToHex(scheme.onTertiaryContainer),
            background = colorToHex(scheme.background),
            onBackground = colorToHex(scheme.onBackground),
            surface = colorToHex(scheme.surface),
            onSurface = colorToHex(scheme.onSurface),
            surfaceVariant = colorToHex(scheme.surfaceVariant),
            onSurfaceVariant = colorToHex(scheme.onSurfaceVariant),
            surfaceTint = colorToHex(scheme.surfaceTint),
            surfaceDim = colorToHex(scheme.surfaceDim),
            surfaceBright = colorToHex(scheme.surfaceBright),
            surfaceContainerLowest = colorToHex(scheme.surfaceContainerLowest),
            surfaceContainerLow = colorToHex(scheme.surfaceContainerLow),
            surfaceContainer = colorToHex(scheme.surfaceContainer),
            surfaceContainerHigh = colorToHex(scheme.surfaceContainerHigh),
            surfaceContainerHighest = colorToHex(scheme.surfaceContainerHighest),
            inverseSurface = colorToHex(scheme.inverseSurface),
            inverseOnSurface = colorToHex(scheme.inverseOnSurface),
            error = colorToHex(scheme.error),
            onError = colorToHex(scheme.onError),
            errorContainer = colorToHex(scheme.errorContainer),
            onErrorContainer = colorToHex(scheme.onErrorContainer),
            outline = colorToHex(scheme.outline),
            outlineVariant = colorToHex(scheme.outlineVariant),
            scrim = colorToHex(scheme.scrim),
            primaryFixed = colorToHex(scheme.primaryFixed),
            primaryFixedDim = colorToHex(scheme.primaryFixedDim),
            onPrimaryFixed = colorToHex(scheme.onPrimaryFixed),
            onPrimaryFixedVariant = colorToHex(scheme.onPrimaryFixedVariant),
            secondaryFixed = colorToHex(scheme.secondaryFixed),
            secondaryFixedDim = colorToHex(scheme.secondaryFixedDim),
            onSecondaryFixed = colorToHex(scheme.onSecondaryFixed),
            onSecondaryFixedVariant = colorToHex(scheme.onSecondaryFixedVariant),
            tertiaryFixed = colorToHex(scheme.tertiaryFixed),
            tertiaryFixedDim = colorToHex(scheme.tertiaryFixedDim),
            onTertiaryFixed = colorToHex(scheme.onTertiaryFixed),
            onTertiaryFixedVariant = colorToHex(scheme.onTertiaryFixedVariant),
        )
    }

    private fun colorToHex(color: androidx.compose.ui.graphics.Color): String {
        val alpha = (color.alpha * 255).toInt()
        val red = (color.red * 255).toInt()
        val green = (color.green * 255).toInt()
        val blue = (color.blue * 255).toInt()
        return if (alpha == 255) {
            String.format("#%02X%02X%02X", red, green, blue)
        } else {
            String.format("#%02X%02X%02X%02X", alpha, red, green, blue)
        }
    }

    data class DynamicColorResult(
        val lightScheme: ThemeColorScheme,
        val darkScheme: ThemeColorScheme,
    )
}
