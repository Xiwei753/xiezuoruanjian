import window from '@ohos.window'
import { ThemePaletteRecordDto, ThemeColorSchemeDto, BuiltinThemeDto } from '../model/CoreDtos'

export type ThemeMode = 'light' | 'dark' | 'system'

export type ColorSource = 'built_in' | 'saved_palette'

export interface ThemeColors {
  // Primary
  primary: string
  onPrimary: string
  primaryContainer: string
  onPrimaryContainer: string
  // Secondary
  secondary: string
  onSecondary: string
  secondaryContainer: string
  onSecondaryContainer: string
  // Tertiary
  tertiary: string
  onTertiary: string
  tertiaryContainer: string
  onTertiaryContainer: string
  // Error
  error: string
  onError: string
  errorContainer: string
  onErrorContainer: string
  // Background/Surface
  background: string
  onBackground: string
  surface: string
  onSurface: string
  surfaceVariant: string
  onSurfaceVariant: string
  // Surface containers
  surfaceContainerLowest: string
  surfaceContainerLow: string
  surfaceContainer: string
  surfaceContainerHigh: string
  surfaceContainerHighest: string
  surfaceDim: string
  surfaceBright: string
  // Outline
  outline: string
  outlineVariant: string
  // Inverse
  inverseSurface: string
  inverseOnSurface: string
  inversePrimary: string
  // Scrim
  scrim: string
  // Legacy compat
  textPrimary: string
  textSecondary: string
  textHint: string
  success: string
  divider: string
  overlay: string
}

export interface ThemeConfig {
  mode: ThemeMode
  colorSource: ColorSource
  selectedPaletteId: string
  selectedBuiltinThemeId: string
  colors: ThemeColors
  fontSize: number
  fontFamily: string
  lineHeight: number
}

export const LightTheme: ThemeColors = {
  primary: '#006497',
  onPrimary: '#FFFFFF',
  primaryContainer: '#CCE5FF',
  onPrimaryContainer: '#001E31',
  secondary: '#51606F',
  onSecondary: '#FFFFFF',
  secondaryContainer: '#D4E4F6',
  onSecondaryContainer: '#0E1D2A',
  tertiary: '#6D578C',
  onTertiary: '#FFFFFF',
  tertiaryContainer: '#F1DAFF',
  onTertiaryContainer: '#261447',
  error: '#BA1A1A',
  onError: '#FFFFFF',
  errorContainer: '#FFDAD6',
  onErrorContainer: '#410002',
  background: '#FCFCFF',
  onBackground: '#1A1C1E',
  surface: '#FCFCFF',
  onSurface: '#1A1C1E',
  surfaceVariant: '#DFE3EB',
  onSurfaceVariant: '#42474E',
  surfaceContainerLowest: '#FFFFFF',
  surfaceContainerLow: '#F1F3F9',
  surfaceContainer: '#EBEDF4',
  surfaceContainerHigh: '#E5E7EF',
  surfaceContainerHighest: '#E0E2E9',
  surfaceDim: '#D7D9DF',
  surfaceBright: '#FCFCFF',
  outline: '#72777F',
  outlineVariant: '#C2C6CF',
  inverseSurface: '#2E3036',
  inverseOnSurface: '#F0F0F7',
  inversePrimary: '#92CCFF',
  scrim: '#000000',
  // Legacy compat
  textPrimary: '#1A1C1E',
  textSecondary: '#42474E',
  textHint: '#72777F',
  success: '#2ECC71',
  divider: '#C2C6CF',
  overlay: '#80000000'
}

