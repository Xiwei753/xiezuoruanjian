import window from '@ohos.window'
import { ThemePaletteRecordDto, ThemeColorSchemeDto, BuiltinThemeDto } from '../../corebridge/dto/SettingsDtos'

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
  private useSystemTokens: boolean = true // 默认使用系统 token，用户显式选择主题后切换为 false
  private effectiveColorMode: string = 'light' // 'light' | 'dark'，根据 mode 和 systemColorMode 计算
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

  // Issue #767：是否使用系统 token（默认启动时为 true，用户显式选择主题后为 false）
  isUsingSystemTokens(): boolean {
    return this.useSystemTokens
  }

  // Issue #767：获取有效颜色模式（'light' | 'dark'），根据 mode 和 systemColorMode 计算
  getEffectiveColorMode(): string {
    return this.effectiveColorMode
  }

  // Issue #767：获取系统当前深浅色模式（0=light, 1=dark）
  getSystemColorMode(): number {
    return this.systemColorMode
  }

  // Issue #767：首次启动时由 EntryAbility 注入当前系统 colorMode，
  // 避免一直靠 systemColorMode = 0 的默认值先按亮色启动。
  initSystemColorMode(colorMode: number): void {
    this.systemColorMode = colorMode
    this.updateEffectiveColorMode()
    if (this.useSystemTokens) {
      this.applySystemColorMode(colorMode)
    }
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
      // Issue #767：用户显式选择了 saved palette，不再使用系统 token
      this.useSystemTokens = false
    }
    this.updateColors()
    this.notifyObservers()
  }

  setSelectedBuiltinThemeId(themeId: string): void {
    this.currentTheme.selectedBuiltinThemeId = themeId
    if (themeId.length > 0) {
      this.currentTheme.colorSource = 'built_in'
      // Issue #767：用户显式选择了 builtin theme，不再使用系统 token
      this.useSystemTokens = false
    }
    this.updateColors()
    this.notifyObservers()
  }

  setPaletteRecords(records: ThemePaletteRecordDto[]): void {
    this.paletteRecords = records
    this.updateColors()
    this.notifyObservers()
  }

  setBuiltinThemes(themes: BuiltinThemeDto[]): void {
    this.builtinThemes = themes
    this.updateColors()
    this.notifyObservers()
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
    this.updateEffectiveColorMode()
    if (this.useSystemTokens) {
      this.applySystemColorMode(colorMode)
    }
    if (this.currentTheme.mode === 'system') {
      this.notifyObservers()
    }
  }

  startSystemThemeListener(): void {
    this.updateEffectiveColorMode()
    if (this.useSystemTokens) {
      this.applySystemColorMode(this.systemColorMode)
    }
  }

  // Issue #767：根据 mode 和 systemColorMode 计算有效颜色模式
  private updateEffectiveColorMode(): void {
    if (this.currentTheme.mode === 'dark') {
      this.effectiveColorMode = 'dark'
    } else if (this.currentTheme.mode === 'light') {
      this.effectiveColorMode = 'light'
    } else {
      // system mode：跟随系统 colorMode
      this.effectiveColorMode = this.systemColorMode === 1 ? 'dark' : 'light'
    }
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
    // Issue #767：使用 effectiveColorMode 判断，而非直接比较 colors === DarkTheme
    return this.effectiveColorMode === 'dark'
  }

  private isCurrentlyDark(): boolean {
    // Issue #767：使用 effectiveColorMode 判断
    return this.effectiveColorMode === 'dark'
  }

  private updateColors(): void {
    this.updateEffectiveColorMode()

    // Issue #767：当 useSystemTokens = true 时，不覆盖 colors，
    // 页面应使用系统 token 而非 themeConfig.colors。
    // currentTheme.colors 保持为 LightTheme/DarkTheme 作为 fallback。
    if (this.useSystemTokens) {
      // 仅更新 fallback colors 以匹配当前模式，不影响 useSystemTokens 标志
      if (this.effectiveColorMode === 'dark') {
        this.currentTheme.colors = DarkTheme
      } else {
        this.currentTheme.colors = LightTheme
      }
      return
    }

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
  // ArkTS 禁止对象展开（arkts-no-spread），逐字段显式复制 base 作为可变起点。
  const result: ThemeColors = {
    primary: base.primary,
    onPrimary: base.onPrimary,
    primaryContainer: base.primaryContainer,
    onPrimaryContainer: base.onPrimaryContainer,
    secondary: base.secondary,
    onSecondary: base.onSecondary,
    secondaryContainer: base.secondaryContainer,
    onSecondaryContainer: base.onSecondaryContainer,
    tertiary: base.tertiary,
    onTertiary: base.onTertiary,
    tertiaryContainer: base.tertiaryContainer,
    onTertiaryContainer: base.onTertiaryContainer,
    error: base.error,
    onError: base.onError,
    errorContainer: base.errorContainer,
    onErrorContainer: base.onErrorContainer,
    background: base.background,
    onBackground: base.onBackground,
    surface: base.surface,
    onSurface: base.onSurface,
    surfaceVariant: base.surfaceVariant,
    onSurfaceVariant: base.onSurfaceVariant,
    surfaceContainerLowest: base.surfaceContainerLowest,
    surfaceContainerLow: base.surfaceContainerLow,
    surfaceContainer: base.surfaceContainer,
    surfaceContainerHigh: base.surfaceContainerHigh,
    surfaceContainerHighest: base.surfaceContainerHighest,
    surfaceDim: base.surfaceDim,
    surfaceBright: base.surfaceBright,
    outline: base.outline,
    outlineVariant: base.outlineVariant,
    inverseSurface: base.inverseSurface,
    inverseOnSurface: base.inverseOnSurface,
    inversePrimary: base.inversePrimary,
    scrim: base.scrim,
    textPrimary: base.textPrimary,
    textSecondary: base.textSecondary,
    textHint: base.textHint,
    success: base.success,
    divider: base.divider,
    overlay: base.overlay,
  }

  if (scheme.primary && scheme.primary.length > 0) { result.primary = scheme.primary }
  if (scheme.on_primary && scheme.on_primary.length > 0) { result.onPrimary = scheme.on_primary }
  if (scheme.primary_container && scheme.primary_container.length > 0) { result.primaryContainer = scheme.primary_container }
  if (scheme.on_primary_container && scheme.on_primary_container.length > 0) { result.onPrimaryContainer = scheme.on_primary_container }
  if (scheme.secondary && scheme.secondary.length > 0) { result.secondary = scheme.secondary }
  if (scheme.on_secondary && scheme.on_secondary.length > 0) { result.onSecondary = scheme.on_secondary }
  if (scheme.secondary_container && scheme.secondary_container.length > 0) { result.secondaryContainer = scheme.secondary_container }
  if (scheme.on_secondary_container && scheme.on_secondary_container.length > 0) { result.onSecondaryContainer = scheme.on_secondary_container }
  if (scheme.tertiary && scheme.tertiary.length > 0) { result.tertiary = scheme.tertiary }
  if (scheme.on_tertiary && scheme.on_tertiary.length > 0) { result.onTertiary = scheme.on_tertiary }
  if (scheme.tertiary_container && scheme.tertiary_container.length > 0) { result.tertiaryContainer = scheme.tertiary_container }
  if (scheme.on_tertiary_container && scheme.on_tertiary_container.length > 0) { result.onTertiaryContainer = scheme.on_tertiary_container }
  if (scheme.error && scheme.error.length > 0) { result.error = scheme.error }
  if (scheme.on_error && scheme.on_error.length > 0) { result.onError = scheme.on_error }
  if (scheme.error_container && scheme.error_container.length > 0) { result.errorContainer = scheme.error_container }
  if (scheme.on_error_container && scheme.on_error_container.length > 0) { result.onErrorContainer = scheme.on_error_container }
  if (scheme.background && scheme.background.length > 0) { result.background = scheme.background }
  if (scheme.on_background && scheme.on_background.length > 0) { result.onBackground = scheme.on_background }
  if (scheme.surface && scheme.surface.length > 0) { result.surface = scheme.surface }
  if (scheme.on_surface && scheme.on_surface.length > 0) { result.onSurface = scheme.on_surface }
  if (scheme.surface_variant && scheme.surface_variant.length > 0) { result.surfaceVariant = scheme.surface_variant }
  if (scheme.on_surface_variant && scheme.on_surface_variant.length > 0) { result.onSurfaceVariant = scheme.on_surface_variant }
  if (scheme.surface_container_lowest && scheme.surface_container_lowest.length > 0) { result.surfaceContainerLowest = scheme.surface_container_lowest }
  if (scheme.surface_container_low && scheme.surface_container_low.length > 0) { result.surfaceContainerLow = scheme.surface_container_low }
  if (scheme.surface_container && scheme.surface_container.length > 0) { result.surfaceContainer = scheme.surface_container }
  if (scheme.surface_container_high && scheme.surface_container_high.length > 0) { result.surfaceContainerHigh = scheme.surface_container_high }
  if (scheme.surface_container_highest && scheme.surface_container_highest.length > 0) { result.surfaceContainerHighest = scheme.surface_container_highest }
  if (scheme.surface_dim && scheme.surface_dim.length > 0) { result.surfaceDim = scheme.surface_dim }
  if (scheme.surface_bright && scheme.surface_bright.length > 0) { result.surfaceBright = scheme.surface_bright }
  if (scheme.outline && scheme.outline.length > 0) { result.outline = scheme.outline }
  if (scheme.outline_variant && scheme.outline_variant.length > 0) { result.outlineVariant = scheme.outline_variant }
  if (scheme.inverse_surface && scheme.inverse_surface.length > 0) { result.inverseSurface = scheme.inverse_surface }
  if (scheme.inverse_on_surface && scheme.inverse_on_surface.length > 0) { result.inverseOnSurface = scheme.inverse_on_surface }
  if (scheme.inverse_primary && scheme.inverse_primary.length > 0) { result.inversePrimary = scheme.inverse_primary }
  if (scheme.scrim && scheme.scrim.length > 0) { result.scrim = scheme.scrim }

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
