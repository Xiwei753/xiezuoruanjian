package com.xiwei.sujian.designsystem.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode

enum class ColorSource {
    BUILT_IN,
    ANDROID_DYNAMIC,
    SAVED_PALETTE
}

fun hexToColor(hex: String): Color {
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

val SujianLightColorScheme = lightColorScheme(
    primary = SujianLightPrimary,
    onPrimary = SujianLightOnPrimary,
    primaryContainer = SujianLightPrimaryContainer,
    onPrimaryContainer = SujianLightOnPrimaryContainer,
    inversePrimary = SujianLightInversePrimary,
    secondary = SujianLightSecondary,
    onSecondary = SujianLightOnSecondary,
    secondaryContainer = SujianLightSecondaryContainer,
    onSecondaryContainer = SujianLightOnSecondaryContainer,
    tertiary = SujianLightTertiary,
    onTertiary = SujianLightOnTertiary,
    tertiaryContainer = SujianLightTertiaryContainer,
    onTertiaryContainer = SujianLightOnTertiaryContainer,
    background = SujianLightBackground,
    onBackground = SujianLightOnBackground,
    surface = SujianLightSurface,
    onSurface = SujianLightOnSurface,
    surfaceVariant = SujianLightSurfaceVariant,
    onSurfaceVariant = SujianLightOnSurfaceVariant,
    surfaceTint = SujianLightSurfaceTint,
    surfaceDim = SujianLightSurfaceDim,
    surfaceBright = SujianLightSurfaceBright,
    surfaceContainerLowest = SujianLightSurfaceContainerLowest,
    surfaceContainerLow = SujianLightSurfaceContainerLow,
    surfaceContainer = SujianLightSurfaceContainer,
    surfaceContainerHigh = SujianLightSurfaceContainerHigh,
    surfaceContainerHighest = SujianLightSurfaceContainerHighest,
    inverseSurface = SujianLightInverseSurface,
    inverseOnSurface = SujianLightInverseOnSurface,
    error = SujianLightError,
    onError = SujianLightOnError,
    errorContainer = SujianLightErrorContainer,
    onErrorContainer = SujianLightOnErrorContainer,
    outline = SujianLightOutline,
    outlineVariant = SujianLightOutlineVariant,
    scrim = SujianLightScrim,
    primaryFixed = SujianLightPrimaryFixed,
    primaryFixedDim = SujianLightPrimaryFixedDim,
    onPrimaryFixed = SujianLightOnPrimaryFixed,
    onPrimaryFixedVariant = SujianLightOnPrimaryFixedVariant,
    secondaryFixed = SujianLightSecondaryFixed,
    secondaryFixedDim = SujianLightSecondaryFixedDim,
    onSecondaryFixed = SujianLightOnSecondaryFixed,
    onSecondaryFixedVariant = SujianLightOnSecondaryFixedVariant,
    tertiaryFixed = SujianLightTertiaryFixed,
    tertiaryFixedDim = SujianLightTertiaryFixedDim,
    onTertiaryFixed = SujianLightOnTertiaryFixed,
    onTertiaryFixedVariant = SujianLightOnTertiaryFixedVariant,
)

val SujianDarkColorScheme = darkColorScheme(
    primary = SujianDarkPrimary,
    onPrimary = SujianDarkOnPrimary,
    primaryContainer = SujianDarkPrimaryContainer,
    onPrimaryContainer = SujianDarkOnPrimaryContainer,
    inversePrimary = SujianDarkInversePrimary,
    secondary = SujianDarkSecondary,
    onSecondary = SujianDarkOnSecondary,
    secondaryContainer = SujianDarkSecondaryContainer,
    onSecondaryContainer = SujianDarkOnSecondaryContainer,
    tertiary = SujianDarkTertiary,
    onTertiary = SujianDarkOnTertiary,
    tertiaryContainer = SujianDarkTertiaryContainer,
    onTertiaryContainer = SujianDarkOnTertiaryContainer,
    background = SujianDarkBackground,
    onBackground = SujianDarkOnBackground,
    surface = SujianDarkSurface,
    onSurface = SujianDarkOnSurface,
    surfaceVariant = SujianDarkSurfaceVariant,
    onSurfaceVariant = SujianDarkOnSurfaceVariant,
    surfaceTint = SujianDarkSurfaceTint,
    surfaceDim = SujianDarkSurfaceDim,
    surfaceBright = SujianDarkSurfaceBright,
    surfaceContainerLowest = SujianDarkSurfaceContainerLowest,
    surfaceContainerLow = SujianDarkSurfaceContainerLow,
    surfaceContainer = SujianDarkSurfaceContainer,
    surfaceContainerHigh = SujianDarkSurfaceContainerHigh,
    surfaceContainerHighest = SujianDarkSurfaceContainerHighest,
    inverseSurface = SujianDarkInverseSurface,
    inverseOnSurface = SujianDarkInverseOnSurface,
    error = SujianDarkError,
    onError = SujianDarkOnError,
    errorContainer = SujianDarkErrorContainer,
    onErrorContainer = SujianDarkOnErrorContainer,
    outline = SujianDarkOutline,
    outlineVariant = SujianDarkOutlineVariant,
    scrim = SujianDarkScrim,
    primaryFixed = SujianDarkPrimaryFixed,
    primaryFixedDim = SujianDarkPrimaryFixedDim,
    onPrimaryFixed = SujianDarkOnPrimaryFixed,
    onPrimaryFixedVariant = SujianDarkOnPrimaryFixedVariant,
    secondaryFixed = SujianDarkSecondaryFixed,
    secondaryFixedDim = SujianDarkSecondaryFixedDim,
    onSecondaryFixed = SujianDarkOnSecondaryFixed,
    onSecondaryFixedVariant = SujianDarkOnSecondaryFixedVariant,
    tertiaryFixed = SujianDarkTertiaryFixed,
    tertiaryFixedDim = SujianDarkTertiaryFixedDim,
    onTertiaryFixed = SujianDarkOnTertiaryFixed,
    onTertiaryFixedVariant = SujianDarkOnTertiaryFixedVariant,
)

@Composable
fun SujianTheme(
    colorScheme: androidx.compose.material3.ColorScheme,
    typography: androidx.compose.material3.Typography = SujianTypography,
    shapes: androidx.compose.material3.Shapes = SujianShapes,
    dimensions: SujianDimensions = SujianDimensions(),
    motion: SujianMotion? = null,
    elevation: SujianElevation = SujianElevation(),
    content: @Composable () -> Unit
) {
    val isInInspectionMode = LocalInspectionMode.current
    val resolvedMotion = motion ?: if (isInInspectionMode) {
        SujianMotion.reducedMotion()
    } else {
        val context = LocalContext.current
        val accessibilityManager = context.getSystemService(android.content.Context.ACCESSIBILITY_SERVICE)
            as? android.view.accessibility.AccessibilityManager
        val isReducedMotion = accessibilityManager?.isTouchExplorationEnabled == true ||
            android.provider.Settings.System.getFloat(
                context.contentResolver,
                android.provider.Settings.Global.TRANSITION_ANIMATION_SCALE,
                1f
            ) == 0f
        if (isReducedMotion) SujianMotion.reducedMotion() else SujianMotion()
    }
    CompositionLocalProvider(
        LocalSujianDimensions provides dimensions,
        LocalSujianMotion provides resolvedMotion,
        LocalSujianElevation provides elevation,
    ) {
        MaterialTheme(
            colorScheme = colorScheme,
            typography = typography,
            shapes = shapes,
            content = content,
        )
    }
}

@Composable
fun SujianTheme(
    isDark: Boolean = isSystemInDarkTheme(),
    useDynamicColor: Boolean = false,
    content: @Composable () -> Unit
) {
    val context = LocalContext.current
    val colorScheme = if (useDynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        if (isDark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
    } else {
        if (isDark) SujianDarkColorScheme else SujianLightColorScheme
    }

    SujianTheme(
        colorScheme = colorScheme,
        content = content,
    )
}
