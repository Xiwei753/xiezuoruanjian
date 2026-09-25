// SettingsScreen — 设置页。
// Issue #760 评论 5822350374：
// - 问题3：宽屏恢复 #760 之前的完整设置列表布局，窄屏保留折叠分类布局
// - 问题9：改为 @ComponentV2 + @Local viewModel，页面直接观察 ViewModel 的 @Trace 字段
// - 问题4：折叠状态通过 ViewModel 的 AppStorageV2 持久化
// - 问题5：复制设备信息按钮调用 viewModel.copyDeviceInfoToClipboard()
// - 问题6：清空日志按钮调用 viewModel.clearLogs()（ViewModel 内部同时清 Rust + HiLog）
// 页面只读取 viewModel 的 @Trace 字段和派发事件，不维护状态副本。

import hilog from '@ohos.hilog'
import { LocalSettings, ThemePaletteRecordDto, BuiltinThemeDto } from '../../../corebridge/dto/SettingsDtos'
import { SyncConfig, SyncDiagnosticsResult } from '../../../corebridge/dto/SyncDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl, ColorSource, ThemeMode } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { MessageKeyMapper } from '../../../ui/i18n/MessageKeyMapper'
import { SettingsViewModel, SettingsSectionId } from '../presentation/SettingsViewModel'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { ScreenRole, WidthClass } from '../../../corebridge/dto/PlatformDtos'

