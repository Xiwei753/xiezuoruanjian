import hilog from '@ohos.hilog'
import { LocalSettings, ThemePaletteRecordDto, BuiltinThemeDto } from '../../../corebridge/dto/SettingsDtos'
import { SyncConfig, SyncDiagnosticsResult } from '../../../corebridge/dto/SyncDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl, ColorSource, ThemeMode } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { MessageKeyMapper } from '../../../ui/i18n/MessageKeyMapper'
import { SettingsViewModel, SettingsSectionId } from '../presentation/SettingsViewModel'

// ── 设置分组标识 ──
type SettingsGroupId = 'appearance' | 'writing' | 'data_sync' | 'advanced' | 'about'

// ── 分组与分类的映射关系（对齐 Android SettingsRoute.kt 信息架构） ──
interface GroupDefinition {
  id: SettingsGroupId
  sections: SettingsSectionId[]
}

const GROUP_DEFINITIONS: GroupDefinition[] = [
  { id: 'appearance', sections: ['appearance'] },
  { id: 'writing', sections: ['editor', 'save'] },
  { id: 'data_sync', sections: ['sync', 'recovery'] },
  { id: 'advanced', sections: ['ai', 'diagnostics', 'laboratory'] },
  { id: 'about', sections: ['about'] }
]

// ── 下拉选择选项类型 ──
interface SettingsSelectOption {
  value: string
  icon: string
}

@ComponentV2
export struct SettingsScreen {
  // navPathStack 通过 NavigationTransactionCoordinator 获取，不使用 @Consume（V1 装饰器）。
  private navCoordinator: NavigationTransactionCoordinator = NavigationTransactionCoordinator.getInstance()

  // ViewModel — 所有 Core/NAPI 调用和状态管理都在这里
  private viewModel: SettingsViewModel = new SettingsViewModel()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  // ── 页面 UI 状态（V2 @Local） ──
  @Local themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @Local adaptiveState: AdaptiveState = getAdaptiveState()
  @Local isLoading: boolean = true
  @Local isSaving: boolean = false
  @Local isSyncing: boolean = false
  @Local isDiagnosing: boolean = false
  @Local saveMessage: string = ''
  @Local syncMessage: string = ''
  @Local syncDiagnostics: SyncDiagnosticsResult | null = null
  @Local searchText: string = ''

  // ── 设置数据（从 ViewModel 同步到页面状态） ──
  @Local settings: LocalSettings = {
    themeMode: 'system',
    appearanceMode: 'system',
    colorSource: 'built_in',
    dynamicColorEnabled: false,
    selectedBuiltinThemeId: '',
    selectedPaletteId: '',
    locale: null,
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
    statsDeviceId: null,
    desktopSidebarWidth: 0,
    desktopEditorWidth: 0,
    editorCoordinatedTextCursorAnimationEnabled: false,
    diagnosticsEnabled: false,
    diagnosticsVerbose: false
  }
  @Local paletteRecords: ThemePaletteRecordDto[] = []
  @Local builtinThemes: BuiltinThemeDto[] = []
  @Local syncConfig: SyncConfig | null = null

  // ── 折叠展开状态（每个分类独立，允许多个同时展开） ──
  @Local expandedSections: Map<SettingsSectionId, boolean> = new Map()

  aboutToAppear() {
    hilog.info(0x0000, 'SettingsScreen', 'route name=Settings')

    // 注册主题观察者
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      this.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    // 注册响应式布局观察者
    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      this.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver)
    this.adaptiveState = AdaptiveContext.getInstance().getCurrentState()

    // 从 ViewModel 恢复展开状态（ViewModel 持久保留，重新进入设置页时不重置）
    this.expandedSections = new Map<SettingsSectionId, boolean>(this.viewModel.getExpandedSections())

    // 加载数据
    this.loadData()
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  // ── 从 ViewModel 加载数据并同步到页面状态 ──
  async loadData(): Promise<void> {
    this.isLoading = true
    await this.viewModel.loadData()
    this.syncFromViewModel()
    this.isLoading = false
  }

