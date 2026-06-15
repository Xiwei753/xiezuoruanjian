export type ThemeMode = 'light' | 'dark' | 'system'

export interface ThemeColors {
  primary: string
  secondary: string
  background: string
  surface: string
  textPrimary: string
  textSecondary: string
  textHint: string
  error: string
  success: string
  divider: string
  overlay: string
}

export interface ThemeConfig {
  mode: ThemeMode
  colors: ThemeColors
  fontSize: number
  fontFamily: string
  lineHeight: number
}

export const LightTheme: ThemeColors = {
  primary: '#4A90D9',
  secondary: '#7B68EE',
  background: '#F5F5F5',
  surface: '#FFFFFF',
  textPrimary: '#333333',
  textSecondary: '#666666',
  textHint: '#999999',
  error: '#E74C3C',
  success: '#2ECC71',
  divider: '#EEEEEE',
  overlay: '#80000000'
}

export const DarkTheme: ThemeColors = {
  primary: '#5BA0E9',
  secondary: '#8B78FF',
  background: '#1A1A1A',
  surface: '#2D2D2D',
  textPrimary: '#FFFFFF',
  textSecondary: '#B0B0B0',
  textHint: '#808080',
  error: '#FF6B6B',
  success: '#4ADE80',
  divider: '#404040',
  overlay: '#80000000'
}

export interface ThemeObserver {
  onThemeChanged(theme: ThemeConfig): void
}

export interface ThemeUpdateParams {
  theme: ThemeMode
  fontSize: number
  fontFamily: string
  lineHeight: number
}

export class HarmonyThemeAdapter {
  private static instance: HarmonyThemeAdapter
  private currentTheme: ThemeConfig = {
    mode: 'system',
    colors: LightTheme,
    fontSize: 16,
    fontFamily: 'HarmonyOS Sans',
    lineHeight: 1.8
  }
  private observers: Set<ThemeObserver> = new Set()
  private systemThemeListener: ((isDark: boolean) => void) | null = null

  static getInstance(): HarmonyThemeAdapter {
    if (!HarmonyThemeAdapter.instance) {
      HarmonyThemeAdapter.instance = new HarmonyThemeAdapter()
    }
    return HarmonyThemeAdapter.instance
  }

  getCurrentTheme(): ThemeConfig {
    const c: ThemeConfig = {
      mode: this.currentTheme.mode,
      colors: this.currentTheme.colors,
      fontSize: this.currentTheme.fontSize,
      fontFamily: this.currentTheme.fontFamily,
      lineHeight: this.currentTheme.lineHeight
    }
    return c
  }

  getColors(): ThemeColors {
    const c: ThemeColors = {
      primary: this.currentTheme.colors.primary,
      secondary: this.currentTheme.colors.secondary,
      background: this.currentTheme.colors.background,
      surface: this.currentTheme.colors.surface,
      textPrimary: this.currentTheme.colors.textPrimary,
      textSecondary: this.currentTheme.colors.textSecondary,
      textHint: this.currentTheme.colors.textHint,
      error: this.currentTheme.colors.error,
      success: this.currentTheme.colors.success,
      divider: this.currentTheme.colors.divider,
      overlay: this.currentTheme.colors.overlay
    }
    return c
  }

  getMode(): ThemeMode {
    return this.currentTheme.mode
  }

  setMode(mode: ThemeMode): void {
    this.currentTheme.mode = mode
    this.updateColors()
    this.notifyObservers()
  }

  setFontSize(fontSize: number): void {
    this.currentTheme.fontSize = Math.max(12, Math.min(32, fontSize))
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

  addObserver(observer: ThemeObserver): void {
    this.observers.add(observer)
  }

  removeObserver(observer: ThemeObserver): void {
    this.observers.delete(observer)
  }

  startSystemThemeListener(): void {
    // TODO: Implement with HarmonyOS Configuration API
    // This will listen to system theme changes
    // configuration.getConfiguration().then(config => { ... })
  }

  stopSystemThemeListener(): void {
    this.systemThemeListener = null
  }

  isDarkMode(): boolean {
    return this.currentTheme.colors === DarkTheme
  }

  private updateColors(): void {
    switch (this.currentTheme.mode) {
      case 'light':
        this.currentTheme.colors = LightTheme
        break
      case 'dark':
        this.currentTheme.colors = DarkTheme
        break
      case 'system':
        // TODO: Detect system theme
        this.currentTheme.colors = LightTheme
        break
    }
  }

  private notifyObservers(): void {
    const theme = this.getCurrentTheme()
    this.observers.forEach(obs => obs.onThemeChanged(theme))
  }
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