export const DarkTheme: ThemeColors = {
  primary: '#92CCFF',
  onPrimary: '#003351',
  primaryContainer: '#004B73',
  onPrimaryContainer: '#CCE5FF',
  secondary: '#B8C8DA',
  onSecondary: '#233240',
  secondaryContainer: '#394857',
  onSecondaryContainer: '#D4E4F6',
  tertiary: '#D7BFFF',
  onTertiary: '#3E2A5C',
  tertiaryContainer: '#554074',
  onTertiaryContainer: '#F1DAFF',
  error: '#FFB4AB',
  onError: '#690005',
  errorContainer: '#93000A',
  onErrorContainer: '#FFDAD6',
  background: '#1A1C1E',
  onBackground: '#E2E2E5',
  surface: '#1A1C1E',
  onSurface: '#E2E2E5',
  surfaceVariant: '#42474E',
  onSurfaceVariant: '#C3C6CF',
  surfaceContainerLowest: '#0D0E13',
  surfaceContainerLow: '#1A1C20',
  surfaceContainer: '#1E2024',
  surfaceContainerHigh: '#282A2F',
  surfaceContainerHighest: '#33353A',
  surfaceDim: '#121418',
  surfaceBright: '#38393F',
  outline: '#8C919A',
  outlineVariant: '#42474E',
  inverseSurface: '#E0E2E9',
  inverseOnSurface: '#2E3036',
  inversePrimary: '#006497',
  scrim: '#000000',
  // Legacy compat
  textPrimary: '#E2E2E5',
  textSecondary: '#C3C6CF',
  textHint: '#8C919A',
  success: '#4ADE80',
  divider: '#42474E',
  overlay: '#80000000'
}

export interface ThemeObserver {
  onThemeChanged(theme: ThemeConfig): void
}

export interface ThemeUpdateParams {
  theme: ThemeMode
  colorSource?: ColorSource
  selectedPaletteId?: string
  selectedBuiltinThemeId?: string
  fontSize: number
  fontFamily: string
  lineHeight: number
}

export class HarmonyThemeAdapter {
  private static instance: HarmonyThemeAdapter
  private mainWindow: window.Window | null = null
  private systemColorMode: number = 0 // 0=light, 1=dark (ConfigurationConstant.ColorMode)
  private currentTheme: ThemeConfig = {
    mode: 'system',
    colorSource: 'built_in',
    selectedPaletteId: '',
    selectedBuiltinThemeId: '',
    colors: LightTheme,
    fontSize: 16,
    fontFamily: 'HarmonyOS Sans',
    lineHeight: 1.8
  }
  private observers: Set<ThemeObserver> = new Set()
  private systemThemeListener: ((isDark: boolean) => void) | null = null
  private paletteRecords: ThemePaletteRecordDto[] = []
  private builtinThemes: BuiltinThemeDto[] = []

  static getInstance(): HarmonyThemeAdapter {
    if (!HarmonyThemeAdapter.instance) {
      HarmonyThemeAdapter.instance = new HarmonyThemeAdapter()
    }
    return HarmonyThemeAdapter.instance
  }

  setMainWindow(win: window.Window): void {
    this.mainWindow = win
  }

  getCurrentTheme(): ThemeConfig {
    const c: ThemeConfig = {
      mode: this.currentTheme.mode,
      colorSource: this.currentTheme.colorSource,
      selectedPaletteId: this.currentTheme.selectedPaletteId,
      selectedBuiltinThemeId: this.currentTheme.selectedBuiltinThemeId,
      colors: this.currentTheme.colors,
      fontSize: this.currentTheme.fontSize,
      fontFamily: this.currentTheme.fontFamily,
      lineHeight: this.currentTheme.lineHeight
    }
    return c
  }

  getColors(): ThemeColors {
    return this.currentTheme.colors
  }

  getMode(): ThemeMode {
    return this.currentTheme.mode
  }

  getColorSource(): ColorSource {
    return this.currentTheme.colorSource
  }

  getSelectedPaletteId(): string {
    return this.currentTheme.selectedPaletteId
  }

  getSelectedBuiltinThemeId(): string {
    return this.currentTheme.selectedBuiltinThemeId
  }

  getPaletteRecords(): ThemePaletteRecordDto[] {
    return this.paletteRecords
  }

  getBuiltinThemes(): BuiltinThemeDto[] {
    return this.builtinThemes
  }

  setMode(mode: ThemeMode): void {
    this.currentTheme.mode = mode
    this.updateColors()
    this.notifyObservers()
  }

  setColorSource(colorSource: ColorSource): void {
    this.currentTheme.colorSource = colorSource
    this.updateColors()
    this.notifyObservers()
  }

