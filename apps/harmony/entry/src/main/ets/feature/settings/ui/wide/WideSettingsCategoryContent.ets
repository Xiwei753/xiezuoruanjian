// WideSettingsCategoryContent — 宽屏设置浮层里的分类正文（Issue #972 评论）。
//
// 浮层外框、分类卡头由 WideSettingsOverlay / WideSettingsSection 拥有；
// 这里只画宽屏自己的设置项排版：两列信息行、宽控件、分组内的说明与操作。
// 数据与副作用仍全部走现有 SettingsViewModel / 主题适配器 / 同步控制器，
// 不复制窄屏 SectionContent 的视觉 builder。

import { BuiltinThemeDto, LocalSettings, ThemePaletteRecordDto } from '../../../../corebridge/dto/SettingsDtos'
import { SyncConfig } from '../../../../corebridge/dto/SyncDtos'
import { ThemeMode } from '../../../../ui/theme/HarmonyThemeTypes'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { MessageKeyMapper } from '../../../../ui/i18n/MessageKeyMapper'
import { SettingsSectionId, SettingsViewModel } from '../../presentation/SettingsViewModel'

interface WideSelectOption {
  value: string
  icon: string
}

@ComponentV2
export struct WideSettingsCategoryContent {
  @Param sectionId: SettingsSectionId = 'appearance'
  @Param viewModel: SettingsViewModel = new SettingsViewModel()

  // 主题变化涉及全局适配器和持久化，由 SettingsContent 统一执行。
  @Param onSetThemeMode: (mode: ThemeMode) => void = (_mode: ThemeMode) => {}
  @Param onSetColorSource: (source: string) => void = (_source: string) => {}
  @Param onSetBuiltinTheme: (themeId: string) => void = (_themeId: string) => {}
  @Param onSetPalette: (paletteId: string) => void = (_paletteId: string) => {}
  @Param onRequestSync: () => void = () => {}
  @Param onOpenConflicts: () => void = () => {}
  @Param onOpenThirdPartyNotices: () => void = () => {}
  @Param onExportDiagnostics: () => void = () => {}
  @Param onClearLogs: () => void = () => {}
  @Param onCopyDeviceInfo: () => void = () => {}
  @Param conflictRemaining: number = -1

