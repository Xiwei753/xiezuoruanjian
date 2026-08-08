package com.xiwei.sujian.core.interop.common

import com.xiwei.sujian.app.theme.model.BuiltinTheme
import com.xiwei.sujian.app.theme.model.ThemeColorScheme
import com.xiwei.sujian.app.theme.model.ThemePaletteRecord

/**
 * UniFFI 主题 DTO → app 层主题 DTO 映射器。
 *
 * 所有映射逻辑集中在此对象，UI 层只使用 [com.xiwei.sujian.model] 下的类型。
 * Repository/Store 层调用这些方法完成转换，UI 层不直接接触 UniFFI 绑定。
 */
object ThemeDtoMapper {
    fun fromDto(dto: uniffi.writer_core.ThemeColorSchemeDto): ThemeColorScheme =
        ThemeColorScheme(
            primary = dto.primary,
            onPrimary = dto.onPrimary,
            primaryContainer = dto.primaryContainer,
            onPrimaryContainer = dto.onPrimaryContainer,
            inversePrimary = dto.inversePrimary,
            secondary = dto.secondary,
            onSecondary = dto.onSecondary,
            secondaryContainer = dto.secondaryContainer,
            onSecondaryContainer = dto.onSecondaryContainer,
            tertiary = dto.tertiary,
            onTertiary = dto.onTertiary,
            tertiaryContainer = dto.tertiaryContainer,
            onTertiaryContainer = dto.onTertiaryContainer,
            background = dto.background,
            onBackground = dto.onBackground,
            surface = dto.surface,
            onSurface = dto.onSurface,
            surfaceVariant = dto.surfaceVariant,
            onSurfaceVariant = dto.onSurfaceVariant,
            surfaceTint = dto.surfaceTint,
            surfaceDim = dto.surfaceDim,
            surfaceBright = dto.surfaceBright,
            surfaceContainerLowest = dto.surfaceContainerLowest,
            surfaceContainerLow = dto.surfaceContainerLow,
            surfaceContainer = dto.surfaceContainer,
            surfaceContainerHigh = dto.surfaceContainerHigh,
            surfaceContainerHighest = dto.surfaceContainerHighest,
            inverseSurface = dto.inverseSurface,
            inverseOnSurface = dto.inverseOnSurface,
            error = dto.error,
            onError = dto.onError,
            errorContainer = dto.errorContainer,
            onErrorContainer = dto.onErrorContainer,
            outline = dto.outline,
            outlineVariant = dto.outlineVariant,
            scrim = dto.scrim,
            primaryFixed = dto.primaryFixed,
            primaryFixedDim = dto.primaryFixedDim,
            onPrimaryFixed = dto.onPrimaryFixed,
            onPrimaryFixedVariant = dto.onPrimaryFixedVariant,
            secondaryFixed = dto.secondaryFixed,
            secondaryFixedDim = dto.secondaryFixedDim,
            onSecondaryFixed = dto.onSecondaryFixed,
            onSecondaryFixedVariant = dto.onSecondaryFixedVariant,
            tertiaryFixed = dto.tertiaryFixed,
            tertiaryFixedDim = dto.tertiaryFixedDim,
            onTertiaryFixed = dto.onTertiaryFixed,
            onTertiaryFixedVariant = dto.onTertiaryFixedVariant,
        )

    fun fromDto(dto: uniffi.writer_core.BuiltinThemeDto): BuiltinTheme =
        BuiltinTheme(
            themeId = dto.themeId,
            name = dto.name,
            lightScheme = fromDto(dto.lightScheme),
            darkScheme = fromDto(dto.darkScheme),
        )

    fun fromDto(dto: uniffi.writer_core.ThemePaletteRecordDto): ThemePaletteRecord =
        ThemePaletteRecord(
            schemaVersion = dto.schemaVersion,
            paletteId = dto.paletteId,
            paletteFingerprint = dto.paletteFingerprint,
            source = dto.source,
            sourcePlatform = dto.sourcePlatform,
            sourceDeviceId = dto.sourceDeviceId,
            sourceDeviceClass = dto.sourceDeviceClass,
            capturedAtMs = dto.capturedAtMs,
            variant = dto.variant,
            lightScheme = fromDto(dto.lightScheme),
            darkScheme = fromDto(dto.darkScheme),
        )

    fun toDto(scheme: ThemeColorScheme): uniffi.writer_core.ThemeColorSchemeDto =
        uniffi.writer_core.ThemeColorSchemeDto(
            primary = scheme.primary,
            onPrimary = scheme.onPrimary,
            primaryContainer = scheme.primaryContainer,
            onPrimaryContainer = scheme.onPrimaryContainer,
            inversePrimary = scheme.inversePrimary,
            secondary = scheme.secondary,
            onSecondary = scheme.onSecondary,
            secondaryContainer = scheme.secondaryContainer,
            onSecondaryContainer = scheme.onSecondaryContainer,
            tertiary = scheme.tertiary,
            onTertiary = scheme.onTertiary,
            tertiaryContainer = scheme.tertiaryContainer,
            onTertiaryContainer = scheme.onTertiaryContainer,
            background = scheme.background,
            onBackground = scheme.onBackground,
            surface = scheme.surface,
            onSurface = scheme.onSurface,
            surfaceVariant = scheme.surfaceVariant,
            onSurfaceVariant = scheme.onSurfaceVariant,
            surfaceTint = scheme.surfaceTint,
            surfaceDim = scheme.surfaceDim,
            surfaceBright = scheme.surfaceBright,
            surfaceContainerLowest = scheme.surfaceContainerLowest,
            surfaceContainerLow = scheme.surfaceContainerLow,
            surfaceContainer = scheme.surfaceContainer,
            surfaceContainerHigh = scheme.surfaceContainerHigh,
            surfaceContainerHighest = scheme.surfaceContainerHighest,
            inverseSurface = scheme.inverseSurface,
            inverseOnSurface = scheme.inverseOnSurface,
            error = scheme.error,
            onError = scheme.onError,
            errorContainer = scheme.errorContainer,
            onErrorContainer = scheme.onErrorContainer,
            outline = scheme.outline,
            outlineVariant = scheme.outlineVariant,
            scrim = scheme.scrim,
            primaryFixed = scheme.primaryFixed,
            primaryFixedDim = scheme.primaryFixedDim,
            onPrimaryFixed = scheme.onPrimaryFixed,
            onPrimaryFixedVariant = scheme.onPrimaryFixedVariant,
            secondaryFixed = scheme.secondaryFixed,
            secondaryFixedDim = scheme.secondaryFixedDim,
            onSecondaryFixed = scheme.onSecondaryFixed,
            onSecondaryFixedVariant = scheme.onSecondaryFixedVariant,
            tertiaryFixed = scheme.tertiaryFixed,
            tertiaryFixedDim = scheme.tertiaryFixedDim,
            onTertiaryFixed = scheme.onTertiaryFixed,
            onTertiaryFixedVariant = scheme.onTertiaryFixedVariant,
        )
}