// ── 设置分组标识（窄屏折叠布局用） ──
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

  // 问题9：ViewModel — 所有 Core/NAPI 调用和状态管理都在这里（@ObservedV2 + @Trace）
  @Local viewModel: SettingsViewModel = new SettingsViewModel()

  // 页面 UI 状态（V2 @Local）
  @Local themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @Local adaptiveState: AdaptiveState = getAdaptiveState()
  @Local searchText: string = ''
  // 搜索过滤后的分组列表（窄屏搜索用）
  @Local filteredGroups: GroupDefinition[] = GROUP_DEFINITIONS
  // 问题5：复制设备信息结果提示
  @Local copyMessage: string = ''

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

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

    // 加载数据 — ViewModel 内部管理 async 和状态
    this.viewModel.loadData()
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  // ── 安全返回 ──
  private goBack(): void {
    this.navCoordinator.safePop()
  }

  // ── 搜索过滤设置项（窄屏搜索用） ──
  // 匹配分组标题（getGroupTitle）、分类标题（getSectionTitle）和具体设置项标题（getSectionSearchTerms），
  // 搜索文本匹配到某个分类时，自动展开该分类。
  private filterSettings(searchText: string): void {
    if (searchText.length === 0) {
      this.filteredGroups = GROUP_DEFINITIONS
      return
    }

    const keyword = searchText.toLowerCase()
    const result: GroupDefinition[] = []

    for (const groupDef of GROUP_DEFINITIONS) {
      const groupTitle = this.getGroupTitle(groupDef.id).toLowerCase()
      let groupMatched = groupTitle.includes(keyword)

      // 检查分组内各分类标题及具体设置项标题是否匹配
      const matchedSections: SettingsSectionId[] = []
      for (const sectionId of groupDef.sections) {
        const sectionTitle = this.getSectionTitle(sectionId).toLowerCase()
        if (sectionTitle.includes(keyword)) {
          matchedSections.push(sectionId)
          groupMatched = true
          continue
        }
        // 检查具体设置项标题是否匹配
        const searchTerms = this.getSectionSearchTerms(sectionId)
        for (const term of searchTerms) {
          if (term.toLowerCase().includes(keyword)) {
            matchedSections.push(sectionId)
            groupMatched = true
            break
          }
        }
      }

      if (groupMatched) {
        // 如果分组标题匹配，保留全部分类；如果只有部分分类匹配，只保留匹配的分类
        if (groupTitle.includes(keyword)) {
          result.push(groupDef)
        } else {
          result.push({ id: groupDef.id, sections: matchedSections })
        }

        // 自动展开匹配到的分类
        for (const sectionId of matchedSections) {
          if (!this.viewModel.isSectionExpanded(sectionId)) {
            this.viewModel.toggleSection(sectionId)
          }
        }
      }
    }

    this.filteredGroups = result
  }

  // ── 保存设置后的消息解析（问题9：在页面层做本地化，ViewModel 只管状态） ──
  private resolveSaveMessage(): string {
    const msg = this.viewModel.saveMessage
    if (msg === 'saved') {
      return getContext(this).resourceManager.getStringSync($r('app.string.settings_saved').id)
    } else if (msg.startsWith('error:')) {
      const errorCode = msg.substring(6)
      return MessageKeyMapper.resolveMessage(getContext(this), undefined, undefined, errorCode)
    }
    return ''
  }

  // ── 同步后的消息解析 ──
  private resolveSyncMessage(): string {
    const msg = this.viewModel.syncMessage
    if (msg.startsWith('sync_complete:')) {
      const count = msg.substring('sync_complete:'.length)
      return getContext(this).resourceManager.getStringSync($r('app.string.sync_complete_format').id).replace('%s', count)
    } else if (msg.startsWith('error:')) {
      const errorKey = msg.substring(6)
      if (errorKey === 'sync_conflict') {
        return getContext(this).resourceManager.getStringSync($r('app.string.error_sync_conflict').id)
      } else if (errorKey === 'dirty_repo_blocked') {
        return getContext(this).resourceManager.getStringSync($r('app.string.sync_dirty_repo_blocked').id)
      } else if (errorKey === 'sync_error') {
        return getContext(this).resourceManager.getStringSync($r('app.string.sync_error').id)
      } else {
        return MessageKeyMapper.resolveMessage(getContext(this), `error.${errorKey}`, undefined, undefined)
      }
    }
    return ''
  }

  build() {
    // Issue #752：NavDestination 作为 Navigation 目标页根节点，否则实机白屏。
    NavDestination() {
      Column() {
        // ── 顶部 Header：宽屏无搜索框，窄屏有搜索框 ──
        if (this.adaptiveState.sizeClass !== WidthClass.Compact) {
          this.WideSettingsHeader()
        } else {
          this.CompactSettingsHeader()
        }

        if (this.viewModel.isLoading) {
          Column() {
            LoadingProgress()
              .width(48)
              .height(48)
            Text($r('app.string.loading'))
              .fontSize(14)
              .margin({ top: 12 })
          }
          .width('100%')
          .layoutWeight(1)
          .justifyContent(FlexAlign.Center)
        } else {
          // 问题3：宽屏/窄屏分支
          // 宽屏 (isTwoPane)：恢复 #760 之前的完整设置列表布局
          // 窄屏 (!isTwoPane)：保留折叠分类布局
          if (this.adaptiveState.sizeClass !== WidthClass.Compact) {
            this.WideScreenLayout()
          } else {
            this.CompactScreenLayout()
          }
        }
      }
      .width('100%')
      .height('100%')
    }
    .hideTitleBar(true)
    .onShown(() => {
      NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.Settings)
    })
  }

  // ════════════════════════════════════════════
  // 问题3：宽屏布局 — 恢复 #760 之前的完整设置列表
  // ════════════════════════════════════════════
  @Builder
  WideScreenLayout() {
    Scroll() {
      Column() {
        // 按 Core 契约顺序：外观→编辑器→保存→同步→AI→关于/高级
        this.WideAppearanceSection()
        this.WideEditorSection()
        this.WideSaveSection()
        this.WideSyncSection()
        this.WideAiSection()
        this.WideAboutSection()
      }
      .width('100%')
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, bottom: 24 })
    }
    .layoutWeight(1)
    .scrollBar(BarState.Off)
    .align(Alignment.Top)
  }

  // ── 宽屏 Header：返回按钮 + 标题 + 保存状态提示（无搜索框，恢复 #760 之前样式） ──
  @Builder
  WideSettingsHeader() {
    Row() {
      Image($r('sys.media.ohos_ic_public_arrow_left'))
        .width(24)
        .height(24)
        .onClick(() => this.goBack())

      Text($r('app.string.settings'))
        .fontSize(20)
        .fontWeight(FontWeight.Bold)
        .margin({ left: 12 })

      Blank()

      // 保存状态提示
      if (this.viewModel.saveMessage) {
        Text(this.resolveSaveMessage())
          .fontSize(12)
      }

      // 复制设备信息结果提示
      if (this.copyMessage) {
        Text(this.copyMessage)
          .fontSize(12)
          .margin({ left: 8 })
      }
    }
    .width('100%')
    .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 16, bottom: 8 })
    .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
  }

  // ── 窄屏 Header：返回按钮 + 标题 + 搜索框 ──
  @Builder
  CompactSettingsHeader() {
    Column() {
      // 返回按钮行
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .onClick(() => this.goBack())

        Text($r('app.string.settings'))
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .margin({ left: 12 })

        Blank()

        // 保存状态提示
        if (this.viewModel.saveMessage) {
          Text(this.resolveSaveMessage())
            .fontSize(12)
        }

        // 复制设备信息结果提示
        if (this.copyMessage) {
          Text(this.copyMessage)
            .fontSize(12)
            .margin({ left: 8 })
        }
      }
      .width('100%')
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 16, bottom: 8 })

      // 搜索框（窄屏搜索过滤设置项）
      Row() {
        Image($r('app.media.ic_search'))
          .width(20)
          .height(20)
          .margin({ left: 12 })

        TextInput({ placeholder: $r('app.string.settings'), text: this.searchText })
          .layoutWeight(1)
          .height(36)
          .fontSize(14)
          .backgroundColor(Color.Transparent)
          .onChange((value: string) => {
            this.searchText = value
            this.filterSettings(value)
          })
      }
      .width('100%')
      .height(40)
      .margin({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, bottom: 8 })
      .backgroundColor($r('sys.color.search_bg_color'))
      .borderRadius(DesignTokens.radiusSm)
    }
    .width('100%')
    .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
  }

  // ════════════════════════════════════════════
  // 问题3：窄屏布局 — 保留折叠分类
  // ════════════════════════════════════════════
  @Builder
  CompactScreenLayout() {
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

  // ── 分组列表（窄屏折叠布局，支持搜索过滤） ──
  @Builder
  SettingsGroupList() {
    ForEach(this.filteredGroups, (groupDef: GroupDefinition) => {
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
          .layoutWeight(1)

        // 当前值摘要
        Text(this.getSectionValueSummary(sectionId))
          .fontSize(14)
          .margin({ right: 8 })

        // 展开/收起箭头
        Image($r('sys.media.ohos_ic_public_arrow_down'))
          .width(20)
          .height(20)
          .rotate({ angle: this.viewModel.isSectionExpanded(sectionId) ? 180 : 0 })
          .animation({ duration: 150, curve: Curve.EaseInOut })
      }
      .width('100%')
      .height(48)
      .padding({ left: 16, right: 16 })
      .onClick(() => this.viewModel.toggleSection(sectionId))

      // 分类内容（展开时显示）
      if (this.viewModel.isSectionExpanded(sectionId)) {
        Column() {
          this.SectionContent(sectionId)
        }
        .width('100%')
        .padding({ left: 16, right: 16, bottom: 12 })
        .animation({ duration: 150, curve: Curve.EaseInOut })
      }
    }
    .width('100%')
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
  // 宽屏 — 外观设置（恢复 #760 之前的完整列表布局）
  // ════════════════════════════════════════════
  @Builder
  WideAppearanceSection() {
    Column() {
      Text($r('app.string.appearance'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 16 })

      // 主题模式 — 顺序必须是 跟随系统→浅色→深色
      Row() {
        Text($r('app.string.theme_mode'))
          .fontSize(16)
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
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 颜色主题来源（实验性：当前页面默认使用系统 token，自定义颜色暂不生效）
      Row() {
        Text('颜色主题（实验性）')
          .fontSize(16)
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
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 内置主题选择（实验性）
      if (this.viewModel.settings.colorSource === 'built_in' && this.viewModel.builtinThemes.length > 0) {
        Row() {
          Text('内置主题（实验性）')
            .fontSize(16)
            .layoutWeight(1)

          Select(this.getBuiltinThemeOptions())
            .selected(this.getSelectedBuiltinThemeIndex())
            .value(this.getSelectedBuiltinThemeName())
            .onSelect((index: number) => {
              if (index < this.viewModel.builtinThemes.length) {
                const themeId = this.viewModel.builtinThemes[index].theme_id
                this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                  s.selectedBuiltinThemeId = themeId
                  return s
                })
                this.themeAdapter.setSelectedBuiltinThemeId(themeId)
                this.viewModel.saveSettings()
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })
      }

      // 已保存调色板选择（实验性）
      if (this.viewModel.settings.colorSource === 'saved_palette' && this.viewModel.paletteRecords.length > 0) {
        Row() {
          Text('设备配色（实验性）')
            .fontSize(16)
            .layoutWeight(1)

          Select(this.viewModel.paletteRecords.map((r: ThemePaletteRecordDto): SettingsSelectOption => {
            const date = new Date(r.captured_at_ms)
            const dateStr = `${date.getFullYear()}-${(date.getMonth() + 1).toString().padStart(2, '0')}-${date.getDate().toString().padStart(2, '0')}`
            return { value: `${r.source_device_class} · ${dateStr}`, icon: '' }
          }))
            .selected(this.getSelectedPaletteIndex())
            .value(this.getSelectedPaletteName())
            .onSelect((index: number) => {
              if (index < this.viewModel.paletteRecords.length) {
                const paletteId = this.viewModel.paletteRecords[index].palette_id
                this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                  s.selectedPaletteId = paletteId
                  return s
                })
                this.themeAdapter.setSelectedPaletteId(paletteId)
                this.viewModel.saveSettings()
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
          .layoutWeight(1)
        Text(`${this.viewModel.settings.editorFontSize}px`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.editorFontSize, min: 12, max: 32, step: 1 })
          .width(120)
          .onChange((value: number) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorFontSize = value
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 行距倍数
      Row() {
        Text($r('app.string.line_spacing_multiplier'))
          .fontSize(16)
          .layoutWeight(1)
        Text(`${this.viewModel.settings.editorLineSpacingMultiplier.toFixed(1)}`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.editorLineSpacingMultiplier, min: 1.0, max: 3.0, step: 0.1 })
          .width(120)
          .onChange((value: number) => {
            const rounded = Math.round(value * 10) / 10
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorLineSpacingMultiplier = rounded
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
    }
    .width('100%')
    .padding(16)
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 16 })
  }

  // ════════════════════════════════════════════
  // 宽屏 — 编辑器设置
  // ════════════════════════════════════════════
  @Builder
  WideEditorSection() {
    Column() {
      Text($r('app.string.editor'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 16 })

      // 自动缩进
      Row() {
        Text($r('app.string.auto_indent'))
          .fontSize(16)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.autoIndentEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoIndentEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 缩进宽度
      Row() {
        Text($r('app.string.indent_width'))
          .fontSize(16)
          .layoutWeight(1)
        Text(`${this.viewModel.settings.autoIndentWidth}`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.autoIndentWidth, min: 0, max: 8, step: 1 })
          .width(120)
          .onChange((value: number) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoIndentWidth = value
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 打字动画 — 按 Issue #629 静态编辑链路有意不做动画，开关禁用
      Row() {
        Text($r('app.string.typing_animation'))
          .fontSize(16)
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
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: false })
          .enabled(false)
      }
      .width('100%')
    }
    .width('100%')
    .padding(16)
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 16 })
  }

  // ════════════════════════════════════════════
  // 宽屏 — 保存设置
  // ════════════════════════════════════════════
  @Builder
  WideSaveSection() {
    Column() {
      Text($r('app.string.save_section'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 16 })

      // 自动保存
      Row() {
        Text($r('app.string.auto_save'))
          .fontSize(16)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.autoSaveEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoSaveEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 自动保存延迟
      if (this.viewModel.settings.autoSaveEnabled) {
        Row() {
          Text($r('app.string.auto_save_delay'))
            .fontSize(16)
            .layoutWeight(1)
          Text(getContext(this).resourceManager.getStringSync($r('app.string.seconds_format').id)
            .replace('%s', `${Math.round(this.viewModel.settings.autoSaveDelayMs / 1000)}`))
            .fontSize(14)
            .margin({ right: 16 })
          Slider({ value: this.viewModel.settings.autoSaveDelayMs / 1000, min: 1, max: 10, step: 1 })
            .width(120)
            .onChange((value: number) => {
              this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                s.autoSaveDelayMs = value * 1000
                return s
              })
              this.viewModel.saveSettings()
            })
        }
        .width('100%')
        .margin({ bottom: 16 })
      }
    }
    .width('100%')
    .padding(16)
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 16 })
  }

  // ════════════════════════════════════════════
  // 宽屏 — 同步设置
  // ════════════════════════════════════════════
  @Builder
  WideSyncSection() {
    Column() {
      Row() {
        Text($r('app.string.sync_section'))
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
        Blank()

        if (this.viewModel.syncMessage) {
          Text(this.resolveSyncMessage())
            .fontSize(12)
        }
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Sync enabled
      Row() {
        Text($r('app.string.enable_sync'))
          .fontSize(16)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.syncConfig?.enabled || false })
          .onChange((isOn: boolean) => {
            if (this.viewModel.syncConfig) {
              const newConfig: SyncConfig = {
                enabled: isOn,
                activeProvider: this.viewModel.syncConfig.activeProvider,
                providerConfig: this.viewModel.syncConfig.providerConfig,
                autoSync: this.viewModel.syncConfig.autoSync,
                syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
              }
              this.viewModel.syncConfig = newConfig
            }
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Sync 详细配置（仅在同步开启时显示）
      if (this.viewModel.syncConfig?.enabled) {
        // Sync backend type
        Row() {
          Text($r('app.string.sync_method'))
            .fontSize(16)
            .layoutWeight(1)

          Select(this.getSyncProviderOptions())
            .selected(this.getSyncProviderIndex())
            .value(this.getSyncProviderName())
            .onSelect((index: number) => {
              if (this.viewModel.syncConfig) {
                const providers = ['github', 'gitlab', 'gitea', 'custom']
                const newConfig: SyncConfig = {
                  enabled: this.viewModel.syncConfig.enabled,
                  activeProvider: providers[index],
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: this.viewModel.syncConfig.autoSync,
                  syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.syncConfig = newConfig
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Auto sync
        Row() {
          Text($r('app.string.auto_sync'))
            .fontSize(16)
            .layoutWeight(1)

          Toggle({ type: ToggleType.Switch, isOn: this.viewModel.syncConfig?.autoSync || false })
            .onChange((isOn: boolean) => {
              if (this.viewModel.syncConfig) {
                const newConfig: SyncConfig = {
                  enabled: this.viewModel.syncConfig.enabled,
                  activeProvider: this.viewModel.syncConfig.activeProvider,
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: isOn,
                  syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.syncConfig = newConfig
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Sync interval
        Row() {
          Text($r('app.string.sync_interval'))
            .fontSize(16)
            .layoutWeight(1)

          Text(getContext(this).resourceManager.getStringSync($r('app.string.seconds_format').id)
            .replace('%s', `${this.viewModel.syncConfig?.syncIntervalSeconds || 300}`))
            .fontSize(14)
            .margin({ right: 16 })

          Slider({
            value: this.viewModel.syncConfig?.syncIntervalSeconds || 300,
            min: 60,
            max: 3600,
            step: 60
          })
            .width(120)
            .onChange((value: number) => {
              if (this.viewModel.syncConfig) {
                const newConfig: SyncConfig = {
                  enabled: this.viewModel.syncConfig.enabled,
                  activeProvider: this.viewModel.syncConfig.activeProvider,
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: this.viewModel.syncConfig.autoSync,
                  syncIntervalSeconds: value,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.syncConfig = newConfig
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
            .enabled(!this.viewModel.isSyncing)
            .onClick(() => this.viewModel.performSync())

          Button($r('app.string.diagnose'))
            .fontSize(14)
            .height(36)
            .margin({ left: 12 })
            .enabled(!this.viewModel.isDiagnosing)
            .onClick(() => this.viewModel.runSyncDiagnostics())
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Diagnostics result card
        if (this.viewModel.syncDiagnostics) {
          this.DiagnosticsResultCard()
        }
      }
    }
    .width('100%')
    .padding(16)
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 16 })
  }

  // ════════════════════════════════════════════
  // 宽屏 — AI 设置
  // ════════════════════════════════════════════
  @Builder
  WideAiSection() {
    Column() {
      Text($r('app.string.ai_section'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 16 })

      Row() {
        Text($r('app.string.enable_ai'))
          .fontSize(16)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.aiEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.aiEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
    }
    .width('100%')
    .padding(16)
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 16 })
  }

  // ════════════════════════════════════════════
  // 宽屏 — 诊断与日志设置
  // ════════════════════════════════════════════
  @Builder
  WideDiagnosticsSection() {
    Column() {
      Text('诊断与日志')
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 16 })

      // 开启诊断日志
      Row() {
        Text('开启诊断日志')
          .fontSize(16)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.diagnosticsEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.diagnosticsEnabled = isOn
              if (!isOn) {
                s.diagnosticsVerbose = false
              }
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 详细日志
      Row() {
        Text('详细日志')
          .fontSize(16)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.diagnosticsVerbose })
          .enabled(this.viewModel.settings.diagnosticsEnabled)
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.diagnosticsVerbose = isOn
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 导出诊断包
      Button('导出诊断包')
        .fontSize(14)
        .height(36)
        .width('100%')
        .margin({ bottom: 12 })
        .onClick(() => {
          this.viewModel.exportDiagnostics()
        })

      // 清空日志（问题6：ViewModel 内部同时清 Rust diagnostics 和 HiLog buffer）
      Button('清空日志')
        .fontSize(14)
        .height(36)
        .width('100%')
        .margin({ bottom: 12 })
        .onClick(() => {
          this.viewModel.clearLogs()
        })

      // 复制设备信息（问题5：调用 copyDeviceInfoToClipboard 真正写入剪贴板）
      Button('复制设备信息')
        .fontSize(14)
        .height(36)
        .width('100%')
        .onClick(async () => {
          const ok = await this.viewModel.copyDeviceInfoToClipboard()
          if (ok) {
            this.copyMessage = '已复制'
            setTimeout(() => { this.copyMessage = '' }, 2000)
          } else {
            this.copyMessage = '复制失败'
            setTimeout(() => { this.copyMessage = '' }, 2000)
          }
        })
    }
    .width('100%')
    .padding(16)
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 16 })
  }

  // ════════════════════════════════════════════
  // 宽屏 — 关于
  // ════════════════════════════════════════════
  @Builder
  WideAboutSection() {
    Column() {
      Text($r('app.string.about_title'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 16 })

      // 应用名
      Row() {
        Text($r('app.string.about_app_name_label'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_app_name'))
          .fontSize(14)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 作者
      Row() {
        Text($r('app.string.about_author_label'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_author'))
          .fontSize(14)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 项目地址
      Column() {
        Text($r('app.string.about_project_label'))
          .fontSize(16)
          .margin({ bottom: 4 })
        Text('GitHub: github.com/Xiwei753/xiezuoruanjian')
          .fontSize(14)
        Text($r('app.string.about_gitee'))
          .fontSize(14)
          .margin({ top: 4 })
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .margin({ bottom: 8 })

      // 开源协议
      Row() {
        Text($r('app.string.about_license_label'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_license'))
          .fontSize(14)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 镜像说明
      Text($r('app.string.about_mirror_note'))
        .fontSize(13)
        .margin({ bottom: 8 })

      // 版本信息
      Row() {
        Text($r('app.string.version_info'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_platform'))
          .fontSize(14)
      }
      .width('100%')

      // 工作区路径
      Row() {
        Text($r('app.string.workspace_path'))
          .fontSize(16)
          .layoutWeight(1)
      }
      .width('100%')
      .margin({ top: 8 })
    }
    .width('100%')
    .padding(16)
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 16 })
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 外观设置
  // ════════════════════════════════════════════
  @Builder
  AppearanceContent() {
    Column() {
      // 主题模式 — 顺序必须是 跟随系统→浅色→深色
      Row() {
        Text($r('app.string.theme_mode'))
          .fontSize(16)
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
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 颜色主题来源（实验性：当前页面默认使用系统 token，自定义颜色暂不生效）
      Row() {
        Text('颜色主题（实验性）')
          .fontSize(16)
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
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 内置主题选择（实验性）
      if (this.viewModel.settings.colorSource === 'built_in' && this.viewModel.builtinThemes.length > 0) {
        Row() {
          Text('内置主题（实验性）')
            .fontSize(16)
            .layoutWeight(1)

          Select(this.getBuiltinThemeOptions())
            .selected(this.getSelectedBuiltinThemeIndex())
            .value(this.getSelectedBuiltinThemeName())
            .onSelect((index: number) => {
              if (index < this.viewModel.builtinThemes.length) {
                const themeId = this.viewModel.builtinThemes[index].theme_id
                this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                  s.selectedBuiltinThemeId = themeId
                  return s
                })
                this.themeAdapter.setSelectedBuiltinThemeId(themeId)
                this.viewModel.saveSettings()
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })
      }

      // 已保存调色板选择（实验性）
      if (this.viewModel.settings.colorSource === 'saved_palette' && this.viewModel.paletteRecords.length > 0) {
        Row() {
          Text('设备配色（实验性）')
            .fontSize(16)
            .layoutWeight(1)

          Select(this.viewModel.paletteRecords.map((r: ThemePaletteRecordDto): SettingsSelectOption => {
            const date = new Date(r.captured_at_ms)
            const dateStr = `${date.getFullYear()}-${(date.getMonth() + 1).toString().padStart(2, '0')}-${date.getDate().toString().padStart(2, '0')}`
            return { value: `${r.source_device_class} · ${dateStr}`, icon: '' }
          }))
            .selected(this.getSelectedPaletteIndex())
            .value(this.getSelectedPaletteName())
            .onSelect((index: number) => {
              if (index < this.viewModel.paletteRecords.length) {
                const paletteId = this.viewModel.paletteRecords[index].palette_id
                this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                  s.selectedPaletteId = paletteId
                  return s
                })
                this.themeAdapter.setSelectedPaletteId(paletteId)
                this.viewModel.saveSettings()
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
          .layoutWeight(1)
        Text(`${this.viewModel.settings.editorFontSize}px`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.editorFontSize, min: 12, max: 32, step: 1 })
          .width(120)
          .onChange((value: number) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorFontSize = value
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 行距倍数
      Row() {
        Text($r('app.string.line_spacing_multiplier'))
          .fontSize(16)
          .layoutWeight(1)
        Text(`${this.viewModel.settings.editorLineSpacingMultiplier.toFixed(1)}`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.editorLineSpacingMultiplier, min: 1.0, max: 3.0, step: 0.1 })
          .width(120)
          .onChange((value: number) => {
            const rounded = Math.round(value * 10) / 10
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorLineSpacingMultiplier = rounded
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
    }
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 编辑器设置
  // ════════════════════════════════════════════
  @Builder
  EditorContent() {
    Column() {
      // 自动缩进
      Row() {
        Text($r('app.string.auto_indent'))
          .fontSize(16)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.autoIndentEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoIndentEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 缩进宽度
      Row() {
        Text($r('app.string.indent_width'))
          .fontSize(16)
          .layoutWeight(1)
        Text(`${this.viewModel.settings.autoIndentWidth}`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.autoIndentWidth, min: 0, max: 8, step: 1 })
          .width(120)
          .onChange((value: number) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoIndentWidth = value
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 打字动画 — 按 Issue #629 静态编辑链路有意不做动画，开关禁用
      Row() {
        Text($r('app.string.typing_animation'))
          .fontSize(16)
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
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: false })
          .enabled(false)
      }
      .width('100%')
    }
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 保存设置
  // ════════════════════════════════════════════
  @Builder
  SaveContent() {
    Column() {
      // 自动保存
      Row() {
        Text($r('app.string.auto_save'))
          .fontSize(16)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.autoSaveEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoSaveEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 自动保存延迟
      if (this.viewModel.settings.autoSaveEnabled) {
        Row() {
          Text($r('app.string.auto_save_delay'))
            .fontSize(16)
            .layoutWeight(1)
          Text(getContext(this).resourceManager.getStringSync($r('app.string.seconds_format').id)
            .replace('%s', `${Math.round(this.viewModel.settings.autoSaveDelayMs / 1000)}`))
            .fontSize(14)
            .margin({ right: 16 })
          Slider({ value: this.viewModel.settings.autoSaveDelayMs / 1000, min: 1, max: 10, step: 1 })
            .width(120)
            .onChange((value: number) => {
              this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                s.autoSaveDelayMs = value * 1000
                return s
              })
              this.viewModel.saveSettings()
            })
        }
        .width('100%')
      }
    }
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 同步设置
  // ════════════════════════════════════════════
  @Builder
  SyncContent() {
    Column() {
      // 同步状态提示
      if (this.viewModel.syncMessage) {
        Row() {
          Text(this.resolveSyncMessage())
            .fontSize(12)
        }
        .width('100%')
        .margin({ bottom: 12 })
      }

      // Sync enabled
      Row() {
        Text($r('app.string.enable_sync'))
          .fontSize(16)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.syncConfig?.enabled || false })
          .onChange((isOn: boolean) => {
            if (this.viewModel.syncConfig) {
              const newConfig: SyncConfig = {
                enabled: isOn,
                activeProvider: this.viewModel.syncConfig.activeProvider,
                providerConfig: this.viewModel.syncConfig.providerConfig,
                autoSync: this.viewModel.syncConfig.autoSync,
                syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
              }
              this.viewModel.syncConfig = newConfig
            }
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Sync 详细配置（仅在同步开启时显示）
      if (this.viewModel.syncConfig?.enabled) {
        // Sync backend type
        Row() {
          Text($r('app.string.sync_method'))
            .fontSize(16)
            .layoutWeight(1)

          Select(this.getSyncProviderOptions())
            .selected(this.getSyncProviderIndex())
            .value(this.getSyncProviderName())
            .onSelect((index: number) => {
              if (this.viewModel.syncConfig) {
                const providers = ['github', 'gitlab', 'gitea', 'custom']
                const newConfig: SyncConfig = {
                  enabled: this.viewModel.syncConfig.enabled,
                  activeProvider: providers[index],
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: this.viewModel.syncConfig.autoSync,
                  syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.syncConfig = newConfig
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Auto sync
        Row() {
          Text($r('app.string.auto_sync'))
            .fontSize(16)
            .layoutWeight(1)

          Toggle({ type: ToggleType.Switch, isOn: this.viewModel.syncConfig?.autoSync || false })
            .onChange((isOn: boolean) => {
              if (this.viewModel.syncConfig) {
                const newConfig: SyncConfig = {
                  enabled: this.viewModel.syncConfig.enabled,
                  activeProvider: this.viewModel.syncConfig.activeProvider,
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: isOn,
                  syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.syncConfig = newConfig
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Sync interval
        Row() {
          Text($r('app.string.sync_interval'))
            .fontSize(16)
            .layoutWeight(1)

          Text(getContext(this).resourceManager.getStringSync($r('app.string.seconds_format').id)
            .replace('%s', `${this.viewModel.syncConfig?.syncIntervalSeconds || 300}`))
            .fontSize(14)
            .margin({ right: 16 })

          Slider({
            value: this.viewModel.syncConfig?.syncIntervalSeconds || 300,
            min: 60,
            max: 3600,
            step: 60
          })
            .width(120)
            .onChange((value: number) => {
              if (this.viewModel.syncConfig) {
                const newConfig: SyncConfig = {
                  enabled: this.viewModel.syncConfig.enabled,
                  activeProvider: this.viewModel.syncConfig.activeProvider,
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: this.viewModel.syncConfig.autoSync,
                  syncIntervalSeconds: value,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.syncConfig = newConfig
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
            .enabled(!this.viewModel.isSyncing)
            .onClick(() => this.viewModel.performSync())

          Button($r('app.string.diagnose'))
            .fontSize(14)
            .height(36)
            .margin({ left: 12 })
            .enabled(!this.viewModel.isDiagnosing)
            .onClick(() => this.viewModel.runSyncDiagnostics())
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Diagnostics result card
        if (this.viewModel.syncDiagnostics) {
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
        .margin({ bottom: 12 })

      // Four check items
      Row() {
        this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.network').id),
          this.viewModel.syncDiagnostics!.networkOk, this.viewModel.syncDiagnostics!.networkStatus)
        this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.auth').id),
          this.viewModel.syncDiagnostics!.authOk, this.viewModel.syncDiagnostics!.authStatus)
        this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.repository').id),
          this.viewModel.syncDiagnostics!.remoteOk, this.viewModel.syncDiagnostics!.networkStatus)
      }
      .width('100%')
      .justifyContent(FlexAlign.SpaceAround)
      .margin({ bottom: 12 })

      // User message
      Text(this.viewModel.syncDiagnostics!.errorCategory && this.viewModel.syncDiagnostics!.errorCategory.length > 0
        ? MessageKeyMapper.resolveMessage(getContext(this), `error.${this.viewModel.syncDiagnostics!.errorCategory}`, undefined, undefined)
        : (this.viewModel.syncDiagnostics!.rawError ?? this.viewModel.syncDiagnostics!.providerDetails ?? ''))
        .fontSize(14)
        .margin({ bottom: 8 })

      // Error category
      if (this.viewModel.syncDiagnostics!.errorCategory && this.viewModel.syncDiagnostics!.errorCategory.length > 0) {
        Row() {
          Text(getContext(this).resourceManager.getStringSync($r('app.string.error_category').id) + ': ')
            .fontSize(12)
          Text(this.viewModel.syncDiagnostics!.errorCategory)
            .fontSize(12)
        }
        .width('100%')
        .margin({ bottom: 4 })
      }
    }
    .width('100%')
    .padding(12)
    .borderRadius(DesignTokens.radiusSm)
    .margin({ bottom: 16 })
  }

  @Builder
  DiagnosticItem(label: string, ok: boolean, status: string) {
    Column() {
      Text(ok ? 'OK' : 'FAIL')
        .fontSize(14)
        .fontWeight(FontWeight.Bold)
      Text(label)
        .fontSize(12)
        .margin({ top: 4 })
      if (status) {
        Text(status)
          .fontSize(10)
          .margin({ top: 2 })
      }
    }
    .alignItems(HorizontalAlign.Center)
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 恢复（占位）
  // ════════════════════════════════════════════
  @Builder
  RecoveryContent() {
    Column() {
      Text('恢复功能即将上线')
        .fontSize(14)
    }
    .width('100%')
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — AI 设置
  // ════════════════════════════════════════════
  @Builder
  AiContent() {
    Column() {
      Row() {
        Text($r('app.string.enable_ai'))
          .fontSize(16)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.aiEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.aiEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
    }
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 诊断与日志
  // ════════════════════════════════════════════
  @Builder
  DiagnosticsContent() {
    Column() {
      // 开启诊断日志
      Row() {
        Text('开启诊断日志')
          .fontSize(16)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.diagnosticsEnabled })
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.diagnosticsEnabled = isOn
              if (!isOn) {
                s.diagnosticsVerbose = false
              }
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 详细日志
      Row() {
        Text('详细日志')
          .fontSize(16)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.diagnosticsVerbose })
          .enabled(this.viewModel.settings.diagnosticsEnabled)
          .onChange((isOn: boolean) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.diagnosticsVerbose = isOn
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // 导出诊断包
      Button('导出诊断包')
        .fontSize(14)
        .height(36)
        .width('100%')
        .margin({ bottom: 12 })
        .onClick(() => {
          this.viewModel.exportDiagnostics()
        })

      // 清空日志（问题6：ViewModel 内部同时清 Rust diagnostics 和 HiLog buffer）
      Button('清空日志')
        .fontSize(14)
        .height(36)
        .width('100%')
        .margin({ bottom: 12 })
        .onClick(() => {
          this.viewModel.clearLogs()
        })

      // 复制设备信息（问题5：调用 copyDeviceInfoToClipboard 真正写入剪贴板）
      Button('复制设备信息')
        .fontSize(14)
        .height(36)
        .width('100%')
        .onClick(async () => {
          const ok = await this.viewModel.copyDeviceInfoToClipboard()
          if (ok) {
            this.copyMessage = '已复制'
            setTimeout(() => { this.copyMessage = '' }, 2000)
          } else {
            this.copyMessage = '复制失败'
            setTimeout(() => { this.copyMessage = '' }, 2000)
          }
        })
    }
    .width('100%')
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 实验室（占位）
  // ════════════════════════════════════════════
  @Builder
  LaboratoryContent() {
    Column() {
      Text('实验室功能即将上线')
        .fontSize(14)
    }
    .width('100%')
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 关于
  // ════════════════════════════════════════════
  @Builder
  AboutContent() {
    Column() {
      // 应用名
      Row() {
        Text($r('app.string.about_app_name_label'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_app_name'))
          .fontSize(14)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 作者
      Row() {
        Text($r('app.string.about_author_label'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_author'))
          .fontSize(14)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 项目地址
      Column() {
        Text($r('app.string.about_project_label'))
          .fontSize(16)
          .margin({ bottom: 4 })
        Text('GitHub: github.com/Xiwei753/xiezuoruanjian')
          .fontSize(14)
        Text($r('app.string.about_gitee'))
          .fontSize(14)
          .margin({ top: 4 })
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .margin({ bottom: 8 })

      // 开源协议
      Row() {
        Text($r('app.string.about_license_label'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_license'))
          .fontSize(14)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 镜像说明
      Text($r('app.string.about_mirror_note'))
        .fontSize(13)
        .margin({ bottom: 8 })

      // 版本信息
      Row() {
        Text($r('app.string.version_info'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_platform'))
          .fontSize(14)
      }
      .width('100%')

      // 工作区路径
      Row() {
        Text($r('app.string.workspace_path'))
          .fontSize(16)
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

  // ── 分类搜索 token — 返回该分类下所有具体设置项标题，用于搜索匹配 ──
  getSectionSearchTerms(sectionId: SettingsSectionId): string[] {
    const ctx = getContext(this)
    switch (sectionId) {
      case 'appearance':
        return [
          ctx.resourceManager.getStringSync($r('app.string.theme_mode').id),
          '颜色主题（实验性）',
          '内置主题（实验性）',
          '设备配色（实验性）',
          ctx.resourceManager.getStringSync($r('app.string.font_size').id),
          ctx.resourceManager.getStringSync($r('app.string.line_spacing_multiplier').id)
        ]
      case 'editor':
        return [
          ctx.resourceManager.getStringSync($r('app.string.auto_indent').id),
          ctx.resourceManager.getStringSync($r('app.string.indent_width').id),
          ctx.resourceManager.getStringSync($r('app.string.typing_animation').id),
          ctx.resourceManager.getStringSync($r('app.string.smooth_cursor').id)
        ]
      case 'save':
        return [
          ctx.resourceManager.getStringSync($r('app.string.auto_save').id),
          ctx.resourceManager.getStringSync($r('app.string.auto_save_delay').id)
        ]
      case 'sync':
        return [
          ctx.resourceManager.getStringSync($r('app.string.enable_sync').id),
          ctx.resourceManager.getStringSync($r('app.string.sync_method').id),
          ctx.resourceManager.getStringSync($r('app.string.auto_sync').id),
          ctx.resourceManager.getStringSync($r('app.string.sync_interval').id),
          ctx.resourceManager.getStringSync($r('app.string.diagnose').id)
        ]
      case 'recovery':
        return ['恢复功能即将上线']
      case 'ai':
        return [ctx.resourceManager.getStringSync($r('app.string.enable_ai').id)]
      case 'diagnostics':
        return ['开启诊断日志', '详细日志', '导出诊断包', '清空日志', '复制设备信息']
      case 'laboratory':
        return ['实验室功能即将上线']
      case 'about':
        return [
          ctx.resourceManager.getStringSync($r('app.string.about_app_name_label').id),
          ctx.resourceManager.getStringSync($r('app.string.about_author_label').id),
          ctx.resourceManager.getStringSync($r('app.string.about_project_label').id),
          ctx.resourceManager.getStringSync($r('app.string.about_license_label').id),
          ctx.resourceManager.getStringSync($r('app.string.version_info').id),
          ctx.resourceManager.getStringSync($r('app.string.workspace_path').id)
        ]
      default:
        return []
    }
  }

  // ── 分类当前值摘要 ──
  getSectionValueSummary(sectionId: SettingsSectionId): string {
    switch (sectionId) {
      case 'appearance': {
        const mode = this.viewModel.settings.appearanceMode || this.viewModel.settings.themeMode || 'system'
        if (mode === 'light') return getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id)
        if (mode === 'dark') return getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id)
        return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
      }
      case 'editor':
        return `${this.viewModel.settings.editorFontSize}px`
      case 'save':
        return this.viewModel.settings.autoSaveEnabled ? '开启' : '关闭'
      case 'sync':
        return this.viewModel.syncConfig?.enabled ? '开启' : '关闭'
      case 'recovery':
        return ''
      case 'ai':
        return this.viewModel.settings.aiEnabled ? '开启' : '关闭'
      case 'diagnostics':
        return this.viewModel.settings.diagnosticsEnabled ? '开启' : '关闭'
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
    return this.viewModel.builtinThemes.map((theme: BuiltinThemeDto): SettingsSelectOption => {
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
    const mode = this.viewModel.settings.appearanceMode || this.viewModel.settings.themeMode || 'system'
    const themes = ['system', 'light', 'dark']
    return themes.indexOf(mode)
  }

  getThemeName(): string {
    const mode = this.viewModel.settings.appearanceMode || this.viewModel.settings.themeMode || 'system'
    if (mode === 'system') return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
    if (mode === 'light') return getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id)
    if (mode === 'dark') return getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id)
    return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
  }

  getColorSourceIndex(): number {
    const sources: string[] = ['built_in', 'saved_palette']
    return sources.indexOf(this.viewModel.settings.colorSource || 'built_in')
  }

  getColorSourceName(): string {
    const s = this.viewModel.settings.colorSource || 'built_in'
    if (s === 'saved_palette') return '已保存的设备配色'
    return '素笺默认'
  }

  getSelectedBuiltinThemeIndex(): number {
    const id = this.viewModel.settings.selectedBuiltinThemeId || ''
    if (id.length === 0) return 0
    return this.viewModel.builtinThemes.findIndex(t => t.theme_id === id)
  }

  getSelectedBuiltinThemeName(): string {
    const id = this.viewModel.settings.selectedBuiltinThemeId || ''
    if (id.length === 0) return '素笺默认'
    const theme = this.viewModel.builtinThemes.find(t => t.theme_id === id)
    return theme ? theme.name : '素笺默认'
  }

  getSelectedPaletteIndex(): number {
    const id = this.viewModel.settings.selectedPaletteId || ''
    if (id.length === 0) return 0
    return this.viewModel.paletteRecords.findIndex(r => r.palette_id === id)
  }

  getSelectedPaletteName(): string {
    const id = this.viewModel.settings.selectedPaletteId || ''
    if (id.length === 0 && this.viewModel.paletteRecords.length > 0) {
      const r = this.viewModel.paletteRecords[0]
      const date = new Date(r.captured_at_ms)
      return `${r.source_device_class} · ${date.getFullYear()}-${(date.getMonth() + 1).toString().padStart(2, '0')}-${date.getDate().toString().padStart(2, '0')}`
    }
    const record = this.viewModel.paletteRecords.find(r => r.palette_id === id)
    if (record) {
      const date = new Date(record.captured_at_ms)
      return `${record.source_device_class} · ${date.getFullYear()}-${(date.getMonth() + 1).toString().padStart(2, '0')}-${date.getDate().toString().padStart(2, '0')}`
    }
    return ''
  }

  getSyncProviderIndex(): number {
    const providers: string[] = ['github', 'gitlab', 'gitea', 'custom']
    return providers.indexOf(this.viewModel.syncConfig?.activeProvider || 'github')
  }

  getSyncProviderName(): string {
    const p: string = this.viewModel.syncConfig?.activeProvider || 'github'
    if (p === 'github') return 'GitHub'
    if (p === 'gitlab') return 'GitLab'
    if (p === 'gitea') return 'Gitea'
    return getContext(this).resourceManager.getStringSync($r('app.string.custom').id)
  }
}