  setSelectedPaletteId(paletteId: string): void {
    this.currentTheme.selectedPaletteId = paletteId
    if (paletteId.length > 0) {
      this.currentTheme.colorSource = 'saved_palette'
    }
    this.updateColors()
    this.notifyObservers()
  }

  setSelectedBuiltinThemeId(themeId: string): void {
    this.currentTheme.selectedBuiltinThemeId = themeId
    if (themeId.length > 0) {
      this.currentTheme.colorSource = 'built_in'
    }
    this.updateColors()
    this.notifyObservers()
  }

  setPaletteRecords(records: ThemePaletteRecordDto[]): void {
    this.paletteRecords = records
  }

  setBuiltinThemes(themes: BuiltinThemeDto[]): void {
    this.builtinThemes = themes
  }

  setFontSize(fontSize: number): void {
    this.currentTheme.fontSize = Math.max(12, Math.min(72, fontSize))
    this.notifyObservers()
  }

  setFontFamily(fontFamily: string): void {
    this.currentTheme.fontFamily = fontFamily
    this.notifyObservers()
  }

  setLineHeight(lineHeight: number): void {
    this.currentTheme.lineHeight = Math.max(1.0, Math.min(3.0, lineHeight))
    this.notifyObservers()
  }

  updateFromSettings(settings: ThemeUpdateParams): void {
    this.currentTheme.mode = settings.theme
    this.currentTheme.fontSize = settings.fontSize
    this.currentTheme.fontFamily = settings.fontFamily
    this.currentTheme.lineHeight = settings.lineHeight
    this.updateColors()
    this.notifyObservers()
  }

  updateFromLocalSettings(appearanceMode: string, colorSource: string, selectedPaletteId: string, selectedBuiltinThemeId: string): void {
    this.currentTheme.mode = appearanceMode as ThemeMode
    this.currentTheme.colorSource = colorSource as ColorSource
    this.currentTheme.selectedPaletteId = selectedPaletteId
    this.currentTheme.selectedBuiltinThemeId = selectedBuiltinThemeId
    this.updateColors()
    this.notifyObservers()
  }

  addObserver(observer: ThemeObserver): void {
    this.observers.add(observer)
  }

  removeObserver(observer: ThemeObserver): void {
    this.observers.delete(observer)
  }

  onConfigurationUpdate(colorMode: number): void {
    this.systemColorMode = colorMode
    this.applySystemColorMode(colorMode)
    if (this.currentTheme.mode === 'system') {
      this.notifyObservers()
    }
  }

  startSystemThemeListener(): void {
    this.applySystemColorMode(this.systemColorMode)
  }

  private applySystemColorMode(colorMode: number): void {
    if (colorMode === 1) {
      this.currentTheme.colors = DarkTheme
    } else {
      this.currentTheme.colors = LightTheme
    }
  }

  stopSystemThemeListener(): void {
    this.systemThemeListener = null
  }

  isDarkMode(): boolean {
    return this.currentTheme.colors === DarkTheme
  }

  private isCurrentlyDark(): boolean {
    if (this.currentTheme.mode === 'dark') return true
    if (this.currentTheme.mode === 'light') return false
    return this.systemColorMode === 1
  }

  private updateColors(): void {
    const isDark = this.isCurrentlyDark()

    if (this.currentTheme.colorSource === 'saved_palette' && this.currentTheme.selectedPaletteId.length > 0) {
      const record = this.findPaletteRecord(this.currentTheme.selectedPaletteId)
      if (record !== null) {
        this.currentTheme.colors = schemeToThemeColors(
          isDark ? record.dark_scheme : record.light_scheme,
          isDark
        )
        return
      }
    }

    if (this.currentTheme.colorSource === 'built_in' && this.currentTheme.selectedBuiltinThemeId.length > 0) {
      const theme = this.findBuiltinTheme(this.currentTheme.selectedBuiltinThemeId)
      if (theme !== null) {
        this.currentTheme.colors = schemeToThemeColors(
          isDark ? theme.dark_scheme : theme.light_scheme,
          isDark
        )
        return
      }
    }

    switch (this.currentTheme.mode) {
      case 'light':
        this.currentTheme.colors = LightTheme
        break
      case 'dark':
        this.currentTheme.colors = DarkTheme
        break
      case 'system':
        this.applySystemColorMode(this.systemColorMode)
        break
    }
  }

