import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import {
  LocalSettings,
  SyncableSettings,
  SyncConfig,
  SyncResult,
  SyncDiagnosticsResult,
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
  @State syncDiagnostics: SyncDiagnosticsResult | null = null
  @State isDiagnosing: boolean = false
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
        this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.settings_saved').id)
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
        this.saveMessage = result.userMessage || getContext(this).resourceManager.getStringSync($r('app.string.save_failed').id)
      }
    } catch (err) {
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_save').id)
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
        this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.sync_complete_format').id).replace('%s', `${result.data.downloadedFiles.length}`)
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

  async runSyncDiagnostics() {
    this.isDiagnosing = true
    this.syncDiagnostics = null

    try {
      const result = await this.bridge.syncDiagnostics()
      if (result.success && result.data) {
        this.syncDiagnostics = result.data
      } else {
        this.syncDiagnostics = {
          success: false,
          backendType: '',
          androidHasInternetPermission: false,
          androidHasAccessNetworkStatePermission: false,
          androidNetworkState: '',
          tcpProbeOk: false,
          tcpProbeStatus: '',
          httpConnectProbeOk: false,
          httpConnectProbeStatus: '',
          libgit2ProbeOk: false,
          libgit2ProbeStatus: '',
          networkOk: false,
          authOk: false,
          repoOk: false,
          branchOk: false,
          networkStatus: '',
          authStatus: '',
          repoStatus: '',
          branchStatus: '',
          remoteUrlSanitized: '',
          transport: '',
          errorCategory: 'unknown',
          userMessage: result.userMessage || getContext(this).resourceManager.getStringSync($r('app.string.diagnostics_error').id)
        } as SyncDiagnosticsResult
      }
    } catch (err) {
      this.syncDiagnostics = {
        success: false,
        backendType: '',
        androidHasInternetPermission: false,
        androidHasAccessNetworkStatePermission: false,
        androidNetworkState: '',
        tcpProbeOk: false,
        tcpProbeStatus: '',
        httpConnectProbeOk: false,
        httpConnectProbeStatus: '',
        libgit2ProbeOk: false,
        libgit2ProbeStatus: '',
        networkOk: false,
        authOk: false,
        repoOk: false,
        branchOk: false,
        networkStatus: '',
        authStatus: '',
        repoStatus: '',
        branchStatus: '',
        remoteUrlSanitized: '',
        transport: '',
        errorCategory: 'unknown',
        userMessage: getContext(this).resourceManager.getStringSync($r('app.string.diagnostics_error_detail').id)
      } as SyncDiagnosticsResult
    } finally {
      this.isDiagnosing = false
    }
  }

  build() {
    Column() {
      // Header — 无保存按钮，仅返回+标题+保存状态提示
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(() => router.back())

        Text($r('app.string.settings'))
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ left: 12 })

        Blank()

        // 保存状态提示（替代保存按钮）
        if (this.saveMessage) {
          Text(this.saveMessage)
            .fontSize(12)
            .fontColor(this.saveMessage === getContext(this).resourceManager.getStringSync($r('app.string.settings_saved').id) ? this.themeConfig.colors.success : this.themeConfig.colors.error)
        }
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
          Text($r('app.string.loading'))
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
            // 按 Core 契约顺序：外观→编辑器→保存→同步→AI→关于/高级
            this.AppearanceSettingsSection()
            this.EditorSettingsSection()
            this.SaveSettingsSection()
            this.SyncSettingsSection()
            this.AiSettingsSection()
            this.AboutSettingsSection()
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

  // ── 外观 (appearance) ── 主题模式、字体大小、行距
  @Builder
  AppearanceSettingsSection() {
    Column() {
      Text($r('app.string.appearance'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })

      // 主题模式 — 顺序必须是 跟随系统→浅色→深色
      Row() {
        Text($r('app.string.theme_mode'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Select([
          { value: $r('app.string.follow_system') as string, icon: '' },
          { value: $r('app.string.light_mode') as string, icon: '' },
          { value: $r('app.string.dark_mode') as string, icon: '' }
        ] as SettingsSelectOption[])
          .selected(this.getThemeIndex())
          .value(this.getThemeName())
          .onSelect((index: number) => {
            const themes: ThemeMode[] = ['system', 'light', 'dark']
            const selectedTheme: ThemeMode = themes[index]
            this.settings.themeMode = selectedTheme
            this.themeAdapter.setMode(selectedTheme)
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 字体大小
      Row() {
        Text($r('app.string.font_size'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Text(`${this.settings.editorFontSize}px`)
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 16 })
        Slider({ value: this.settings.editorFontSize, min: 12, max: 32, step: 1 })
          .width(120)
          .onChange((value: number) => {
            this.settings.editorFontSize = value
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 行距倍数
      Row() {
        Text($r('app.string.line_spacing_multiplier'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Text(`${this.settings.editorLineSpacingMultiplier.toFixed(1)}`)
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 16 })
        Slider({ value: this.settings.editorLineSpacingMultiplier, min: 1.0, max: 3.0, step: 0.1 })
          .width(120)
          .onChange((value: number) => {
            this.settings.editorLineSpacingMultiplier = Math.round(value * 10) / 10
            this.saveSettings()
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

  // ── 编辑器 (editor) ── 自动缩进、缩进宽度、打字动画、动画时长、平滑光标、光标时长
  @Builder
  EditorSettingsSection() {
    Column() {
      Text($r('app.string.editor'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })

      // 自动缩进
      Row() {
        Text($r('app.string.auto_indent'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.settings.autoIndentEnabled })
          .onChange((isOn: boolean) => {
            this.settings.autoIndentEnabled = isOn
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 缩进宽度
      Row() {
        Text($r('app.string.indent_width'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Text(`${this.settings.autoIndentWidth}`)
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 16 })
        Slider({ value: this.settings.autoIndentWidth, min: 0, max: 8, step: 1 })
          .width(120)
          .onChange((value: number) => {
            this.settings.autoIndentWidth = value
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 打字动画
      Row() {
        Text($r('app.string.typing_animation'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.settings.editorTypingAnimationEnabled })
          .onChange((isOn: boolean) => {
            this.settings.editorTypingAnimationEnabled = isOn
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 打字动画时长
      if (this.settings.editorTypingAnimationEnabled) {
        Row() {
           Text($r('app.string.typing_animation_duration'))
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)
          Text(`${this.settings.editorTypingAnimationDurationMs}ms`)
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 16 })
          Slider({ value: this.settings.editorTypingAnimationDurationMs, min: 0, max: 1000, step: 10 })
            .width(120)
            .onChange((value: number) => {
              this.settings.editorTypingAnimationDurationMs = value
              this.saveSettings()
            })
        }
        .width('100%')
        .margin({ bottom: 16 })
      }

      // 平滑光标
      Row() {
        Text($r('app.string.smooth_cursor'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.settings.editorSmoothCursorEnabled })
          .onChange((isOn: boolean) => {
            this.settings.editorSmoothCursorEnabled = isOn
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 平滑光标时长
      if (this.settings.editorSmoothCursorEnabled) {
        Row() {
           Text($r('app.string.smooth_cursor_duration'))
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)
          Text(`${this.settings.editorSmoothCursorDurationMs}ms`)
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 16 })
          Slider({ value: this.settings.editorSmoothCursorDurationMs, min: 0, max: 1000, step: 10 })
            .width(120)
            .onChange((value: number) => {
              this.settings.editorSmoothCursorDurationMs = value
              this.saveSettings()
            })
        }
        .width('100%')
        .margin({ bottom: 16 })
      }
    }
    .width('100%')
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(12)
    .margin({ bottom: 16 })
  }

  // ── 保存 (save) ── 自动保存开关、自动保存延迟
  @Builder
  SaveSettingsSection() {
    Column() {
      Text($r('app.string.save_section'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })

      // 自动保存
      Row() {
        Text($r('app.string.auto_save'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.settings.autoSaveEnabled })
          .onChange((isOn: boolean) => {
            this.settings.autoSaveEnabled = isOn
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 自动保存延迟
      if (this.settings.autoSaveEnabled) {
        Row() {
           Text($r('app.string.auto_save_delay'))
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)
          Text(getContext(this).resourceManager.getStringSync($r('app.string.seconds_format').id).replace('%s', `${Math.round(this.settings.autoSaveDelayMs / 1000)}`))
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 16 })
          Slider({ value: this.settings.autoSaveDelayMs / 1000, min: 1, max: 10, step: 1 })
            .width(120)
            .onChange((value: number) => {
              this.settings.autoSaveDelayMs = value * 1000
              this.saveSettings()
            })
        }
        .width('100%')
        .margin({ bottom: 16 })
      }
    }
    .width('100%')
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(12)
    .margin({ bottom: 16 })
  }

  // ── 同步 (sync) ── 同步开关、同步方式、自动同步、同步间隔、同步按钮、诊断
  @Builder
  SyncSettingsSection() {
    Column() {
      Row() {
        Text($r('app.string.sync_section'))
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
        Text($r('app.string.enable_sync'))
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
           Text($r('app.string.sync_method'))
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

           Select([
            { value: 'GitHub', icon: '' },
            { value: 'GitLab', icon: '' },
            { value: 'Gitea', icon: '' },
            { value: $r('app.string.custom') as string, icon: '' }
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
           Text($r('app.string.auto_sync'))
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
           Text($r('app.string.sync_interval'))
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

          Text(getContext(this).resourceManager.getStringSync($r('app.string.seconds_format').id).replace('%s', `${this.syncConfig?.syncIntervalSeconds || 300}`))
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
          Button($r('app.string.sync_section'))
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.primary)
            .enabled(!this.isSyncing)
            .onClick(() => this.performSync())

          Button($r('app.string.diagnose'))
            .fontSize(14)
            .height(36)
            .backgroundColor(this.themeConfig.colors.secondary || '#6b7280')
            .fontColor(this.themeConfig.colors.secondary || '#ffffff')
            .margin({ left: 12 })
            .enabled(!this.isDiagnosing)
            .onClick(() => this.runSyncDiagnostics())
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Diagnostics result card
        if (this.syncDiagnostics) {
          Column() {
             Text($r('app.string.diagnostics_result'))
              .fontSize(16)
              .fontWeight(FontWeight.Medium)
              .fontColor(this.themeConfig.colors.textPrimary)
              .margin({ bottom: 12 })

            // Four check items
            Row() {
              this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.network').id), this.syncDiagnostics.networkOk, this.syncDiagnostics.networkStatus)
              this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.auth').id), this.syncDiagnostics.authOk, this.syncDiagnostics.authStatus)
              this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.repository').id), this.syncDiagnostics.repoOk, this.syncDiagnostics.repoStatus)
              this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.branch').id), this.syncDiagnostics.branchOk, this.syncDiagnostics.branchStatus)
            }
            .width('100%')
            .justifyContent(FlexAlign.SpaceAround)
            .margin({ bottom: 12 })

            // User message
            Text(this.syncDiagnostics.userMessage)
              .fontSize(14)
              .fontColor(this.syncDiagnostics.success ? this.themeConfig.colors.success : this.themeConfig.colors.error)
              .margin({ bottom: 8 })

            // Error category
            if (this.syncDiagnostics.errorCategory && this.syncDiagnostics.errorCategory.length > 0) {
              Row() {
                Text(getContext(this).resourceManager.getStringSync($r('app.string.error_category').id) + ': ')
                  .fontSize(12)
                  .fontColor(this.themeConfig.colors.textSecondary)
                Text(this.syncDiagnostics.errorCategory)
                  .fontSize(12)
                  .fontColor(this.themeConfig.colors.error)
              }
              .width('100%')
              .margin({ bottom: 4 })
            }

            // Remote URL
            if (this.syncDiagnostics.remoteUrlSanitized) {
              Row() {
                Text(getContext(this).resourceManager.getStringSync($r('app.string.remote_address').id) + ': ')
                  .fontSize(12)
                  .fontColor(this.themeConfig.colors.textSecondary)
                Text(this.syncDiagnostics.remoteUrlSanitized)
                  .fontSize(12)
                  .fontColor(this.themeConfig.colors.textSecondary)
              }
              .width('100%')
              .margin({ bottom: 4 })
            }

            // Transport
            if (this.syncDiagnostics.transport) {
              Row() {
                Text(getContext(this).resourceManager.getStringSync($r('app.string.transport_protocol').id) + ': ')
                  .fontSize(12)
                  .fontColor(this.themeConfig.colors.textSecondary)
                Text(this.syncDiagnostics.transport)
                  .fontSize(12)
                  .fontColor(this.themeConfig.colors.textSecondary)
              }
              .width('100%')
            }
          }
          .width('100%')
          .padding(12)
          .backgroundColor(this.themeConfig.colors.background)
          .borderRadius(8)
          .margin({ bottom: 16 })
        }
      }
    }
    .width('100%')
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(12)
    .margin({ bottom: 16 })
  }

  // ── AI (ai) ── AI 开关
  @Builder
  AiSettingsSection() {
    Column() {
      Text($r('app.string.ai_section'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })

      Row() {
        Text($r('app.string.enable_ai'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.settings.aiEnabled })
          .onChange((isOn: boolean) => {
            this.settings.aiEnabled = isOn
            this.saveSettings()
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

  // ── 关于/高级 (about) ── 工作区路径、版本信息
  @Builder
  AboutSettingsSection() {
    Column() {
      Text($r('app.string.about_advanced'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })

      // 工作区路径
      Row() {
        Text($r('app.string.workspace_path'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 版本信息
      Row() {
        Text($r('app.string.version_info'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Text($r('app.string.harmonyos_client'))
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
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
  DiagnosticItem(label: string, ok: boolean, status: string) {
    Column() {
      Text(ok ? 'OK' : 'FAIL')
        .fontSize(14)
        .fontWeight(FontWeight.Bold)
        .fontColor(ok ? this.themeConfig.colors.success : this.themeConfig.colors.error)
      Text(label)
        .fontSize(12)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ top: 4 })
      if (status) {
        Text(status)
          .fontSize(10)
          .fontColor(this.themeConfig.colors.textHint)
          .margin({ top: 2 })
      }
    }
    .alignItems(HorizontalAlign.Center)
  }

  // 主题选项顺序：system→light→dark
  getThemeIndex(): number {
    const themes = ['system', 'light', 'dark']
    return themes.indexOf(this.settings.themeMode || 'system')
  }

  getThemeName(): string {
    if (this.settings.themeMode === 'system') return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
    if (this.settings.themeMode === 'light') return getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id)
    if (this.settings.themeMode === 'dark') return getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id)
    return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
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
    return getContext(this).resourceManager.getStringSync($r('app.string.custom').id)
  }
}
