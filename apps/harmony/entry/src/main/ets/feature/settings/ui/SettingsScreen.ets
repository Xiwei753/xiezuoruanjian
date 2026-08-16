import { Routes } from '../../../app/navigation/Routes'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { LocalSettings, SyncableSettings, ThemeMode, ThemePaletteRecordDto, BuiltinThemeDto } from '../../../corebridge/dto/SettingsDtos'
import { SyncConfig, SyncDiagnosticsResult } from '../../../corebridge/dto/SyncDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeUpdateParams, ThemeObserverImpl, ColorSource } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { WidthClass, ShellMode } from '../../../corebridge/dto/PlatformDtos'
import { MessageKeyMapper } from '../../../ui/i18n/MessageKeyMapper'

interface SettingsSelectOption {
  value: string
  icon: string
}

interface SettingsPageParams {
  // per-project sync: projectId selects the sync root. Empty = no project selected.
  projectId?: string
}

@Component
export struct SettingsScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
  @State settings: LocalSettings = {
    themeMode: 'system',
    appearanceMode: 'system',
    colorSource: 'built_in',
    dynamicColorEnabled: false,
    selectedBuiltinThemeId: '',
    selectedPaletteId: '',
    autoSaveEnabled: true,
    editorFontSize: 16,
    editorLineSpacingMultiplier: 1.8,
    windowWidth: 0,
    windowHeight: 0,
    autoSaveDelayMs: 30000,
    autoIndentEnabled: true,
    autoIndentWidth: 2,
    editorTypingAnimationEnabled: false,
    editorSmoothCursorEnabled: false,
    editorTypingAnimationDurationMs: 100,
    editorSmoothCursorDurationMs: 80,
    aiEnabled: false,
    linuxQtSidebarWidth: 0,
    linuxQtEditorWidth: 0
  }
  @State paletteRecords: ThemePaletteRecordDto[] = []
  @State builtinThemes: BuiltinThemeDto[] = []
  @State syncConfig: SyncConfig | null = null
  @State isLoading: boolean = true
  @State isSaving: boolean = false
  @State isSyncing: boolean = false
  @State saveMessage: string = ''
  @State syncMessage: string = ''
  @State syncDiagnostics: SyncDiagnosticsResult | null = null
  @State isDiagnosing: boolean = false
  @State projectId: string = ''
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State adaptiveState: AdaptiveState = getAdaptiveState()

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  aboutToAppear() {
    const self = this
    // 读取路由参数中的 projectId（per-project sync）
    const paramsArr = this.navPathStack.getParamByName(Routes.Settings) as SettingsPageParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params && params.projectId) {
      this.projectId = params.projectId
    }
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
      // 全量同步（Issue #630）：一个全局 SyncConfig，不依赖选中作品。
      const syncConfigResult = await this.bridge.loadSyncConfig()

      try {
        const paletteResult = await this.bridge.listPaletteRecords()
        if (paletteResult.success && paletteResult.data) {
          this.paletteRecords = paletteResult.data
          this.themeAdapter.setPaletteRecords(paletteResult.data)
        }
      } catch (_e) { }

      try {
        const builtinResult = await this.bridge.listBuiltinThemes()
        if (builtinResult.success && builtinResult.data) {
          this.builtinThemes = builtinResult.data
          this.themeAdapter.setBuiltinThemes(builtinResult.data)
        }
      } catch (_e) { }

      if (settingsResult.success && settingsResult.data) {
        this.settings = settingsResult.data
        const appearanceMode = settingsResult.data.appearanceMode || settingsResult.data.themeMode || 'system'
        this.themeAdapter.updateFromLocalSettings(
          appearanceMode,
          settingsResult.data.colorSource || 'built_in',
          settingsResult.data.selectedPaletteId || '',
          settingsResult.data.selectedBuiltinThemeId || ''
        )
        this.themeAdapter.updateFromSettings({
          theme: appearanceMode as ThemeMode,
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
        const appearanceMode = this.settings.appearanceMode || this.settings.themeMode || 'system'
        this.themeAdapter.updateFromLocalSettings(
          appearanceMode,
          this.settings.colorSource || 'built_in',
          this.settings.selectedPaletteId || '',
          this.settings.selectedBuiltinThemeId || ''
        )
        this.themeAdapter.updateFromSettings({
          theme: appearanceMode as ThemeMode,
          fontSize: this.settings.editorFontSize,
          fontFamily: 'HarmonyOS Sans',
          lineHeight: this.settings.editorLineSpacingMultiplier
        } as ThemeUpdateParams)
        setTimeout(() => {
          this.saveMessage = ''
        }, 2000)
      } else {
        this.saveMessage = MessageKeyMapper.resolveMessage(getContext(this), result.messageKey, result.messageArgs, result.errorCode)
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
        const syncResult = result.data
        if (syncResult.overallStatus === 'conflict' || syncResult.overallStatus === 'partial_conflict') {
          this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_sync_conflict').id)
        } else if (syncResult.overallStatus === 'dirty_repo_blocked') {
          this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.sync_dirty_repo_blocked').id)
        } else if (syncResult.overallStatus === 'no_changes') {
          this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.sync_complete_format').id).replace('%s', '0')
          setTimeout(() => { this.syncMessage = '' }, 3000)
        } else if (syncResult.overallStatus === 'recoverable_error' ||
          syncResult.overallStatus === 'fatal_error' ||
          syncResult.overallStatus === 'error' ||
          syncResult.overallStatus === 'syncing') {
          // 失败/未完成：需要用户处理或可重试，展示错误分类文案
          const errorCategory = syncResult.errorCategory
          if (errorCategory && errorCategory.length > 0) {
            this.syncMessage = MessageKeyMapper.resolveMessage(getContext(this), `error.${errorCategory}`, undefined, undefined)
          } else {
            this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.sync_error').id)
          }
        } else {
          this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.sync_complete_format').id).replace('%s', `${syncResult.totalDownloaded}`)
          setTimeout(() => { this.syncMessage = '' }, 3000)
        }
      } else {
        // 优先用 FullSyncResult.errorCategory，其次用 ResultEnvelope.messageKey
        const errorCategory = result.data?.errorCategory
        if (errorCategory && errorCategory.length > 0) {
          this.syncMessage = MessageKeyMapper.resolveMessage(getContext(this), `error.${errorCategory}`, undefined, undefined)
        } else if (result.messageKey && result.messageKey.length > 0) {
          this.syncMessage = MessageKeyMapper.resolveMessage(getContext(this), result.messageKey, result.messageArgs, result.errorCode)
        } else {
          this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.sync_error').id)
        }
      }
    } catch (err) {
      this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.sync_error').id)
    } finally {
      this.isSyncing = false
    }
  }

  async runSyncDiagnostics() {
    this.isDiagnosing = true
    this.syncDiagnostics = null

    try {
      const result = await this.bridge.syncDiagnostics()
      if (result.success && result.data && result.data.diagnostics) {
        this.syncDiagnostics = result.data.diagnostics
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
          userMessage: getContext(this).resourceManager.getStringSync($r('app.string.diagnostics_error').id)
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
          .onClick(() => this.navPathStack.pop())

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
          { value: getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id), icon: '' },
          { value: getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id), icon: '' },
          { value: getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id), icon: '' }
        ] as SettingsSelectOption[])
          .selected(this.getThemeIndex())
          .value(this.getThemeName())
          .onSelect((index: number) => {
            const themes: ThemeMode[] = ['system', 'light', 'dark']
            const selectedTheme: ThemeMode = themes[index]
            this.settings.appearanceMode = selectedTheme
            this.settings.themeMode = selectedTheme
            this.themeAdapter.setMode(selectedTheme)
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 颜色主题来源
      Row() {
        Text('颜色主题')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Select([
          { value: '素笺默认', icon: '' },
          { value: '已保存的设备配色', icon: '' }
        ] as SettingsSelectOption[])
          .selected(this.getColorSourceIndex())
          .value(this.getColorSourceName())
          .onSelect((index: number) => {
            const sources: ColorSource[] = ['built_in', 'saved_palette']
            const selected = sources[index]
            this.settings.colorSource = selected
            this.themeAdapter.setColorSource(selected)
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 内置主题选择
      if (this.settings.colorSource === 'built_in' && this.builtinThemes.length > 0) {
        Row() {
          Text('内置主题')
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

          Select(this.builtinThemes.map(t => ({ value: t.name, icon: '' }) as SettingsSelectOption[]))
            .selected(this.getSelectedBuiltinThemeIndex())
            .value(this.getSelectedBuiltinThemeName())
            .onSelect((index: number) => {
              if (index < this.builtinThemes.length) {
                const themeId = this.builtinThemes[index].theme_id
                this.settings.selectedBuiltinThemeId = themeId
                this.themeAdapter.setSelectedBuiltinThemeId(themeId)
                this.saveSettings()
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })
      }

      // 已保存调色板选择
      if (this.settings.colorSource === 'saved_palette' && this.paletteRecords.length > 0) {
        Row() {
          Text('设备配色')
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

          Select(this.paletteRecords.map(r => {
            const date = new Date(r.captured_at_ms)
            const dateStr = `${date.getFullYear()}-${(date.getMonth() + 1).toString().padStart(2, '0')}-${date.getDate().toString().padStart(2, '0')}`
            return { value: `${r.source_device_class} · ${dateStr}`, icon: '' } as SettingsSelectOption
          }))
            .selected(this.getSelectedPaletteIndex())
            .value(this.getSelectedPaletteName())
            .onSelect((index: number) => {
              if (index < this.paletteRecords.length) {
                const paletteId = this.paletteRecords[index].palette_id
                this.settings.selectedPaletteId = paletteId
                this.themeAdapter.setSelectedPaletteId(paletteId)
                this.saveSettings()
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })
      }

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
    .borderRadius(DesignTokens.radiusMd)
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

      // 打字动画 — 按 Issue #629 静态编辑链路有意不做动画（motion 层后续新增，不改输入层/session 层），开关禁用
      Row() {
        Text($r('app.string.typing_animation'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textSecondary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: false, enabled: false })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 打字动画时长 — 按 Issue #629 静态链路有意不做动画，不显示时长设置
      // motion 层后续新增时接入 this.settings.editorTypingAnimationDurationMs

      // 平滑光标 — 按 Issue #629 静态编辑链路有意不做光标动画，开关禁用
      Row() {
        Text($r('app.string.smooth_cursor'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textSecondary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: false, enabled: false })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 平滑光标时长 — 按 Issue #629 静态链路有意不做光标动画，不显示时长设置
      // motion 层后续新增时接入 this.settings.editorSmoothCursorDurationMs
    }
    .width('100%')
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(DesignTokens.radiusMd)
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
    .borderRadius(DesignTokens.radiusMd)
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
            { value: getContext(this).resourceManager.getStringSync($r('app.string.custom').id), icon: '' }
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

        // Sync actions（全量同步：全局配置，不依赖选中作品）
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
            .backgroundColor(this.themeConfig.colors.secondaryContainer)
            .fontColor(this.themeConfig.colors.onSecondaryContainer)
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

            // User message — 优先用 errorCategory 走 MessageKeyMapper，否则直接显示 userMessage
            Text(this.syncDiagnostics.errorCategory && this.syncDiagnostics.errorCategory.length > 0
              ? MessageKeyMapper.resolveMessage(getContext(this), `error.${this.syncDiagnostics.errorCategory}`, undefined, undefined)
              : (this.syncDiagnostics.userMessage ?? ''))
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
          .borderRadius(DesignTokens.radiusSm)
          .margin({ bottom: 16 })
        }
      }
    }
    .width('100%')
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(DesignTokens.radiusMd)
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
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 16 })
  }

  // ── 关于/高级 (about) ── 应用名、作者、项目地址、开源协议、镜像说明、版本信息、工作区路径
  @Builder
  AboutSettingsSection() {
    Column() {
      Text($r('app.string.about_title'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 16 })

      // 应用名
      Row() {
        Text($r('app.string.about_app_name_label'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Text($r('app.string.about_app_name'))
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 作者
      Row() {
        Text($r('app.string.about_author_label'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Text($r('app.string.about_author'))
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 项目地址
      Column() {
        Text($r('app.string.about_project_label'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ bottom: 4 })
        Text('GitHub: github.com/Xiwei753/xiezuoruanjian')
          .fontSize(14)
          .fontColor(this.themeConfig.colors.primary)
        Text($r('app.string.about_gitee'))
          .fontSize(14)
          .fontColor(this.themeConfig.colors.primary)
          .margin({ top: 4 })
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .margin({ bottom: 8 })

      // 开源协议
      Row() {
        Text($r('app.string.about_license_label'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Text($r('app.string.about_license'))
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 镜像说明
      Text($r('app.string.about_mirror_note'))
        .fontSize(13)
        .fontColor(this.themeConfig.colors.textMuted)
        .margin({ bottom: 8 })

      // 版本信息
      Row() {
        Text($r('app.string.version_info'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Text($r('app.string.about_platform'))
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
      }
      .width('100%')

      // 工作区路径
      Row() {
        Text($r('app.string.workspace_path'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
      }
      .width('100%')
      .margin({ top: 8 })
    }
    .width('100%')
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(DesignTokens.radiusMd)
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
    const mode = this.settings.appearanceMode || this.settings.themeMode || 'system'
    const themes = ['system', 'light', 'dark']
    return themes.indexOf(mode)
  }

  getThemeName(): string {
    const mode = this.settings.appearanceMode || this.settings.themeMode || 'system'
    if (mode === 'system') return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
    if (mode === 'light') return getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id)
    if (mode === 'dark') return getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id)
    return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
  }

  getColorSourceIndex(): number {
    const sources: string[] = ['built_in', 'saved_palette']
    return sources.indexOf(this.settings.colorSource || 'built_in')
  }

  getColorSourceName(): string {
    const s = this.settings.colorSource || 'built_in'
    if (s === 'saved_palette') return '已保存的设备配色'
    return '素笺默认'
  }

  getSelectedBuiltinThemeIndex(): number {
    const id = this.settings.selectedBuiltinThemeId || ''
    if (id.length === 0) return 0
    return this.builtinThemes.findIndex(t => t.theme_id === id)
  }

  getSelectedBuiltinThemeName(): string {
    const id = this.settings.selectedBuiltinThemeId || ''
    if (id.length === 0) return '素笺默认'
    const theme = this.builtinThemes.find(t => t.theme_id === id)
    return theme ? theme.name : '素笺默认'
  }

  getSelectedPaletteIndex(): number {
    const id = this.settings.selectedPaletteId || ''
    if (id.length === 0) return 0
    return this.paletteRecords.findIndex(r => r.palette_id === id)
  }

  getSelectedPaletteName(): string {
    const id = this.settings.selectedPaletteId || ''
    if (id.length === 0 && this.paletteRecords.length > 0) {
      const r = this.paletteRecords[0]
      const date = new Date(r.captured_at_ms)
      return `${r.source_device_class} · ${date.getFullYear()}-${(date.getMonth() + 1).toString().padStart(2, '0')}-${date.getDate().toString().padStart(2, '0')}`
    }
    const record = this.paletteRecords.find(r => r.palette_id === id)
    if (record) {
      const date = new Date(record.captured_at_ms)
      return `${record.source_device_class} · ${date.getFullYear()}-${(date.getMonth() + 1).toString().padStart(2, '0')}-${date.getDate().toString().padStart(2, '0')}`
    }
    return ''
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