  // ── 从 ViewModel 保存设置 ──
  async saveSettings(): Promise<void> {
    this.isSaving = true
    this.saveMessage = ''
    await this.viewModel.saveSettings()
    this.syncFromViewModel()
    this.isSaving = false

    // 解析 saveMessage
    const msg = this.viewModel.getSaveMessage()
    if (msg === 'saved') {
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.settings_saved').id)
      setTimeout(() => { this.saveMessage = '' }, 2000)
    } else if (msg.startsWith('error:')) {
      const errorCode = msg.substring(6)
      this.saveMessage = MessageKeyMapper.resolveMessage(getContext(this), undefined, undefined, errorCode)
    }
  }

  // ── 执行同步 ──
  async performSync(): Promise<void> {
    this.isSyncing = true
    this.syncMessage = ''
    await this.viewModel.performSync()
    this.syncFromViewModel()
    this.isSyncing = false

    // 解析 syncMessage
    const msg = this.viewModel.getSyncMessage()
    if (msg.startsWith('sync_complete:')) {
      const count = msg.substring('sync_complete:'.length)
      this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.sync_complete_format').id).replace('%s', count)
      setTimeout(() => { this.syncMessage = '' }, 3000)
    } else if (msg.startsWith('error:')) {
      const errorKey = msg.substring(6)
      if (errorKey === 'sync_conflict') {
        this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_sync_conflict').id)
      } else if (errorKey === 'dirty_repo_blocked') {
        this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.sync_dirty_repo_blocked').id)
      } else if (errorKey === 'sync_error') {
        this.syncMessage = getContext(this).resourceManager.getStringSync($r('app.string.sync_error').id)
      } else {
        this.syncMessage = MessageKeyMapper.resolveMessage(getContext(this), `error.${errorKey}`, undefined, undefined)
      }
    }
  }

  // ── 运行同步诊断 ──
  async runSyncDiagnostics(): Promise<void> {
    this.isDiagnosing = true
    this.syncDiagnostics = null
    await this.viewModel.runSyncDiagnostics()
    this.syncDiagnostics = this.viewModel.getSyncDiagnostics()

    // 如果诊断结果中的 rawError 是资源 key，解析为本地化文案
    if (this.syncDiagnostics && !this.syncDiagnostics.success && this.syncDiagnostics.rawError) {
      const rawErr = this.syncDiagnostics.rawError
      if (rawErr === 'diagnostics_error') {
        this.syncDiagnostics.rawError = getContext(this).resourceManager.getStringSync($r('app.string.diagnostics_error').id)
      } else if (rawErr === 'diagnostics_error_detail') {
        this.syncDiagnostics.rawError = getContext(this).resourceManager.getStringSync($r('app.string.diagnostics_error_detail').id)
      }
    }
    this.isDiagnosing = false
  }

  // ── 从 ViewModel 同步数据到页面状态 ──
  private syncFromViewModel(): void {
    this.settings = this.viewModel.getSettings()
    this.paletteRecords = this.viewModel.getPaletteRecords()
    this.builtinThemes = this.viewModel.getBuiltinThemes()
    this.syncConfig = this.viewModel.getSyncConfig()
  }

  // ── 切换分类展开/收起 ──
  // 状态持久保留在 ViewModel 中；@Local Map 创建新引用以触发 V2 观察更新。
  toggleSection(section: SettingsSectionId): void {
    this.viewModel.toggleSection(section)
    this.expandedSections = new Map<SettingsSectionId, boolean>(this.viewModel.getExpandedSections())
  }

  isSectionExpanded(section: SettingsSectionId): boolean {
    return this.expandedSections.get(section) ?? false
  }

  // ── 安全返回 ──
  private goBack(): void {
    this.navCoordinator.safePop()
  }

