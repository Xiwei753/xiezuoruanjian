// ─── Settings DTOs ───

export interface LocalSettings {
  themeMode?: string
  appearanceMode: string
  colorSource: string
  dynamicColorEnabled: boolean
  selectedBuiltinThemeId: string
  selectedPaletteId: string
  locale?: string
  autoSaveEnabled: boolean
  editorFontSize: number
  editorLineSpacingMultiplier: number
  windowWidth: number
  windowHeight: number
  autoSaveDelayMs: number
  autoIndentEnabled: boolean
  autoIndentWidth: number
  editorTypingAnimationEnabled: boolean
  editorSmoothCursorEnabled: boolean
  editorTypingAnimationDurationMs: number
  editorSmoothCursorDurationMs: number
  aiEnabled: boolean
  statsDeviceId?: string
  linuxQtSidebarWidth: number
  linuxQtEditorWidth: number
}

export interface SyncableSettings {
  fontSize: number
  themeMode: string
  monetColor: string
  themePaletteJson: string
}

export type ThemeMode = 'light' | 'dark' | 'system'

// ─── Theme Palette DTOs ───

export interface ThemeColorSchemeDto {
  primary: string
  on_primary: string
  primary_container: string
  on_primary_container: string
  inverse_primary: string
  secondary: string
  on_secondary: string
  secondary_container: string
  on_secondary_container: string
  tertiary: string
  on_tertiary: string
  tertiary_container: string
  on_tertiary_container: string
  background: string
  on_background: string
  surface: string
  on_surface: string
  surface_variant: string
  on_surface_variant: string
  surface_tint: string
  surface_dim: string
  surface_bright: string
  surface_container_lowest: string
  surface_container_low: string
  surface_container: string
  surface_container_high: string
  surface_container_highest: string
  inverse_surface: string
  inverse_on_surface: string
  error: string
  on_error: string
  error_container: string
  on_error_container: string
  outline: string
  outline_variant: string
  scrim: string
}

export interface ThemePaletteRecordDto {
  schema_version: number
  palette_id: string
  palette_fingerprint: string
  source: string
  source_platform: string
  source_device_id: string
  source_device_class: string
  captured_at_ms: number
  variant: string
  light_scheme: ThemeColorSchemeDto
  dark_scheme: ThemeColorSchemeDto
}

export interface BuiltinThemeDto {
  theme_id: string
  name: string
  light_scheme: ThemeColorSchemeDto
  dark_scheme: ThemeColorSchemeDto
}
