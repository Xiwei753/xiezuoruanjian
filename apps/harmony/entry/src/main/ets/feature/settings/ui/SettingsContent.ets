// SettingsContent — 设置内容（宽屏大号悬浮层与窄屏路由页共用）。
// Issue #825：把设置内容、搜索、分组折叠布局从 SettingsScreen 抽出来，
// 让宽屏设置可以直接悬浮在打开前的页面之上，而不是再压一层路由页。
// - 本组件只负责内容与折叠状态，不持有导航栈；
// - 唯一关闭入口是 onRequestClose 回调，由外壳注入
//   （窄屏路由外壳注入 safePopWithStack，宽屏模态浮层注入 SettingsOverlayController.close()）。
// - twoColumn=true 时用大号浮层布局：顶部搜索 + 两列折叠分组。
//   是否两列由 SettingsOverlayController 按 Core 的 primaryNavigationPlacement 决定，
//   只有一级导航在 Side（>=840vp）才两列，Bottom 那档用单列。
// 设置内容、保存逻辑、主题逻辑仍然全部走现有 SettingsViewModel 和 backend。

import { promptAction } from '@kit.ArkUI'
import { LocalSettings, ThemePaletteRecordDto, BuiltinThemeDto } from '../../../corebridge/dto/SettingsDtos'
import { SyncConfig, SyncDiagnosticsResult } from '../../../corebridge/dto/SyncDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl, ColorSource } from '../../../ui/theme/HarmonyThemeAdapter'
import { ThemeMode } from '../../../ui/theme/HarmonyThemeTypes'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { MessageKeyMapper } from '../../../ui/i18n/MessageKeyMapper'
import { SettingsViewModel, SettingsSectionId } from '../presentation/SettingsViewModel'
import { PrimaryNavigationPlacement, WidthClass } from '../../../corebridge/dto/PlatformDtos'

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
export struct SettingsContent {
  // Issue #825：唯一的关闭入口回调。内容组件自己不碰导航栈。
  @Param onRequestClose: () => void = () => {}
  // Issue #825：是否由宽屏模态浮层承载（标题 + 关闭 + 搜索由本组件自己画）。
  // 窄屏路由页传 false，标题栏由 NavDestination 承载。
  @Param overlayLayout: boolean = false
  // Issue #825 复核第3项：两列与否不作为打开时快照传进来，改成按最新 AdaptiveContext 派生。
  // 只有 Core 判定一级导航在 Side（>=840vp）才两列；600–839vp 那档 Workbench 但一级导航
  // 仍是 Bottom，用单列。平板旋转 / 自由窗口在两档之间移动时，浮层会跟着换布局。
  private get twoColumn(): boolean {
    return this.adaptiveState.layoutPlan.primaryNavigationPlacement === PrimaryNavigationPlacement.Side
  }

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