  build() {
    // Issue #752：NavDestination 作为 Navigation 目标页根节点，否则实机白屏。
    NavDestination() {
      Column() {
        // ── 顶部：返回按钮 + 搜索框 ──
        this.SettingsHeader()

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
              // 按分组渲染：外观→写作→数据与同步→高级→关于
              this.SettingsGroupList()
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
    .hideTitleBar(true)
  }

  // ── 顶部 Header：返回按钮 + 搜索框 ──
  @Builder
  SettingsHeader() {
    Column() {
      // 返回按钮行
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(() => this.goBack())

        Text($r('app.string.settings'))
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ left: 12 })

        Blank()

        // 保存状态提示
        if (this.saveMessage) {
          Text(this.saveMessage)
            .fontSize(12)
            .fontColor(this.saveMessage === getContext(this).resourceManager.getStringSync($r('app.string.settings_saved').id)
              ? this.themeConfig.colors.success : this.themeConfig.colors.error)
        }
      }
      .width('100%')
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 16, bottom: 8 })

      // 搜索框（仅 UI，不实现搜索逻辑）
      Row() {
        Image($r('sys.media.ohos_ic_public_search'))
          .width(20)
          .height(20)
          .fillColor(this.themeConfig.colors.textHint)
          .margin({ left: 12 })

        TextInput({ placeholder: $r('app.string.settings'), text: this.searchText })
          .layoutWeight(1)
          .height(36)
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textPrimary)
          .placeholderColor(this.themeConfig.colors.textHint)
          .backgroundColor(Color.Transparent)
          .onChange((value: string) => {
            this.searchText = value
          })
      }
      .width('100%')
      .height(40)
      .margin({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, bottom: 8 })
      .backgroundColor(this.themeConfig.colors.surfaceContainerLow)
      .borderRadius(DesignTokens.radiusSm)
    }
    .width('100%')
    .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
  }

  // ── 分组列表 ──
  @Builder
  SettingsGroupList() {
    ForEach(GROUP_DEFINITIONS, (groupDef: GroupDefinition) => {
      Column() {
        // 分组标题
        this.SettingsGroupHeader(groupDef.id)

        // 分组内各分类（可独立展开折叠项）
        ForEach(groupDef.sections, (sectionId: SettingsSectionId) => {
          this.ExpandableCategory(sectionId)
        }, (sectionId: SettingsSectionId) => sectionId)
      }
      .width('100%')
      .margin({ bottom: 16 })
    }, (groupDef: GroupDefinition) => groupDef.id)
  }

  // ── 分组标题 ──
  @Builder
  SettingsGroupHeader(groupId: SettingsGroupId) {
    Text(this.getGroupTitle(groupId))
      .fontSize(14)
      .fontWeight(FontWeight.Medium)
      .fontColor(this.themeConfig.colors.textSecondary)
      .margin({ bottom: 8, top: 8 })
      .width('100%')
  }

  // ── 可展开折叠分类 ──
  @Builder
  ExpandableCategory(sectionId: SettingsSectionId) {
    Column() {
      // 分类标题行（点击展开/收起）
      Row() {
        Text(this.getSectionTitle(sectionId))
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        // 当前值摘要
        Text(this.getSectionValueSummary(sectionId))
          .fontSize(14)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 8 })

        // 展开/收起箭头
        Image($r('sys.media.ohos_ic_public_arrow_down'))
          .width(20)
          .height(20)
          .fillColor(this.themeConfig.colors.textHint)
          .rotate({ angle: this.isSectionExpanded(sectionId) ? 180 : 0 })
          .animation({ duration: 150, curve: Curve.EaseInOut })
      }
      .width('100%')
      .height(48)
      .padding({ left: 16, right: 16 })
      .onClick(() => this.toggleSection(sectionId))

      // 分类内容（展开时显示）
      if (this.isSectionExpanded(sectionId)) {
        Column() {
          this.SectionContent(sectionId)
        }
        .width('100%')
        .padding({ left: 16, right: 16, bottom: 12 })
        .animation({ duration: 150, curve: Curve.EaseInOut })
      }
    }
    .width('100%')
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 8 })
  }

  // ── 分类内容路由 ──
  @Builder
  SectionContent(sectionId: SettingsSectionId) {
    if (sectionId === 'appearance') {
      this.AppearanceContent()
    } else if (sectionId === 'editor') {
      this.EditorContent()
    } else if (sectionId === 'save') {
      this.SaveContent()
    } else if (sectionId === 'sync') {
      this.SyncContent()
    } else if (sectionId === 'recovery') {
      this.RecoveryContent()
    } else if (sectionId === 'ai') {
      this.AiContent()
    } else if (sectionId === 'diagnostics') {
      this.DiagnosticsContent()
    } else if (sectionId === 'laboratory') {
      this.LaboratoryContent()
    } else if (sectionId === 'about') {
      this.AboutContent()
    }
  }

  // ════════════════════════════════════════════
  // 外观设置内容
  // ════════════════════════════════════════════
  @Builder
  AppearanceContent() {
    Column() {
      // 主题模式 — 顺序必须是 跟随系统→浅色→深色
      Row() {
        Text($r('app.string.theme_mode'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Select(this.getAppearanceOptions())
          .selected(this.getThemeIndex())
          .value(this.getThemeName())
          .onSelect((index: number) => {
            const themes: ThemeMode[] = ['system', 'light', 'dark']
            const selectedTheme: ThemeMode = themes[index]
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.appearanceMode = selectedTheme
              s.themeMode = selectedTheme
              return s
            })
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

        Select(this.getColorSourceOptions())
          .selected(this.getColorSourceIndex())
          .value(this.getColorSourceName())
          .onSelect((index: number) => {
            const sources: ColorSource[] = ['built_in', 'saved_palette']
            const selected = sources[index]
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.colorSource = selected
              return s
            })
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

          Select(this.getBuiltinThemeOptions())
            .selected(this.getSelectedBuiltinThemeIndex())
            .value(this.getSelectedBuiltinThemeName())
            .onSelect((index: number) => {
              if (index < this.builtinThemes.length) {
                const themeId = this.builtinThemes[index].theme_id
                this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                  s.selectedBuiltinThemeId = themeId
                  return s
                })
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

          Select(this.paletteRecords.map((r: ThemePaletteRecordDto): SettingsSelectOption => {
            const date = new Date(r.captured_at_ms)
            const dateStr = `${date.getFullYear()}-${(date.getMonth() + 1).toString().padStart(2, '0')}-${date.getDate().toString().padStart(2, '0')}`
            return { value: `${r.source_device_class} · ${dateStr}`, icon: '' }
          }))
            .selected(this.getSelectedPaletteIndex())
            .value(this.getSelectedPaletteName())
            .onSelect((index: number) => {
              if (index < this.paletteRecords.length) {
                const paletteId = this.paletteRecords[index].palette_id
                this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                  s.selectedPaletteId = paletteId
                  return s
                })
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
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorFontSize = value
              return s
            })
            this.settings = this.viewModel.getSettings()
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
            const rounded = Math.round(value * 10) / 10
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorLineSpacingMultiplier = rounded
              return s
            })
            this.settings = this.viewModel.getSettings()
            this.saveSettings()
          })
      }
      .width('100%')
    }
  }

  // ════════════════════════════════════════════
  // 写作区设置内容
  // ════════════════════════════════════════════
  @Builder
  EditorContent() {
    Column() {
      // 自动缩进
      Row() {
        Text($r('app.string.auto_indent'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.settings.autoIndentEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoIndentEnabled = isOn
              return s
            })
            this.settings = this.viewModel.getSettings()
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
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoIndentWidth = value
              return s
            })
            this.settings = this.viewModel.getSettings()
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 打字动画 — 按 Issue #629 静态编辑链路有意不做动画，开关禁用
      Row() {
        Text($r('app.string.typing_animation'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textSecondary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: false })
          .enabled(false)
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 平滑光标 — 按 Issue #629 静态编辑链路有意不做光标动画，开关禁用
      Row() {
        Text($r('app.string.smooth_cursor'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textSecondary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: false })
          .enabled(false)
      }
      .width('100%')
    }
  }

  // ════════════════════════════════════════════
  // 保存设置内容
  // ════════════════════════════════════════════
  @Builder
  SaveContent() {
    Column() {
      // 自动保存
      Row() {
        Text($r('app.string.auto_save'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.settings.autoSaveEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoSaveEnabled = isOn
              return s
            })
            this.settings = this.viewModel.getSettings()
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
          Text(getContext(this).resourceManager.getStringSync($r('app.string.seconds_format').id)
            .replace('%s', `${Math.round(this.settings.autoSaveDelayMs / 1000)}`))
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ right: 16 })
          Slider({ value: this.settings.autoSaveDelayMs / 1000, min: 1, max: 10, step: 1 })
            .width(120)
            .onChange((value: number) => {
              this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                s.autoSaveDelayMs = value * 1000
                return s
              })
              this.settings = this.viewModel.getSettings()
              this.saveSettings()
            })
        }
        .width('100%')
      }
    }
  }

  // ════════════════════════════════════════════
  // 同步设置内容
  // ════════════════════════════════════════════
  @Builder
  SyncContent() {
    Column() {
      // 同步状态提示
      if (this.syncMessage) {
        Row() {
          Text(this.syncMessage)
            .fontSize(12)
            .fontColor(this.syncMessage.includes(getContext(this).resourceManager.getStringSync($r('app.string.sync_complete_format').id).split('%s')[0])
              ? this.themeConfig.colors.success : this.themeConfig.colors.error)
        }
        .width('100%')
        .margin({ bottom: 12 })
      }

      // Sync enabled
      Row() {
        Text($r('app.string.enable_sync'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.syncConfig?.enabled || false })
          .onChange((isOn: boolean) => {
            if (this.syncConfig) {
              const newConfig: SyncConfig = {
                enabled: isOn,
                activeProvider: this.syncConfig.activeProvider,
                providerConfig: this.syncConfig.providerConfig,
                autoSync: this.syncConfig.autoSync,
                syncIntervalSeconds: this.syncConfig.syncIntervalSeconds,
                hasNetworkPermission: this.syncConfig.hasNetworkPermission,
                hasNetworkStatePermission: this.syncConfig.hasNetworkStatePermission
              }
              this.syncConfig = newConfig
            }
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Sync 详细配置（仅在同步开启时显示）
      if (this.syncConfig?.enabled) {
        // Sync backend type
        Row() {
          Text($r('app.string.sync_method'))
            .fontSize(16)
            .fontColor(this.themeConfig.colors.textPrimary)
            .layoutWeight(1)

          Select(this.getSyncProviderOptions())
            .selected(this.getSyncProviderIndex())
            .value(this.getSyncProviderName())
            .onSelect((index: number) => {
              if (this.syncConfig) {
                const providers = ['github', 'gitlab', 'gitea', 'custom']
                const newConfig: SyncConfig = {
                  enabled: this.syncConfig.enabled,
                  activeProvider: providers[index],
                  providerConfig: this.syncConfig.providerConfig,
                  autoSync: this.syncConfig.autoSync,
                  syncIntervalSeconds: this.syncConfig.syncIntervalSeconds,
                  hasNetworkPermission: this.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.syncConfig.hasNetworkStatePermission
                }
                this.syncConfig = newConfig
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
                const newConfig: SyncConfig = {
                  enabled: this.syncConfig.enabled,
                  activeProvider: this.syncConfig.activeProvider,
                  providerConfig: this.syncConfig.providerConfig,
                  autoSync: isOn,
                  syncIntervalSeconds: this.syncConfig.syncIntervalSeconds,
                  hasNetworkPermission: this.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.syncConfig.hasNetworkStatePermission
                }
                this.syncConfig = newConfig
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

          Text(getContext(this).resourceManager.getStringSync($r('app.string.seconds_format').id)
            .replace('%s', `${this.syncConfig?.syncIntervalSeconds || 300}`))
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
                const newConfig: SyncConfig = {
                  enabled: this.syncConfig.enabled,
                  activeProvider: this.syncConfig.activeProvider,
                  providerConfig: this.syncConfig.providerConfig,
                  autoSync: this.syncConfig.autoSync,
                  syncIntervalSeconds: value,
                  hasNetworkPermission: this.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.syncConfig.hasNetworkStatePermission
                }
                this.syncConfig = newConfig
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
          this.DiagnosticsResultCard()
        }
      }
    }
  }

  // ── 诊断结果卡片 ──
  @Builder
  DiagnosticsResultCard() {
    Column() {
      Text($r('app.string.diagnostics_result'))
        .fontSize(16)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.textPrimary)
        .margin({ bottom: 12 })

      // Four check items
      Row() {
        this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.network').id),
          this.syncDiagnostics!.networkOk, this.syncDiagnostics!.networkStatus)
        this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.auth').id),
          this.syncDiagnostics!.authOk, this.syncDiagnostics!.authStatus)
        this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.repository').id),
          this.syncDiagnostics!.remoteOk, this.syncDiagnostics!.networkStatus)
      }
      .width('100%')
      .justifyContent(FlexAlign.SpaceAround)
      .margin({ bottom: 12 })

      // User message
      Text(this.syncDiagnostics!.errorCategory && this.syncDiagnostics!.errorCategory.length > 0
        ? MessageKeyMapper.resolveMessage(getContext(this), `error.${this.syncDiagnostics!.errorCategory}`, undefined, undefined)
        : (this.syncDiagnostics!.rawError ?? this.syncDiagnostics!.providerDetails ?? ''))
        .fontSize(14)
        .fontColor(this.syncDiagnostics!.success ? this.themeConfig.colors.success : this.themeConfig.colors.error)
        .margin({ bottom: 8 })

      // Error category
      if (this.syncDiagnostics!.errorCategory && this.syncDiagnostics!.errorCategory.length > 0) {
        Row() {
          Text(getContext(this).resourceManager.getStringSync($r('app.string.error_category').id) + ': ')
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textSecondary)
          Text(this.syncDiagnostics!.errorCategory)
            .fontSize(12)
            .fontColor(this.themeConfig.colors.error)
        }
        .width('100%')
        .margin({ bottom: 4 })
      }
    }
    .width('100%')
    .padding(12)
    .backgroundColor(this.themeConfig.colors.background)
    .borderRadius(DesignTokens.radiusSm)
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

  // ════════════════════════════════════════════
  // 恢复内容（占位）
  // ════════════════════════════════════════════
  @Builder
  RecoveryContent() {
    Column() {
      Text('恢复功能即将上线')
        .fontSize(14)
        .fontColor(this.themeConfig.colors.textHint)
    }
    .width('100%')
  }

  // ════════════════════════════════════════════
  // AI 设置内容
  // ════════════════════════════════════════════
  @Builder
  AiContent() {
    Column() {
      Row() {
        Text($r('app.string.enable_ai'))
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.settings.aiEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.aiEnabled = isOn
              return s
            })
            this.settings = this.viewModel.getSettings()
            this.saveSettings()
          })
      }
      .width('100%')
    }
  }

  // ════════════════════════════════════════════
  // 诊断与日志内容
  // ════════════════════════════════════════════
  @Builder
  DiagnosticsContent() {
    Column() {
      // 开启诊断日志
      Row() {
        Text('开启诊断日志')
          .fontSize(16)
          .fontColor(this.themeConfig.colors.textPrimary)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.settings.diagnosticsEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.diagnosticsEnabled = isOn
              if (!isOn) {
                s.diagnosticsVerbose = false
              }
              return s
            })
            this.settings = this.viewModel.getSettings()
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 详细日志
      Row() {
        Text('详细日志')
          .fontSize(16)
          .fontColor(this.settings.diagnosticsEnabled ? this.themeConfig.colors.textPrimary : this.themeConfig.colors.textHint)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.settings.diagnosticsVerbose })
          .enabled(this.settings.diagnosticsEnabled)
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.diagnosticsVerbose = isOn
              return s
            })
            this.settings = this.viewModel.getSettings()
            this.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 导出诊断包
      Button('导出诊断包')
        .fontSize(14)
        .height(36)
        .width('100%')
        .backgroundColor(this.themeConfig.colors.secondaryContainer)
        .fontColor(this.themeConfig.colors.onSecondaryContainer)
        .margin({ bottom: 12 })
        .onClick(() => {
          this.viewModel.exportDiagnostics()
        })

      // 清空日志
      Button('清空日志')
        .fontSize(14)
        .height(36)
        .width('100%')
        .backgroundColor(this.themeConfig.colors.secondaryContainer)
        .fontColor(this.themeConfig.colors.onSecondaryContainer)
        .margin({ bottom: 12 })
        .onClick(() => {
          this.viewModel.clearLogs()
        })

      // 复制设备信息
      Button('复制设备信息')
        .fontSize(14)
        .height(36)
        .width('100%')
        .backgroundColor(this.themeConfig.colors.secondaryContainer)
        .fontColor(this.themeConfig.colors.onSecondaryContainer)
        .onClick(() => {
          this.viewModel.getDeviceInfoJson()
        })
    }
    .width('100%')
  }

  // ════════════════════════════════════════════
  // 实验室内容（占位）
  // ════════════════════════════════════════════
  @Builder
  LaboratoryContent() {
    Column() {
      Text('实验室功能即将上线')
        .fontSize(14)
        .fontColor(this.themeConfig.colors.textHint)
    }
    .width('100%')
  }

  // ════════════════════════════════════════════
  // 关于内容
  // ════════════════════════════════════════════
  @Builder
  AboutContent() {
    Column() {
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
        .fontColor(this.themeConfig.colors.textHint)
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
  }

  // ════════════════════════════════════════════
  // 辅助方法 — 分组标题
  // ════════════════════════════════════════════
  getGroupTitle(groupId: SettingsGroupId): string {
    switch (groupId) {
      case 'appearance':
        return getContext(this).resourceManager.getStringSync($r('app.string.appearance').id)
      case 'writing':
        return '写作'
      case 'data_sync':
        return '数据与同步'
      case 'advanced':
        return '高级'
      case 'about':
        return getContext(this).resourceManager.getStringSync($r('app.string.about_title').id)
      default:
        return ''
    }
  }

  // ── 分类标题 ──
  getSectionTitle(sectionId: SettingsSectionId): string {
    switch (sectionId) {
      case 'appearance':
        return getContext(this).resourceManager.getStringSync($r('app.string.appearance').id)
      case 'editor':
        return getContext(this).resourceManager.getStringSync($r('app.string.editor').id)
      case 'save':
        return getContext(this).resourceManager.getStringSync($r('app.string.save_section').id)
      case 'sync':
        return getContext(this).resourceManager.getStringSync($r('app.string.sync_section').id)
      case 'recovery':
        return '恢复'
      case 'ai':
        return getContext(this).resourceManager.getStringSync($r('app.string.ai_section').id)
      case 'diagnostics':
        return '诊断与日志'
      case 'laboratory':
        return '实验室'
      case 'about':
        return getContext(this).resourceManager.getStringSync($r('app.string.about_title').id)
      default:
        return ''
    }
  }

  // ── 分类当前值摘要 ──
  getSectionValueSummary(sectionId: SettingsSectionId): string {
    switch (sectionId) {
      case 'appearance': {
        const mode = this.settings.appearanceMode || this.settings.themeMode || 'system'
        if (mode === 'light') return getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id)
        if (mode === 'dark') return getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id)
        return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
      }
      case 'editor':
        return `${this.settings.editorFontSize}px`
      case 'save':
        return this.settings.autoSaveEnabled ? '开启' : '关闭'
      case 'sync':
        return this.syncConfig?.enabled ? '开启' : '关闭'
      case 'recovery':
        return ''
      case 'ai':
        return this.settings.aiEnabled ? '开启' : '关闭'
      case 'diagnostics':
        return this.settings.diagnosticsEnabled ? '开启' : '关闭'
      case 'laboratory':
        return ''
      case 'about':
        return ''
      default:
        return ''
    }
  }

  // ── 下拉选项辅助方法 ──
  getAppearanceOptions(): SettingsSelectOption[] {
    return [
      { value: getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id), icon: '' },
      { value: getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id), icon: '' },
      { value: getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id), icon: '' }
    ]
  }

  getColorSourceOptions(): SettingsSelectOption[] {
    return [
      { value: '素笺默认', icon: '' },
      { value: '已保存的设备配色', icon: '' }
    ]
  }

  getBuiltinThemeOptions(): SettingsSelectOption[] {
    return this.builtinThemes.map((theme: BuiltinThemeDto): SettingsSelectOption => {
      return { value: theme.name, icon: '' }
    })
  }

  getSyncProviderOptions(): SettingsSelectOption[] {
    return [
      { value: 'GitHub', icon: '' },
      { value: 'GitLab', icon: '' },
      { value: 'Gitea', icon: '' },
      { value: getContext(this).resourceManager.getStringSync($r('app.string.custom').id), icon: '' }
    ]
  }

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
    return providers.indexOf(this.syncConfig?.activeProvider || 'github')
  }

  getSyncProviderName(): string {
    const p: string = this.syncConfig?.activeProvider || 'github'
    if (p === 'github') return 'GitHub'
    if (p === 'gitlab') return 'GitLab'
    if (p === 'gitea') return 'Gitea'
    return getContext(this).resourceManager.getStringSync($r('app.string.custom').id)
  }
}
