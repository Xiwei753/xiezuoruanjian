import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import {
  LocalSettings,
  SyncableSettings,
  SyncConfig,
  SyncDiagnostics,
  SyncResult,
  ThemeMode,
  ResultEnvelope,
  SyncProvider,
  ConflictStrategy
} from '../model/CoreDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeUpdateParams, ThemeObserverImpl } from '../system/HarmonyThemeAdapter'
import { getBridge } from '../common/AppContext'

interface SettingsSelectOption {
  value: string
  icon: string
}

@Entry
@Component
struct SettingsPage {
  @State settings: LocalSettings = {
    fontSize: 16,
    lineHeight: 1.8,
    fontFamily: 'HarmonyOS Sans',
    theme: 'system',
    autoSave: true,
    autoSaveInterval: 30,
    autoIndent: true,
    showWordCount: true,
    showLineNumbers: false,
    wordWrap: true,
    spellCheck: false
  }
  @State syncConfig: SyncConfig | null = null
  @State syncDiagnostics: SyncDiagnostics | null = null
  @State isLoading: boolean = true
  @State isSaving: boolean = false
  @State isSyncing: boolean = false
  @State saveMessage: string = ''
  @State syncMessage: string = ''
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null

  aboutToAppear() {
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)
    this.loadSettings()
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
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
          theme: settingsResult.data.theme,
          fontSize: settingsResult.data.fontSize,
          fontFamily: settingsResult.data.fontFamily,
          lineHeight: settingsResult.data.lineHeight
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
          theme: this.settings.theme,
          fontSize: this.settings.fontSize,
          fontFamily: this.settings.fontFamily,
          lineHeight: this.settings.lineHeight
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

  async runSyncDiagnostics() {
    try {
      const result = await this.bridge.syncDiagnostics()
      if (result.success && result.data) {
        this.syncDiagnostics = result.data
      }
    } catch (err) {
      console.error('Sync diagnostics failed:', err)
    }
  }

  async performSync() {
    this.isSyncing = true
    this.syncMessage = ''

    try {
      const result = await this.bridge.performSync()
      if (result.success && result.data) {
        this.syncMessage = `同步完成: ${result.data.filesModified} 个文件已更新`
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
      .padding({ left: 16, right: 16, top: 16, bottom: 12 })

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

            // Display settings
            this.DisplaySettingsSection()

            // Theme settings
            this.ThemeSettingsSection()

            // Sync settings
            this.SyncSettingsSection()
          }
          .width('100%')
          .padding({ left: 16, right: 16, bottom: 24 })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
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

        Text(`${this.settings.fontSize}px`)
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 16 })

