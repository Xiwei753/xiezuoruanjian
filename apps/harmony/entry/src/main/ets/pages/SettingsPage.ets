import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import {
  LocalSettings,
  SyncableSettings,
  SyncConfig,
  SyncResult,
  ThemeMode,
  ResultEnvelope
} from '../model/CoreDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeUpdateParams, ThemeObserverImpl } from '../system/HarmonyThemeAdapter'
import { getBridge } from '../common/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../common/AdaptiveContext'
import { WidthClass, ShellMode } from '../model/CoreDtos'

interface SettingsSelectOption {
  value: string
  icon: string
}

@Entry
@Component
struct SettingsPage {
  @State settings: LocalSettings = {
    themeMode: 'system',
    autoSaveEnabled: true,
    editorFontSize: 16,
    editorLineSpacingMultiplier: 1.8,
    windowWidth: 0,
    windowHeight: 0,
    autoSaveDelayMs: 30000,
    autoIndentEnabled: true,
    autoIndentWidth: 2,
    editorTypingAnimationEnabled: true,
    editorSmoothCursorEnabled: true,
    editorTypingAnimationDurationMs: 80,
    editorSmoothCursorDurationMs: 120,
    aiEnabled: false,
    desktopSidebarWidth: 280,
    desktopEditorWidth: 820
  }
  @State syncConfig: SyncConfig | null = null
  @State isLoading: boolean = true
  @State isSaving: boolean = false
  @State isSyncing: boolean = false
  @State saveMessage: string = ''
  @State syncMessage: string = ''
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State adaptiveState: AdaptiveState = getAdaptiveState()

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  aboutToAppear() {
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    // 注册响应式布局观察者
    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver)
    // 初始化时同步一次状态
    this.adaptiveState = AdaptiveContext.getInstance().getCurrentState()

    this.loadSettings()
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  async loadSettings() {
    this.isLoading = true

    try {
      const settingsResult = await this.bridge.getLocalSettings()
      const syncConfigResult = await this.bridge.loadSyncConfig()

      if (settingsResult.success && settingsResult.data) {
        this.settings = settingsResult.data
        this.themeAdapter.updateFromSettings({
          theme: settingsResult.data.themeMode || 'system',
          fontSize: settingsResult.data.editorFontSize,
          fontFamily: 'HarmonyOS Sans',
          lineHeight: settingsResult.data.editorLineSpacingMultiplier
        } as ThemeUpdateParams)
      }

      if (syncConfigResult.success && syncConfigResult.data) {
        this.syncConfig = syncConfigResult.data
      }
    } catch (err) {
      console.error('Failed to load settings:', err)
    } finally {
      this.isLoading = false
    }
  }

  async saveSettings() {
    this.isSaving = true
    this.saveMessage = ''

    try {
      const result = await this.bridge.saveLocalSettings(this.settings)
      if (result.success) {
        this.saveMessage = '设置已保存'
        this.themeAdapter.updateFromSettings({
          theme: this.settings.themeMode || 'system',
          fontSize: this.settings.editorFontSize,
          fontFamily: 'HarmonyOS Sans',
          lineHeight: this.settings.editorLineSpacingMultiplier
        } as ThemeUpdateParams)
        setTimeout(() => {
          this.saveMessage = ''
        }, 2000)
      } else {
        this.saveMessage = result.userMessage || '保存失败'
      }
    } catch (err) {
      this.saveMessage = '保存时发生错误'
    } finally {
      this.isSaving = false
    }
  }

  async performSync() {
    this.isSyncing = true
    this.syncMessage = ''

    try {
      const result = await this.bridge.performSync()
      if (result.success && result.data) {
        this.syncMessage = `同步完成: ${result.data.downloadedFiles.length} 个文件已下载`
        setTimeout(() => {
          this.syncMessage = ''
        }, 3000)
      } else {
        this.syncMessage = result.userMessage || '同步失败'
      }
    } catch (err) {
      this.syncMessage = '同步时发生错误'
    } finally {
      this.isSyncing = false
    }
  }

  build() {
    Column() {
      // Header
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(() => router.back())

        Text('设置')
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ left: 12 })

        Blank()

        if (this.saveMessage) {
          Text(this.saveMessage)
            .fontSize(12)
            .fontColor(this.saveMessage === '设置已保存' ? this.themeConfig.colors.success : this.themeConfig.colors.error)
        }

