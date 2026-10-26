package com.xiwei.sujian.app.theme.interop

import com.xiwei.sujian.app.theme.model.BuiltinTheme
import com.xiwei.sujian.app.theme.model.ThemeColorScheme

/**
 * UniFFI 主题 DTO → app 层主题 DTO 映射器。
 *
 * 所有映射逻辑集中在此对象，UI 层只使用 com.xiwei.sujian.app.theme.model 下的主题模型。
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
}