  // Issue #825：设置内容的唯一根 Builder。
  // 窄屏由 SettingsScreen 的 NavDestination/HdsSettingsDestination 外壳调用，
  // 宽屏由 SettingsOverlayPanel 调用：overlayLayout 决定用浮层外壳（标题 + 关闭 + 搜索），
  // twoColumn（内部按 AdaptiveContext 派生）决定分组是排两列还是单列。
  @Builder
  NavContent() {
    Column() {
      // ── 顶部 Header：宽屏浮层有标题、关闭和搜索，窄屏路由页才有搜索框 ──
      if (this.overlayLayout) {
        this.OverlaySettingsHeader()
      } else if (this.adaptiveState.sizeClass !== WidthClass.Compact) {
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
        // 宽屏浮层：两列（>=840vp，一级导航在 Side）或单列（600–839vp，一级导航仍是 Bottom）；
        // 单列沿用折叠分组列表，和窄屏路由页同一套展开逻辑。
        // 路由页宽屏 (sizeClass 非 Compact)：#760 之前的完整设置列表布局。
        // 列数来自最新的 Core primaryNavigationPlacement，窗口改档会即时重排。
        if (this.twoColumn) {
          this.TwoColumnSettingsLayout()
        } else if (this.overlayLayout || this.adaptiveState.sizeClass === WidthClass.Compact) {
          this.CompactScreenLayout()
        } else {
          this.WideScreenLayout()
        }
      }
    }
    .width('100%')
    .height('100%')
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

  // ── 宽屏 Header：保存状态提示（标题由外壳标题栏承载，Issue #768/#774） ──
  @Builder
  WideSettingsHeader() {
    Row() {
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

  // ── 窄屏 Header：保存状态提示 + 搜索框（标题由外壳标题栏承载，Issue #768/#774） ──
  @Builder
  CompactSettingsHeader() {
    Column() {
      // 状态提示行
      Row() {
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
      .padding({ top: 16, bottom: 8 })

      // 搜索框（窄屏搜索过滤设置项）
      Row() {
        Image($r('app.media.ic_search'))
          .width(20)
          .height(20)
          .fillColor($r('sys.color.icon_primary'))
          .margin({ left: 12 })

        TextInput({ placeholder: '', text: this.searchText })
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
      .margin({ bottom: 8 })
      .backgroundColor($r('sys.color.comp_background_gray'))
      .borderRadius(DesignTokens.radiusSm)
    }
    .width('100%')
    .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding })
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
        // 分组标题（单分类且组名等于分类名时不重复画）
        if (this.shouldShowGroupHeader(groupDef)) {
          this.SettingsGroupHeader(groupDef.id)
        }

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

  // ── 中性操作按钮（使用系统中性表面色） ──
  @Builder
  SettingsNeutralAction(label: ResourceStr, onClick: () => void, enabled: boolean = true) {
    Button(label, { type: ButtonType.Normal })
      .width('100%')
      .backgroundColor($r('sys.color.ohos_id_color_button_normal'))
      .fontColor($r('sys.color.font_primary'))
      .borderRadius(DesignTokens.radiusSm)
      .enabled(enabled)
      .onClick(onClick)
  }

  // ── 横排内联操作按钮（用于 Row 中并排按钮，layoutWeight 均分宽度） ──
  @Builder
  SettingsInlineAction(label: ResourceStr, onClick: () => void, enabled: boolean = true) {
    Button(label, { type: ButtonType.Normal })
      .layoutWeight(1)
      .backgroundColor($r('sys.color.ohos_id_color_button_normal'))
      .fontColor($r('sys.color.font_primary'))
      .borderRadius(DesignTokens.radiusSm)
      .enabled(enabled)
      .onClick(onClick)
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

      // 启用同步
      Row() {
        Text($r('app.string.enable_sync'))
          .fontSize(16)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.syncConfig?.enabled || false })
          .onChange((isOn: boolean) => {
            if (this.viewModel.syncConfig) {
              const newConfig: SyncConfig = {
                enabled: isOn,
                activeProvider: 'github_api',
                providerConfig: this.viewModel.syncConfig.providerConfig,
                autoSync: this.viewModel.syncConfig.autoSync,
                syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
              }
              this.viewModel.updateSyncConfig(newConfig)
            }
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Sync 详细配置（仅在同步开启时显示）
      if (this.viewModel.syncConfig?.enabled) {
        // 自动同步
        Row() {
          Text($r('app.string.auto_sync'))
            .fontSize(16)
            .layoutWeight(1)

          Toggle({ type: ToggleType.Switch, isOn: this.viewModel.syncConfig?.autoSync || false })
            .onChange((isOn: boolean) => {
              if (this.viewModel.syncConfig) {
                const newConfig: SyncConfig = {
                  enabled: this.viewModel.syncConfig.enabled,
                  activeProvider: 'github_api',
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: isOn,
                  syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.updateSyncConfig(newConfig)
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // GitHub 远程仓库地址
        Column() {
          Text($r('app.string.github_repo'))
            .fontSize(16)
            .margin({ bottom: 8 })

          TextInput({ text: this.viewModel.syncRemoteUrl })
            .width('100%')
            .height(40)
            .fontSize(14)
            .onChange((value: string) => {
              this.viewModel.updateSyncRemoteUrl(value)
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // 分支
        Column() {
          Text($r('app.string.sync_branch'))
            .fontSize(16)
            .margin({ bottom: 8 })

          TextInput({ text: this.viewModel.syncBranch })
            .width('100%')
            .height(40)
            .fontSize(14)
            .onChange((value: string) => {
              this.viewModel.updateSyncBranch(value)
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // HTTPS Token
        Column() {
          Text($r('app.string.https_token'))
            .fontSize(16)
            .margin({ bottom: 8 })

          TextInput({ text: this.viewModel.syncToken })
            .width('100%')
            .height(40)
            .fontSize(14)
            .type(InputType.Password)
            .showPasswordIcon(true)
            .onChange((value: string) => {
              this.viewModel.updateSyncToken(value)
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // 同步间隔
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
                  activeProvider: 'github_api',
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: this.viewModel.syncConfig.autoSync,
                  syncIntervalSeconds: value,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.updateSyncConfig(newConfig)
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Sync actions
        Row({ space: 12 }) {
          this.SettingsInlineAction($r('app.string.sync_section'),
            () => this.viewModel.performSync(),
            !this.viewModel.isSyncing)

          this.SettingsInlineAction($r('app.string.diagnose'),
            () => this.viewModel.runSyncDiagnostics(),
            !this.viewModel.isDiagnosing)
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
      Column({ space: 12 }) {
        this.SettingsNeutralAction('导出诊断包', async () => {
          const result = await this.viewModel.exportDiagnostics()
          if (result.status === 'success') {
            promptAction.showToast({ message: '诊断包已导出' })
          } else if (result.status === 'failed') {
            promptAction.showToast({ message: '诊断包导出失败' })
          }
        })

        // 清空日志（问题6：ViewModel 内部同时清 Rust diagnostics 和 HiLog buffer）
        this.SettingsNeutralAction('清空日志', () => {
          this.viewModel.clearLogs()
        })

        // 复制设备信息（问题5：调用 copyDeviceInfoToClipboard 真正写入剪贴板）
        this.SettingsNeutralAction('复制设备信息', async () => {
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

      // 启用同步
      Row() {
        Text($r('app.string.enable_sync'))
          .fontSize(16)
          .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.syncConfig?.enabled || false })
          .onChange((isOn: boolean) => {
            if (this.viewModel.syncConfig) {
              const newConfig: SyncConfig = {
                enabled: isOn,
                activeProvider: 'github_api',
                providerConfig: this.viewModel.syncConfig.providerConfig,
                autoSync: this.viewModel.syncConfig.autoSync,
                syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
              }
              this.viewModel.updateSyncConfig(newConfig)
            }
          })
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Sync 详细配置（仅在同步开启时显示）
      if (this.viewModel.syncConfig?.enabled) {
        // 自动同步
        Row() {
          Text($r('app.string.auto_sync'))
            .fontSize(16)
            .layoutWeight(1)

          Toggle({ type: ToggleType.Switch, isOn: this.viewModel.syncConfig?.autoSync || false })
            .onChange((isOn: boolean) => {
              if (this.viewModel.syncConfig) {
                const newConfig: SyncConfig = {
                  enabled: this.viewModel.syncConfig.enabled,
                  activeProvider: 'github_api',
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: isOn,
                  syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.updateSyncConfig(newConfig)
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // GitHub 远程仓库地址
        Column() {
          Text($r('app.string.github_repo'))
            .fontSize(16)
            .margin({ bottom: 8 })

          TextInput({ text: this.viewModel.syncRemoteUrl })
            .width('100%')
            .height(40)
            .fontSize(14)
            .onChange((value: string) => {
              this.viewModel.updateSyncRemoteUrl(value)
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // 分支
        Column() {
          Text($r('app.string.sync_branch'))
            .fontSize(16)
            .margin({ bottom: 8 })

          TextInput({ text: this.viewModel.syncBranch })
            .width('100%')
            .height(40)
            .fontSize(14)
            .onChange((value: string) => {
              this.viewModel.updateSyncBranch(value)
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // HTTPS Token
        Column() {
          Text($r('app.string.https_token'))
            .fontSize(16)
            .margin({ bottom: 8 })

          TextInput({ text: this.viewModel.syncToken })
            .width('100%')
            .height(40)
            .fontSize(14)
            .type(InputType.Password)
            .showPasswordIcon(true)
            .onChange((value: string) => {
              this.viewModel.updateSyncToken(value)
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // 同步间隔
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
                  activeProvider: 'github_api',
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: this.viewModel.syncConfig.autoSync,
                  syncIntervalSeconds: value,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.updateSyncConfig(newConfig)
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

        // Sync actions
        Row({ space: 12 }) {
          this.SettingsInlineAction($r('app.string.sync_section'),
            () => this.viewModel.performSync(),
            !this.viewModel.isSyncing)

          this.SettingsInlineAction($r('app.string.diagnose'),
            () => this.viewModel.runSyncDiagnostics(),
            !this.viewModel.isDiagnosing)
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
      Column({ space: 12 }) {
        this.SettingsNeutralAction('导出诊断包', async () => {
          const result = await this.viewModel.exportDiagnostics()
          if (result.status === 'success') {
            promptAction.showToast({ message: '诊断包已导出' })
          } else if (result.status === 'failed') {
            promptAction.showToast({ message: '诊断包导出失败' })
          }
        })

        // 清空日志（问题6：ViewModel 内部同时清 Rust diagnostics 和 HiLog buffer）
        this.SettingsNeutralAction('清空日志', () => {
          this.viewModel.clearLogs()
        })

        // 复制设备信息（问题5：调用 copyDeviceInfoToClipboard 真正写入剪贴板）
        this.SettingsNeutralAction('复制设备信息', async () => {
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

  // ── 判断是否需要显示分组标题（单分类时不画分组标题，只画 ExpandableCategory 自己的标题） ──
  private shouldShowGroupHeader(groupDef: GroupDefinition): boolean {
    return groupDef.sections.length > 1
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
          ctx.resourceManager.getStringSync($r('app.string.auto_sync').id),
          ctx.resourceManager.getStringSync($r('app.string.github_repo').id),
          ctx.resourceManager.getStringSync($r('app.string.sync_branch').id),
          ctx.resourceManager.getStringSync($r('app.string.https_token').id),
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

  // ════════════════════════════════════════════
  // Issue #825：宽屏大号悬浮层布局（顶部搜索 + 两列分组）
  // ════════════════════════════════════════════
  @Builder
  OverlaySettingsHeader() {
    Column() {
      Row() {
        Text($r('app.string.settings'))
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
        Blank()
        if (this.viewModel.saveMessage) {
          Text(this.resolveSaveMessage())
            .fontSize(12)
        }
        if (this.copyMessage) {
          Text(this.copyMessage)
            .fontSize(12)
            .margin({ left: 8 })
        }
        Button('关闭', { type: ButtonType.Normal })
          .height(32)
          .fontSize(14)
          .fontColor($r('sys.color.font_primary'))
          .backgroundColor($r('sys.color.comp_background_gray'))
          .borderRadius(DesignTokens.radiusSm)
          .margin({ left: 12 })
          .onClick(() => { this.onRequestClose() })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      Row() {
        Image($r('app.media.ic_search'))
          .width(20)
          .height(20)
          .fillColor($r('sys.color.icon_primary'))
          .margin({ left: 12 })
        TextInput({
          placeholder: '',
          text: this.searchText
        })
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
      .margin({ top: 12, bottom: 4 })
      .backgroundColor($r('sys.color.comp_background_gray'))
      .borderRadius(DesignTokens.radiusSm)
    }
    .width('100%')
    .padding({ left: 24, right: 24, top: 20, bottom: 8 })
  }

  // 把过滤后的分组按列数均分，第一列多一个。
  private columnGroups(index: number): GroupDefinition[] {
    const split = Math.ceil(this.filteredGroups.length / 2)
    return index === 0 ? this.filteredGroups.slice(0, split) : this.filteredGroups.slice(split)
  }

  @Builder
  SettingsGroupColumn(groups: GroupDefinition[], columnIndex: number) {
    Column() {
      ForEach(groups, (groupDef: GroupDefinition) => {
        Column() {
          if (this.shouldShowGroupHeader(groupDef)) {
            this.SettingsGroupHeader(groupDef.id)
          }
          ForEach(groupDef.sections, (sectionId: SettingsSectionId) => {
            this.ExpandableCategory(sectionId)
          }, (sectionId: SettingsSectionId) => sectionId)
        }
        .width('100%')
        .margin({ bottom: 16 })
      }, (groupDef: GroupDefinition) => `c${columnIndex}_${groupDef.id}`)
    }
    .width('100%')
  }

  @Builder
  TwoColumnSettingsLayout() {
    Row() {
      Scroll() {
        this.SettingsGroupColumn(this.columnGroups(0), 0)
      }
      .layoutWeight(1)
      .height('100%')
      .scrollBar(BarState.Off)
      .align(Alignment.Top)
      .padding({ left: this.adaptiveState.pagePadding, right: 12 })

      Scroll() {
        this.SettingsGroupColumn(this.columnGroups(1), 1)
      }
      .layoutWeight(1)
      .height('100%')
      .scrollBar(BarState.Off)
      .align(Alignment.Top)
      .padding({ left: 12, right: this.adaptiveState.pagePadding, bottom: 24 })
    }
    .width('100%')
    .layoutWeight(1)
  }

  build() {
    this.NavContent()
  }
}