  private findPaletteRecord(paletteId: string): ThemePaletteRecordDto | null {
    for (const r of this.paletteRecords) {
      if (r.palette_id === paletteId) return r
    }
    return null
  }

  private findBuiltinTheme(themeId: string): BuiltinThemeDto | null {
    for (const t of this.builtinThemes) {
      if (t.theme_id === themeId) return t
    }
    return null
  }

  private notifyObservers(): void {
    const theme = this.getCurrentTheme()
    this.observers.forEach(obs => obs.onThemeChanged(theme))
  }
}

function schemeToThemeColors(scheme: ThemeColorSchemeDto, isDark: boolean): ThemeColors {
  const base = isDark ? DarkTheme : LightTheme
  const result: ThemeColors = { ...base }

  const merge = (key: keyof ThemeColors, value: string | undefined) => {
    if (value && value.length > 0) {
      (result as Record<string, string>)[key] = value
    }
  }

  merge('primary', scheme.primary)
  merge('onPrimary', scheme.on_primary)
  merge('primaryContainer', scheme.primary_container)
  merge('onPrimaryContainer', scheme.on_primary_container)
  merge('secondary', scheme.secondary)
  merge('onSecondary', scheme.on_secondary)
  merge('secondaryContainer', scheme.secondary_container)
  merge('onSecondaryContainer', scheme.on_secondary_container)
  merge('tertiary', scheme.tertiary)
  merge('onTertiary', scheme.on_tertiary)
  merge('tertiaryContainer', scheme.tertiary_container)
  merge('onTertiaryContainer', scheme.on_tertiary_container)
  merge('error', scheme.error)
  merge('onError', scheme.on_error)
  merge('errorContainer', scheme.error_container)
  merge('onErrorContainer', scheme.on_error_container)
  merge('background', scheme.background)
  merge('onBackground', scheme.on_background)
  merge('surface', scheme.surface)
  merge('onSurface', scheme.on_surface)
  merge('surfaceVariant', scheme.surface_variant)
  merge('onSurfaceVariant', scheme.on_surface_variant)
  merge('surfaceContainerLowest', scheme.surface_container_lowest)
  merge('surfaceContainerLow', scheme.surface_container_low)
  merge('surfaceContainer', scheme.surface_container)
  merge('surfaceContainerHigh', scheme.surface_container_high)
  merge('surfaceContainerHighest', scheme.surface_container_highest)
  merge('surfaceDim', scheme.surface_dim)
  merge('surfaceBright', scheme.surface_bright)
  merge('outline', scheme.outline)
  merge('outlineVariant', scheme.outline_variant)
  merge('inverseSurface', scheme.inverse_surface)
  merge('inverseOnSurface', scheme.inverse_on_surface)
  merge('inversePrimary', scheme.inverse_primary)
  merge('scrim', scheme.scrim)

  result.textPrimary = result.onSurface
  result.textSecondary = result.onSurfaceVariant
  result.textHint = result.outline
  result.divider = result.outlineVariant

  return result
}

export class ThemeObserverImpl implements ThemeObserver {
  private callback: (theme: ThemeConfig) => void

  constructor(callback: (theme: ThemeConfig) => void) {
    this.callback = callback
  }

  onThemeChanged(theme: ThemeConfig): void {
    this.callback(theme)
  }
}

export class ThemeAwareComponent {
  protected themeAdapter: HarmonyThemeAdapter
  private themeObserver: ThemeObserver

  constructor() {
    this.themeAdapter = HarmonyThemeAdapter.getInstance()
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.onThemeChanged(theme)
    })
  }

  attachTheme(): void {
    this.themeAdapter.addObserver(this.themeObserver)
  }

  detachTheme(): void {
    this.themeAdapter.removeObserver(this.themeObserver)
  }

  protected onThemeChanged(theme: ThemeConfig): void {
    // Override in subclasses
  }

  getThemeColors(): ThemeColors {
    return this.themeAdapter.getColors()
  }

  getThemeConfig(): ThemeConfig {
    return this.themeAdapter.getCurrentTheme()
  }
}