        Button('保存')
          .fontSize(14)
          .height(32)
          .backgroundColor(this.themeConfig.colors.primary)
          .enabled(!this.isSaving)
          .onClick(() => this.saveSettings())
      }
      .width('100%')
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 16, bottom: 12 })

      if (this.isLoading) {
        Column() {
          LoadingProgress()
            .width(48)
            .height(48)
            .color(this.themeConfig.colors.primary)
          Text('加载中...')
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textHint)
            .margin({ top: 12 })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        Scroll() {
          Column() {
            // Editor settings
            this.EditorSettingsSection()

            // Theme settings
            this.ThemeSettingsSection()

            // Sync settings
            this.SyncSettingsSection()
          }
          .width('100%')
          .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
          .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, bottom: 24 })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
        .align(Alignment.Top)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
  }

  @Builder
  EditorSettingsSection() {
    Column() {
      Text('编辑器设置')
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })

      // Font size
      Row() {
        Text('字体大小')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Text(`${this.settings.editorFontSize}px`)
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 16 })

        Slider({
          value: this.settings.editorFontSize,
          min: 12,
          max: 32,
          step: 1
        })
          .width(120)
          .onChange((value: number) => {
            this.settings.editorFontSize = value
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Line spacing multiplier
      Row() {
        Text('行距倍数')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Text(`${this.settings.editorLineSpacingMultiplier.toFixed(1)}`)
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 16 })

        Slider({
          value: this.settings.editorLineSpacingMultiplier,
          min: 1.0,
          max: 3.0,
          step: 0.1
        })
          .width(120)
          .onChange((value: number) => {
            this.settings.editorLineSpacingMultiplier = Math.round(value * 10) / 10
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Auto save
      Row() {
        Text('自动保存')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.settings.autoSaveEnabled })
          .onChange((isOn: boolean) => {
            this.settings.autoSaveEnabled = isOn
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Auto save delay
      if (this.settings.autoSaveEnabled) {
        Row() {
          Text('自动保存延迟')
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

          Text(`${Math.round(this.settings.autoSaveDelayMs / 1000)} 秒`)
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 16 })

          Slider({
            value: this.settings.autoSaveDelayMs / 1000,
            min: 10,
            max: 120,
            step: 10
          })
            .width(120)
            .onChange((value: number) => {
              this.settings.autoSaveDelayMs = value * 1000
            })
        }
        .width('100%')
        .margin({ bottom: 16 })
      }

      // Auto indent
      Row() {
        Text('自动缩进')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.settings.autoIndentEnabled })
          .onChange((isOn: boolean) => {
            this.settings.autoIndentEnabled = isOn
          })
      }
      .width('100%')
    }
    .width('100%')
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(12)
    .margin({ bottom: 16 })
  }

  @Builder
  ThemeSettingsSection() {
    Column() {
      Text('主题设置')
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })

      // Theme mode
      Row() {
        Text('主题模式')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Select([
          { value: '浅色', icon: '' },
          { value: '深色', icon: '' },
          { value: '跟随系统', icon: '' }
        ] as SettingsSelectOption[])
          .selected(this.getThemeIndex())
          .value(this.getThemeName())
          .onSelect((index: number) => {
            const themes: ThemeMode[] = ['light', 'dark', 'system']
            this.settings.themeMode = themes[index]
            this.themeAdapter.setMode(this.settings.themeMode)
          })
      }
      .width('100%')
    }
    .width('100%')
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(12)
    .margin({ bottom: 16 })
  }

  @Builder
  SyncSettingsSection() {
    Column() {
      Row() {
        Text('同步设置')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        if (this.syncMessage) {
          Text(this.syncMessage)
            .fontSize(12)
            .fontColor(this.syncMessage.includes('完成') ? this.themeConfig.colors.success : this.themeConfig.colors.error)
        }
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Sync enabled
      Row() {
        Text('启用同步')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.syncConfig?.enabled || false })
          .onChange((isOn: boolean) => {
            if (this.syncConfig) {
              this.syncConfig.enabled = isOn
            }
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Sync backend type
      if (this.syncConfig?.enabled) {
        Row() {
          Text('同步方式')
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

          Select([
            { value: 'GitHub', icon: '' },
            { value: 'GitLab', icon: '' },
            { value: 'Gitea', icon: '' },
            { value: '自定义', icon: '' }
          ] as SettingsSelectOption[])
            .selected(this.getSyncProviderIndex())
            .value(this.getSyncProviderName())
            .onSelect((index: number) => {
              if (this.syncConfig) {
                const providers = ['github', 'gitlab', 'gitea', 'custom']
                this.syncConfig.backendType = providers[index]
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Auto sync
        Row() {
          Text('自动同步')
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

          Toggle({ type: ToggleType.Switch, isOn: this.syncConfig?.autoSync || false })
            .onChange((isOn: boolean) => {
              if (this.syncConfig) {
                this.syncConfig.autoSync = isOn
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Sync interval
        Row() {
          Text('同步间隔')
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

          Text(`${this.syncConfig?.syncIntervalSeconds || 300} 秒`)
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 16 })

          Slider({
            value: this.syncConfig?.syncIntervalSeconds || 300,
            min: 60,
            max: 3600,
            step: 60
          })
            .width(120)
            .onChange((value: number) => {
              if (this.syncConfig) {
                this.syncConfig.syncIntervalSeconds = value
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Sync actions
        Row() {
          Button('同步')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.primary)
            .enabled(!this.isSyncing)
            .onClick(() => this.performSync())
        }
        .width('100%')
        .margin({ bottom: 16 })
      }
    }
    .width('100%')
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(12)
  }

  getThemeIndex(): number {
    const themes = ['light', 'dark', 'system']
    return themes.indexOf(this.settings.themeMode || 'system')
  }

  getThemeName(): string {
    if (this.settings.themeMode === 'light') return '浅色'
    if (this.settings.themeMode === 'dark') return '深色'
    return '跟随系统'
  }

  getSyncProviderIndex(): number {
    const providers: string[] = ['github', 'gitlab', 'gitea', 'custom']
    return providers.indexOf(this.syncConfig?.backendType || 'github')
  }

  getSyncProviderName(): string {
    const p: string = this.syncConfig?.backendType || 'github'
    if (p === 'github') return 'GitHub'
    if (p === 'gitlab') return 'GitLab'
    if (p === 'gitea') return 'Gitea'
    return '自定义'
  }
}