  private themeLabels(): string[] {
    return [
      getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id),
      getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id),
      getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id)
    ]
  }

  private themeSelectOptions(): WideSelectOption[] {
    const labels: string[] = this.themeLabels()
    const options: WideSelectOption[] = []
    for (let index: number = 0; index < labels.length; index++) {
      options.push({ value: labels[index], icon: '' })
    }
    return options
  }

  private colorSourceOptions(): WideSelectOption[] {
    return [
      { value: '素笺默认', icon: '' },
      { value: '已保存的设备配色', icon: '' }
    ]
  }

  private builtinThemeSelectOptions(): WideSelectOption[] {
    const options: WideSelectOption[] = []
    for (let index: number = 0; index < this.viewModel.builtinThemes.length; index++) {
      options.push({ value: this.viewModel.builtinThemes[index].name, icon: '' })
    }
    return options
  }

  private paletteSelectOptions(): WideSelectOption[] {
    const labels: string[] = this.paletteOptions()
    const options: WideSelectOption[] = []
    for (let index: number = 0; index < labels.length; index++) {
      options.push({ value: labels[index], icon: '' })
    }
    return options
  }

  private themeIndex(): number {
    const mode: string = this.viewModel.settings.appearanceMode || this.viewModel.settings.themeMode || 'system'
    const modes: string[] = ['system', 'light', 'dark']
    const index: number = modes.indexOf(mode)
    return index >= 0 ? index : 0
  }

  private builtinThemeIndex(): number {
    const id: string = this.viewModel.settings.selectedBuiltinThemeId || ''
    if (id.length === 0) {
      return 0
    }
    const index: number = this.viewModel.builtinThemes.findIndex((theme: BuiltinThemeDto) => theme.theme_id === id)
    return index >= 0 ? index : 0
  }

  private builtinThemeName(): string {
    const id: string = this.viewModel.settings.selectedBuiltinThemeId || ''
    if (id.length === 0) {
      return '素笺默认'
    }
    const theme: BuiltinThemeDto | undefined =
      this.viewModel.builtinThemes.find((item: BuiltinThemeDto) => item.theme_id === id)
    return theme ? theme.name : '素笺默认'
  }

  private paletteOptions(): string[] {
    return this.viewModel.paletteRecords.map((record: ThemePaletteRecordDto): string => {
      const date: Date = new Date(record.captured_at_ms)
      const dateText: string = `${date.getFullYear()}-${(date.getMonth() + 1).toString().padStart(2, '0')}-${date.getDate().toString().padStart(2, '0')}`
      return `${record.source_device_class} · ${dateText}`
    })
  }

  private paletteIndex(): number {
    const id: string = this.viewModel.settings.selectedPaletteId || ''
    if (id.length === 0) {
      return 0
    }
    const index: number = this.viewModel.paletteRecords.findIndex((record: ThemePaletteRecordDto) => record.palette_id === id)
    return index >= 0 ? index : 0
  }

  private paletteName(): string {
    const options: string[] = this.paletteOptions()
    if (options.length === 0) {
      return ''
    }
    return options[this.paletteIndex()]
  }

  private conflictLabel(): string {
    if (this.conflictRemaining > 0) {
      return `同步冲突 ${this.conflictRemaining} 处，查看并解决`
    }
    return '查看同步冲突'
  }

  build() {
    Column({ space: 4 }) {
      if (this.sectionId === 'appearance') {
        this.AppearanceRows()
      } else if (this.sectionId === 'editor') {
        this.EditorRows()
      } else if (this.sectionId === 'save') {
        this.SaveRows()
      } else if (this.sectionId === 'sync') {
        this.SyncRows()
      } else if (this.sectionId === 'ai') {
        this.AiRows()
      } else if (this.sectionId === 'diagnostics') {
        this.DiagnosticsRows()
      } else if (this.sectionId === 'about') {
        this.AboutRows()
      } else if (this.sectionId === 'recovery') {
        this.PlaceholderRows('恢复功能即将上线')
      } else if (this.sectionId === 'laboratory') {
        this.PlaceholderRows('实验室功能即将上线')
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  // ── 宽屏设置行：左侧标题 + 右侧控件，给控件留出稳定宽度 ──
  @Builder
  WideSettingRow(label: string, valueText: string) {
    Row({ space: 12 }) {
      Column({ space: 2 }) {
        Text(label)
          .fontSize(15)
          .fontColor($r('sys.color.font_primary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
        if (valueText.length > 0) {
          Text(valueText)
            .fontSize(11)
            .fontColor($r('sys.color.font_tertiary'))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  AppearanceRows() {
    // 主题模式
    Row({ space: 12 }) {
      Text($r('app.string.theme_mode'))
        .fontSize(15)
        .layoutWeight(1)
      Select(this.themeSelectOptions())
        .selected(this.themeIndex())
        .value(this.themeLabels()[this.themeIndex()])
        .width('32%')
        .onSelect((index: number) => {
          const modes: ThemeMode[] = ['system', 'light', 'dark']
          this.onSetThemeMode(modes[index])
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)

    // 颜色来源（实验性）
    Row({ space: 12 }) {
      Text('颜色主题（实验性）')
        .fontSize(15)
        .layoutWeight(1)
      Select(this.colorSourceOptions())
        .selected(this.viewModel.settings.colorSource === 'saved_palette' ? 1 : 0)
        .value(this.viewModel.settings.colorSource === 'saved_palette' ? '已保存的设备配色' : '素笺默认')
        .width('32%')
        .onSelect((index: number) => {
          this.onSetColorSource(index === 1 ? 'saved_palette' : 'built_in')
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)

    if (this.viewModel.settings.colorSource === 'built_in' && this.viewModel.builtinThemes.length > 0) {
      Row({ space: 12 }) {
        Text('内置主题（实验性）')
          .fontSize(15)
          .layoutWeight(1)
        Select(this.builtinThemeSelectOptions())
          .selected(this.builtinThemeIndex())
          .value(this.builtinThemeName())
          .width('32%')
          .onSelect((index: number) => {
            if (index < this.viewModel.builtinThemes.length) {
              this.onSetBuiltinTheme(this.viewModel.builtinThemes[index].theme_id)
            }
          })
      }
      .width('100%')
      .padding({ top: 7, bottom: 7 })
      .alignItems(VerticalAlign.Center)
    }

    if (this.viewModel.settings.colorSource === 'saved_palette' && this.viewModel.paletteRecords.length > 0) {
      Row({ space: 12 }) {
        Text('设备配色（实验性）')
          .fontSize(15)
          .layoutWeight(1)
        Select(this.paletteSelectOptions())
          .selected(this.paletteIndex())
          .value(this.paletteName())
          .width('32%')
          .onSelect((index: number) => {
            if (index < this.viewModel.paletteRecords.length) {
              this.onSetPalette(this.viewModel.paletteRecords[index].palette_id)
            }
          })
      }
      .width('100%')
      .padding({ top: 7, bottom: 7 })
      .alignItems(VerticalAlign.Center)
    }

    // 字体大小
    Row({ space: 12 }) {
      Column({ space: 2 }) {
        Text($r('app.string.font_size'))
          .fontSize(15)
        Text(`${this.viewModel.settings.editorFontSize}px`)
          .fontSize(11)
          .fontColor($r('sys.color.font_tertiary'))
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)
      Slider({ value: this.viewModel.settings.editorFontSize, min: 12, max: 32, step: 1 })
        .width('42%')
        .onChange((value: number) => {
          this.viewModel.updateSettings((settings: LocalSettings): LocalSettings => {
            settings.editorFontSize = value
            return settings
          })
          this.viewModel.saveSettings()
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)

    // 行距倍数
    Row({ space: 12 }) {
      Column({ space: 2 }) {
        Text($r('app.string.line_spacing_multiplier'))
          .fontSize(15)
        Text(this.viewModel.settings.editorLineSpacingMultiplier.toFixed(1))
          .fontSize(11)
          .fontColor($r('sys.color.font_tertiary'))
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)
      Slider({ value: this.viewModel.settings.editorLineSpacingMultiplier, min: 1.0, max: 3.0, step: 0.1 })
        .width('42%')
        .onChange((value: number) => {
          const rounded: number = Math.round(value * 10) / 10
          this.viewModel.updateSettings((settings: LocalSettings): LocalSettings => {
            settings.editorLineSpacingMultiplier = rounded
            return settings
          })
          this.viewModel.saveSettings()
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  EditorRows() {
    Row({ space: 12 }) {
      Text($r('app.string.auto_indent'))
        .fontSize(15)
        .layoutWeight(1)
      Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.autoIndentEnabled })
        .onChange((isOn: boolean) => {
          this.viewModel.updateSettings((settings: LocalSettings): LocalSettings => {
            settings.autoIndentEnabled = isOn
            return settings
          })
          this.viewModel.saveSettings()
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)

    Row({ space: 12 }) {
      Column({ space: 2 }) {
        Text($r('app.string.indent_width'))
          .fontSize(15)
        Text(`${this.viewModel.settings.autoIndentWidth}`)
          .fontSize(11)
          .fontColor($r('sys.color.font_tertiary'))
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)
      Slider({ value: this.viewModel.settings.autoIndentWidth, min: 0, max: 8, step: 1 })
        .width('42%')
        .onChange((value: number) => {
          this.viewModel.updateSettings((settings: LocalSettings): LocalSettings => {
            settings.autoIndentWidth = value
            return settings
          })
          this.viewModel.saveSettings()
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  SaveRows() {
    Row({ space: 12 }) {
      Text($r('app.string.auto_save'))
        .fontSize(15)
        .layoutWeight(1)
      Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.autoSaveEnabled })
        .onChange((isOn: boolean) => {
          this.viewModel.updateSettings((settings: LocalSettings): LocalSettings => {
            settings.autoSaveEnabled = isOn
            return settings
          })
          this.viewModel.saveSettings()
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)

    if (this.viewModel.settings.autoSaveEnabled) {
      Row({ space: 12 }) {
        Column({ space: 2 }) {
          Text($r('app.string.auto_save_delay'))
            .fontSize(15)
          Text(`${Math.round(this.viewModel.settings.autoSaveDelayMs / 1000)} 秒`)
            .fontSize(11)
            .fontColor($r('sys.color.font_tertiary'))
        }
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Start)
        Slider({ value: this.viewModel.settings.autoSaveDelayMs / 1000, min: 1, max: 10, step: 1 })
          .width('42%')
          .onChange((value: number) => {
            this.viewModel.updateSettings((settings: LocalSettings): LocalSettings => {
              settings.autoSaveDelayMs = value * 1000
              return settings
            })
            this.viewModel.saveSettings()
          })
      }
      .width('100%')
      .padding({ top: 7, bottom: 7 })
      .alignItems(VerticalAlign.Center)
    }
  }

  @Builder
  SyncRows() {
    Row({ space: 12 }) {
      Text($r('app.string.enable_sync'))
        .fontSize(15)
        .layoutWeight(1)
      Toggle({ type: ToggleType.Switch, isOn: this.viewModel.syncConfig?.enabled || false })
        .onChange((isOn: boolean) => {
          const config: SyncConfig | null = this.viewModel.syncConfig
          if (config) {
            const next: SyncConfig = {
              enabled: isOn,
              activeProvider: 'github_api',
              providerConfig: config.providerConfig,
              autoSync: config.autoSync,
              syncIntervalSeconds: config.syncIntervalSeconds,
              hasNetworkPermission: config.hasNetworkPermission,
              hasNetworkStatePermission: config.hasNetworkStatePermission
            }
            this.viewModel.updateSyncConfig(next)
          }
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)

    if (this.viewModel.syncConfig?.enabled) {
      Row({ space: 12 }) {
        Text($r('app.string.auto_sync'))
          .fontSize(15)
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.viewModel.syncConfig?.autoSync || false })
          .onChange((isOn: boolean) => {
            const config: SyncConfig | null = this.viewModel.syncConfig
            if (config) {
              const next: SyncConfig = {
                enabled: config.enabled,
                activeProvider: 'github_api',
                providerConfig: config.providerConfig,
                autoSync: isOn,
                syncIntervalSeconds: config.syncIntervalSeconds,
                hasNetworkPermission: config.hasNetworkPermission,
                hasNetworkStatePermission: config.hasNetworkStatePermission
              }
              this.viewModel.updateSyncConfig(next)
            }
          })
      }
      .width('100%')
      .padding({ top: 7, bottom: 7 })
      .alignItems(VerticalAlign.Center)

      Column({ space: 6 }) {
        Text($r('app.string.github_repo'))
          .fontSize(13)
          .fontColor($r('sys.color.font_secondary'))
        TextInput({ text: this.viewModel.syncRemoteUrl })
          .width('100%')
          .fontSize(13)
          .borderRadius(DesignTokens.inputFieldRadius)
          .onChange((value: string) => {
            this.viewModel.updateSyncRemoteUrl(value)
          })
      }
      .width('100%')
      .padding({ top: 7, bottom: 7 })
      .alignItems(HorizontalAlign.Start)

      Column({ space: 6 }) {
        Text($r('app.string.https_token'))
          .fontSize(13)
          .fontColor($r('sys.color.font_secondary'))
        TextInput({ text: this.viewModel.syncToken })
          .width('100%')
          .fontSize(13)
          .borderRadius(DesignTokens.inputFieldRadius)
          .type(InputType.Password)
          .showPasswordIcon(true)
          .onChange((value: string) => {
            this.viewModel.updateSyncToken(value)
          })
          .onBlur(() => {
            void this.viewModel.commitSyncToken()
          })
          .onSubmit(() => {
            void this.viewModel.commitSyncToken()
          })
      }
      .width('100%')
      .padding({ top: 7, bottom: 7 })
      .alignItems(HorizontalAlign.Start)

      Row({ space: 12 }) {
        Column({ space: 2 }) {
          Text($r('app.string.sync_interval'))
            .fontSize(15)
          Text(`${this.viewModel.syncConfig?.syncIntervalSeconds || 300} 秒`)
            .fontSize(11)
            .fontColor($r('sys.color.font_tertiary'))
        }
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Start)
        Slider({ value: this.viewModel.syncConfig?.syncIntervalSeconds || 300, min: 60, max: 3600, step: 60 })
          .width('42%')
          .onChange((value: number) => {
            const config: SyncConfig | null = this.viewModel.syncConfig
            if (config) {
              const next: SyncConfig = {
                enabled: config.enabled,
                activeProvider: 'github_api',
                providerConfig: config.providerConfig,
                autoSync: config.autoSync,
                syncIntervalSeconds: value,
                hasNetworkPermission: config.hasNetworkPermission,
                hasNetworkStatePermission: config.hasNetworkStatePermission
              }
              this.viewModel.updateSyncConfig(next)
            }
          })
      }
      .width('100%')
      .padding({ top: 7, bottom: 7 })
      .alignItems(VerticalAlign.Center)

      Row({ space: 10 }) {
        Button($r('app.string.sync_section'))
          .fontSize(13)
          .onClick(() => {
            this.onRequestSync()
          })
        Button($r('app.string.diagnose'))
          .fontSize(13)
          .enabled(!this.viewModel.isDiagnosing)
          .onClick(() => {
            this.viewModel.runSyncDiagnostics()
          })
        Blank()
      }
      .width('100%')
      .padding({ top: 7, bottom: 7 })

      if (this.conflictRemaining !== 0) {
        Button(this.conflictLabel())
          .fontSize(13)
          .backgroundColor(Color.Transparent)
          .fontColor($r('sys.color.font_emphasize'))
          .padding({ left: 0, right: 0 })
          .onClick(() => {
            this.onOpenConflicts()
          })
      }

      // Issue #972 复核：读取现有 ViewModel 的诊断结果，不重新发起诊断。
      if (this.viewModel.syncDiagnostics) {
        this.SyncDiagnosticsCard()
      }
    }
  }

  @Builder
  SyncDiagnosticsCard() {
    Column({ space: 8 }) {
      Text($r('app.string.diagnostics_result'))
        .fontSize(14)
        .fontWeight(FontWeight.Medium)
        .width('100%')

      Row() {
        this.SyncDiagnosticItem($r('app.string.network'), this.viewModel.syncDiagnostics!.networkOk,
          this.viewModel.syncDiagnostics!.networkStatus)
        this.SyncDiagnosticItem($r('app.string.auth'), this.viewModel.syncDiagnostics!.authOk,
          this.viewModel.syncDiagnostics!.authStatus)
        // SyncDiagnosticsResult 没有 remoteStatus 字段；仓库状态只能由 remoteOk 单独给出，
        // 不能复用 networkStatus，否则网络正常但仓库失败时会显示自相矛盾的文案。
        this.SyncDiagnosticItem($r('app.string.repository'), this.viewModel.syncDiagnostics!.remoteOk,
          this.viewModel.syncDiagnostics!.remoteOk ? '可访问' : '不可访问')
      }
      .width('100%')
      .justifyContent(FlexAlign.SpaceAround)

      if ((this.viewModel.syncDiagnostics!.errorCategory ?? '').length > 0) {
        Text(MessageKeyMapper.resolveMessage(getContext(this),
          `error.${this.viewModel.syncDiagnostics!.errorCategory}`, undefined, undefined))
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
          .width('100%')
        Text(`错误分类：${this.viewModel.syncDiagnostics!.errorCategory}`)
          .fontSize(11)
          .fontColor($r('sys.color.font_tertiary'))
          .width('100%')
      } else if ((this.viewModel.syncDiagnostics!.rawError ?? this.viewModel.syncDiagnostics!.providerDetails ?? '').length > 0) {
        Text(this.viewModel.syncDiagnostics!.rawError ?? this.viewModel.syncDiagnostics!.providerDetails ?? '')
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
          .width('100%')
      }
    }
    .width('100%')
    .padding(10)
    .margin({ top: 6 })
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor($r('sys.color.comp_background_gray'))
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  SyncDiagnosticItem(label: ResourceStr, ok: boolean, status: string) {
    Column() {
      Text(ok ? 'OK' : 'FAIL')
        .fontSize(13)
        .fontWeight(FontWeight.Bold)
        .fontColor(ok ? $r('sys.color.font_primary') : '#E84026')
      Text(label)
        .fontSize(11)
        .margin({ top: 2 })
      if (status.length > 0) {
        Text(status)
          .fontSize(10)
          .fontColor($r('sys.color.font_tertiary'))
          .margin({ top: 2 })
      }
    }
    .alignItems(HorizontalAlign.Center)
  }

  @Builder
  AiRows() {
    Row({ space: 12 }) {
      Column({ space: 2 }) {
        Text($r('app.string.enable_ai'))
          .fontSize(15)
        Text('AI 能力尚未接入写作工具面板')
          .fontSize(11)
          .fontColor($r('sys.color.font_tertiary'))
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)
      Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.aiEnabled })
        .onChange((isOn: boolean) => {
          this.viewModel.updateSettings((settings: LocalSettings): LocalSettings => {
            settings.aiEnabled = isOn
            return settings
          })
          this.viewModel.saveSettings()
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  DiagnosticsRows() {
    Row({ space: 12 }) {
      Text('开启诊断日志')
        .fontSize(15)
        .layoutWeight(1)
      Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.diagnosticsEnabled })
        .onChange((isOn: boolean) => {
          this.viewModel.updateSettings((settings: LocalSettings): LocalSettings => {
            settings.diagnosticsEnabled = isOn
            if (!isOn) {
              settings.diagnosticsVerbose = false
            }
            return settings
          })
          this.viewModel.saveSettings()
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)

    Row({ space: 12 }) {
      Text('详细日志')
        .fontSize(15)
        .layoutWeight(1)
      Toggle({ type: ToggleType.Switch, isOn: this.viewModel.settings.diagnosticsVerbose })
        .enabled(this.viewModel.settings.diagnosticsEnabled)
        .onChange((isOn: boolean) => {
          this.viewModel.updateSettings((settings: LocalSettings): LocalSettings => {
            settings.diagnosticsVerbose = isOn
            return settings
          })
          this.viewModel.saveSettings()
        })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)

    Row({ space: 10 }) {
      Button('导出诊断包')
        .fontSize(13)
        .onClick(() => {
          this.onExportDiagnostics()
        })
      Button('清空日志')
        .fontSize(13)
        .onClick(() => {
          this.onClearLogs()
        })
      Button('复制设备信息')
        .fontSize(13)
        .onClick(() => {
          this.onCopyDeviceInfo()
        })
      Blank()
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
  }

  @Builder
  AboutRows() {
    this.AboutRow($r('app.string.about_app_name_label'), $r('app.string.about_app_name'))
    this.AboutRow($r('app.string.about_author_label'), $r('app.string.about_author'))
    this.AboutRow($r('app.string.about_license_label'), $r('app.string.about_license'))

    Column({ space: 4 }) {
      Text($r('app.string.about_project_label'))
        .fontSize(13)
        .fontColor($r('sys.color.font_secondary'))
      Text('GitHub: github.com/Xiwei753/xiezuoruanjian')
        .fontSize(13)
      Text($r('app.string.about_gitee'))
        .fontSize(13)
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(HorizontalAlign.Start)

    Button($r('app.string.about_third_party_resources'))
      .fontSize(13)
      .backgroundColor(Color.Transparent)
      .fontColor($r('sys.color.font_emphasize'))
      .padding({ left: 0, right: 0 })
      .onClick(() => {
        this.onOpenThirdPartyNotices()
      })

    Text($r('app.string.about_mirror_note'))
      .fontSize(12)
      .fontColor($r('sys.color.font_tertiary'))
      .margin({ top: 6, bottom: 6 })

    this.AboutRow($r('app.string.version_info'), $r('app.string.about_platform'))

    Row({ space: 12 }) {
      Text($r('app.string.workspace_path'))
        .fontSize(15)
        .layoutWeight(1)
      Text('—')
        .fontSize(13)
        .fontColor($r('sys.color.font_secondary'))
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  AboutRow(label: Resource, value: Resource) {
    Row({ space: 12 }) {
      Text(label)
        .fontSize(15)
        .layoutWeight(1)
      Text(value)
        .fontSize(13)
        .fontColor($r('sys.color.font_secondary'))
        .maxLines(2)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
    }
    .width('100%')
    .padding({ top: 7, bottom: 7 })
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  PlaceholderRows(text: string) {
    Text(text)
      .fontSize(13)
      .fontColor($r('sys.color.font_secondary'))
      .padding({ top: 8, bottom: 8 })
  }
}
