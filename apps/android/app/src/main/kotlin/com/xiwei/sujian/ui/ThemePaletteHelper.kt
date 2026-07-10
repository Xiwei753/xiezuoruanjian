package com.xiwei.sujian.ui

import android.content.Context
import android.os.Build
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

object ThemePaletteHelper {

    private const val TAG = "ThemePaletteHelper"

    fun extractDynamicColorSchemes(context: Context): DynamicColorResult? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return null
        return try {
            val lightScheme = androidx.compose.material3.dynamicLightColorScheme(context)
            val darkScheme = androidx.compose.material3.dynamicDarkColorScheme(context)
            val lightDto = colorSchemeToDto(lightScheme)
            val darkDto = colorSchemeToDto(darkScheme)
            DynamicColorResult(lightScheme = lightDto, darkScheme = darkDto)
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "Failed to extract dynamic color schemes", e)
            null
        }
    }

    private fun colorSchemeToDto(
        scheme: androidx.compose.material3.ColorScheme
    ): uniffi.writer_core.ThemeColorSchemeDto {
        return uniffi.writer_core.ThemeColorSchemeDto(
            primary = colorToHex(scheme.primary),
            on_primary = colorToHex(scheme.onPrimary),
            primary_container = colorToHex(scheme.primaryContainer),
            on_primary_container = colorToHex(scheme.onPrimaryContainer),
            inverse_primary = colorToHex(scheme.inversePrimary),
            secondary = colorToHex(scheme.secondary),
            on_secondary = colorToHex(scheme.onSecondary),
            secondary_container = colorToHex(scheme.secondaryContainer),
            on_secondary_container = colorToHex(scheme.onSecondaryContainer),
            tertiary = colorToHex(scheme.tertiary),
            on_tertiary = colorToHex(scheme.onTertiary),
            tertiary_container = colorToHex(scheme.tertiaryContainer),
            on_tertiary_container = colorToHex(scheme.onTertiaryContainer),
            background = colorToHex(scheme.background),
            on_background = colorToHex(scheme.onBackground),
            surface = colorToHex(scheme.surface),
            on_surface = colorToHex(scheme.onSurface),
            surface_variant = colorToHex(scheme.surfaceVariant),
            on_surface_variant = colorToHex(scheme.onSurfaceVariant),
            surface_tint = colorToHex(scheme.surfaceTint),
            surface_dim = colorToHex(scheme.surfaceDim),
            surface_bright = colorToHex(scheme.surfaceBright),
            surface_container_lowest = colorToHex(scheme.surfaceContainerLowest),
            surface_container_low = colorToHex(scheme.surfaceContainerLow),
            surface_container = colorToHex(scheme.surfaceContainer),
            surface_container_high = colorToHex(scheme.surfaceContainerHigh),
            surface_container_highest = colorToHex(scheme.surfaceContainerHighest),
            inverse_surface = colorToHex(scheme.inverseSurface),
            inverse_on_surface = colorToHex(scheme.inverseOnSurface),
            error = colorToHex(scheme.error),
            on_error = colorToHex(scheme.onError),
            error_container = colorToHex(scheme.errorContainer),
            on_error_container = colorToHex(scheme.onErrorContainer),
            outline = colorToHex(scheme.outline),
            outline_variant = colorToHex(scheme.outlineVariant),
            scrim = colorToHex(scheme.scrim),
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
        val lightScheme: uniffi.writer_core.ThemeColorSchemeDto,
        val darkScheme: uniffi.writer_core.ThemeColorSchemeDto,
    )
}