        Slider({
          value: this.settings.fontSize,
          min: 12,
          max: 32,
          step: 1
        })
          .width(120)
          .onChange((value: number) => {
            this.settings.fontSize = value
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Line height
      Row() {
        Text('行高')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Text(`${this.settings.lineHeight.toFixed(1)}`)
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 16 })

        Slider({
          value: this.settings.lineHeight,
          min: 1.0,
          max: 3.0,
          step: 0.1
        })
          .width(120)
          .onChange((value: number) => {
            this.settings.lineHeight = Math.round(value * 10) / 10
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

        Toggle({ type: ToggleType.Switch, isOn: this.settings.autoSave })
          .onChange((isOn: boolean) => {
            this.settings.autoSave = isOn
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Auto save interval
      if (this.settings.autoSave) {
        Row() {
          Text('自动保存间隔')
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

          Text(`${this.settings.autoSaveInterval} 秒`)
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 16 })

          Slider({
            value: this.settings.autoSaveInterval,
            min: 10,
            max: 120,
            step: 10
          })
            .width(120)
            .onChange((value: number) => {
              this.settings.autoSaveInterval = value
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

        Toggle({ type: ToggleType.Switch, isOn: this.settings.autoIndent })
          .onChange((isOn: boolean) => {
            this.settings.autoIndent = isOn
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
  DisplaySettingsSection() {
    Column() {
      Text('显示设置')
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })

      // Show word count
      Row() {
        Text('显示字数统计')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.settings.showWordCount })
          .onChange((isOn: boolean) => {
            this.settings.showWordCount = isOn
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Show line numbers
      Row() {
        Text('显示行号')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.settings.showLineNumbers })
          .onChange((isOn: boolean) => {
            this.settings.showLineNumbers = isOn
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Word wrap
      Row() {
        Text('自动换行')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.settings.wordWrap })
          .onChange((isOn: boolean) => {
            this.settings.wordWrap = isOn
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Spell check
      Row() {
        Text('拼写检查')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.settings.spellCheck })
          .onChange((isOn: boolean) => {
            this.settings.spellCheck = isOn
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
            this.settings.theme = themes[index]
            this.themeAdapter.setMode(this.settings.theme)
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Font family
      Row() {
        Text('字体')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Select([
          { value: 'HarmonyOS Sans', icon: '' },
          { value: '系统默认', icon: '' }
        ] as SettingsSelectOption[])
          .selected(0)
          .value(this.settings.fontFamily)
          .onSelect((index: number) => {
            this.settings.fontFamily = index === 0 ? 'HarmonyOS Sans' : 'System Default'
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

      // Sync provider
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
                this.syncConfig.provider = providers[index] as SyncProvider
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

        // Conflict strategy
        Row() {
          Text('冲突处理')
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

          Select([
            { value: '保留本地', icon: '' },
            { value: '保留远程', icon: '' },
            { value: '手动处理', icon: '' },
            { value: '自动合并', icon: '' }
          ] as SettingsSelectOption[])
            .selected(this.getConflictStrategyIndex())
            .value(this.getConflictStrategyName())
            .onSelect((index: number) => {
              if (this.syncConfig) {
                const strategies = ['keep_local', 'keep_remote', 'manual', 'auto_merge']
                this.syncConfig.conflictStrategy = strategies[index] as ConflictStrategy
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Sync actions
        Row() {
          Button('诊断')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.secondary)
            .margin({ right: 12 })
            .onClick(() => this.runSyncDiagnostics())

          Button('同步')
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.primary)
            .enabled(!this.isSyncing)
            .onClick(() => this.performSync())
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Sync diagnostics
        if (this.syncDiagnostics) {
          this.SyncDiagnosticsCard()
        }
      }
    }
    .width('100%')
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(12)
  }

  @Builder
  SyncDiagnosticsCard() {
    Column() {
      Text('同步诊断')
        .fontSize(14)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 8 })

      Row() {
        Text('Git 仓库')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .layoutWeight(1)

        Text(this.syncDiagnostics?.isGitRepository ? '是' : '否')
          .fontSize(14)
          .fontColor(this.syncDiagnostics?.isGitRepository ? this.themeConfig.colors.success : this.themeConfig.colors.error)
      }
      .width('100%')
      .margin({ bottom: 4 })

      Row() {
        Text('远程仓库')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .layoutWeight(1)

        Text(this.syncDiagnostics?.hasRemote ? '已配置' : '未配置')
          .fontSize(14)
          .fontColor(this.syncDiagnostics?.hasRemote ? this.themeConfig.colors.success : this.themeConfig.colors.error)
      }
      .width('100%')
      .margin({ bottom: 4 })

      Row() {
        Text('工作区状态')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .layoutWeight(1)

        Text(this.syncDiagnostics?.isClean ? '干净' : '有变更')
          .fontSize(14)
          .fontColor(this.syncDiagnostics?.isClean ? this.themeConfig.colors.success : this.themeConfig.colors.secondary)
      }
      .width('100%')

      if (this.syncDiagnostics?.warnings && this.syncDiagnostics.warnings.length > 0) {
        Column() {
          ForEach(this.syncDiagnostics.warnings, (warning: string) => {
            Text(`⚠ ${warning}`)
              .fontSize(12)
              .fontColor(this.themeConfig.colors.secondary)
              .margin({ top: 4 })
          }, (warning: string) => warning)
        }
        .width('100%')
        .margin({ top: 8 })
      }
    }
    .width('100%')
    .padding(12)
    .backgroundColor(this.themeConfig.colors.background)
    .borderRadius(8)
  }

  getThemeIndex(): number {
    const themes = ['light', 'dark', 'system']
    return themes.indexOf(this.settings.theme)
  }

  getThemeName(): string {
    if (this.settings.theme === 'light') return '浅色'
    if (this.settings.theme === 'dark') return '深色'
    return '跟随系统'
  }

  getSyncProviderIndex(): number {
    const providers: string[] = ['github', 'gitlab', 'gitea', 'custom']
    return providers.indexOf(this.syncConfig?.provider || 'github')
  }

  getSyncProviderName(): string {
    const p: SyncProvider = this.syncConfig?.provider || 'github'
    if (p === 'github') return 'GitHub'
    if (p === 'gitlab') return 'GitLab'
    if (p === 'gitea') return 'Gitea'
    return '自定义'
  }

  getConflictStrategyIndex(): number {
    const strategies: string[] = ['keep_local', 'keep_remote', 'manual', 'auto_merge']
    return strategies.indexOf(this.syncConfig?.conflictStrategy || 'manual')
  }

  getConflictStrategyName(): string {
    const s: ConflictStrategy = this.syncConfig?.conflictStrategy || 'manual'
    if (s === 'keep_local') return '保留本地'
    if (s === 'keep_remote') return '保留远程'
    if (s === 'auto_merge') return '自动合并'
    return '手动处理'
  }
}
