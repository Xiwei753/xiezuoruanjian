// =============================================================================
// ThemePalette.ets — 跨端同步主题调色板
// =============================================================================
//
// 非 Android 端只消费同步 palette，不伪装莫奈。
// 只有同步仓库存在 theme_palette.source = android_dynamic_color 时才使用。
// =============================================================================

import { ThemeColors } from './HarmonyThemeAdapter'

export interface ThemePaletteData {
  source: string
  updated_at_ms: number
  device_id: string
  variant: string
  light_primary: string
  light_on_primary: string
  light_primary_container: string
  light_on_primary_container: string
  light_secondary: string
  light_on_secondary: string
  light_secondary_container: string
  light_on_secondary_container: string
  light_tertiary: string
  light_on_tertiary: string
  light_tertiary_container: string
  light_on_tertiary_container: string
  light_background: string
  light_on_background: string
  light_surface: string
  light_on_surface: string
  light_surface_variant: string
  light_on_surface_variant: string
  light_surface_container: string
  light_surface_container_high: string
  light_outline: string
  light_outline_variant: string
  dark_primary: string
  dark_on_primary: string
  dark_primary_container: string
  dark_on_primary_container: string
  dark_secondary: string
  dark_on_secondary: string
  dark_secondary_container: string
  dark_on_secondary_container: string
  dark_tertiary: string
  dark_on_tertiary: string
  dark_tertiary_container: string
  dark_on_tertiary_container: string
  dark_background: string
  dark_on_background: string
  dark_surface: string
  dark_on_surface: string
  dark_surface_variant: string
  dark_on_surface_variant: string
  dark_surface_container: string
  dark_surface_container_high: string
  dark_outline: string
  dark_outline_variant: string
}

export class ThemePaletteHelper {
  /**
   * Check if a synced theme palette is valid and from Android Dynamic Color.
   * Only when this returns true should the UI show "Use Android synced theme color" toggle.
   */
  static isAndroidDynamicColor(palette: ThemePaletteData | null): boolean {
    return palette !== null && palette.source === 'android_dynamic_color'
  }

  /**
   * Merge palette colors into ThemeColors, only overriding non-empty values.
   * Paper/editor background only uses neutral/surface, never high-saturation primary.
   */
  static mergeIntoThemeColors(colors: ThemeColors, palette: ThemePaletteData, isDark: boolean): ThemeColors {
    const prefix = isDark ? 'dark_' : 'light_'
    const result = { ...colors }

    const merge = (key: keyof ThemeColors, paletteKey: string) => {
      const val = (palette as Record<string, string>)[paletteKey]
      if (val && val.length > 0) {
        (result as Record<string, string>)[key] = val
      }
    }

    merge('primary', prefix + 'primary')
    merge('onPrimary', prefix + 'on_primary')
    merge('primaryContainer', prefix + 'primary_container')
    merge('onPrimaryContainer', prefix + 'on_primary_container')
    merge('secondary', prefix + 'secondary')
    merge('onSecondary', prefix + 'on_secondary')
    merge('secondaryContainer', prefix + 'secondary_container')
    merge('onSecondaryContainer', prefix + 'on_secondary_container')
    merge('tertiary', prefix + 'tertiary')
    merge('onTertiary', prefix + 'on_tertiary')
    merge('tertiaryContainer', prefix + 'tertiary_container')
    merge('onTertiaryContainer', prefix + 'on_tertiary_container')
    merge('background', prefix + 'background')
    merge('onBackground', prefix + 'on_background')
    merge('surface', prefix + 'surface')
    merge('onSurface', prefix + 'on_surface')
    merge('surfaceVariant', prefix + 'surface_variant')
    merge('onSurfaceVariant', prefix + 'on_surface_variant')
    merge('surfaceContainer', prefix + 'surface_container')
    merge('surfaceContainerHigh', prefix + 'surface_container_high')
    merge('outline', prefix + 'outline')
    merge('outlineVariant', prefix + 'outline_variant')

    return result
  }
}
