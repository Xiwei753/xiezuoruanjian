// StatsScreen — 统计页入口与共享快照状态（Issue #945）。
//
// 职责边界（遵守根 AGENTS.md 平台端边界）：
// - 只从 Core 的 Explorer DTO（#944）取一份统计快照，不在 ArkTS 重算天数 / 纪录 / 速度；
// - 持有页面状态：selectedPeriod（7d / 30d / 6mo / lifetime / custom）、
//   expandedPanel（none / range / lifetime，宽屏展开位置）、自选起止日期、
//   isLoading 与 errorMessage；宽窄两套正文只读同一份状态，不复制两份；
// - 宽窄分流只按外层传入的 layoutMode；旋转 / 展开设备后沿用
//   selectedPeriod，不重建导航栈（生涯与近七天只是同一页面换快照）；
// - 自选区间用系统日期选择（UIContext.showDatePickerDialog），确认后把用户所选
//   日期原样传给 Core（未来结束日不改写成今天），错误在页内显示。

import hilog from '@ohos.hilog'
import emitter from '@ohos.events.emitter'
import { UIContext } from '@kit.ArkUI'
import { NativeStatsBridge, StatsExplorerRequestResult, StatsHeatmapRequestResult } from '../../../corebridge/native/NativeStatsBridge'
import { SYNC_CONTENT_CHANGED_EVENT_ID } from '../../settings/presentation/SyncOperationController'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import {
  WritingStatsExplorer,
  WritingStatsHeatmap,
  STATS_PRESET_LAST_7_DAYS,
  STATS_PRESET_LIFETIME,
  STATS_PRESET_CUSTOM
} from '../../../corebridge/dto/StatsDtos'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { ScreenRole, ActionSlot, resolveScreenPolicy } from '../../../app/navigation/ScreenSemantics'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsAppDestinationApi23 } from '../../../app/navigation/impl/api23/HdsAppDestinationApi23'
import { buildNavMenuItems } from '../../../app/navigation/NavigationHeaderActionMenu'
import { handleRootBack } from '../../../app/navigation/RootBackHandler'
import { CompactPageState, CompactPageStateKind } from '../../../ui/components/CompactPageState'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../../../platform/diagnostics/HarmonyDiagnosticEvents'
import { STATS_PANEL_NONE, STATS_PANEL_RANGE, STATS_PANEL_LIFETIME, formatIsoDate, parseIsoDate } from './StatsFormat'
import { CompactStatsContent } from './CompactStatsContent'
import { WideStatisticsPage } from './wide/WideStatisticsPage'
import { WideLayoutMode } from '../../../app/navigation/wide/WideLayoutMode'
import { Routes, StatsRouteParams } from '../../../app/navigation/Routes'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'

@Component
export struct StatsScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
  // Issue #972：外壳唯一布局模式，由 PageMapForStack 传入。
  @Prop layoutMode: WideLayoutMode = WideLayoutMode.Compact
  // Core Explorer 快照（生涯摘要 + 四卡 + 选中时段详情），宽窄共用同一份。
  @State explorer: WritingStatsExplorer | null = null
  // 当前选中时段：Core 固定 preset 字符串。
  @State selectedPeriod: string = STATS_PRESET_LAST_7_DAYS
  // 宽屏展开位置：none（四卡总览）/ range（三栏详情）/ lifetime（生涯详情）。
  @State expandedPanel: string = STATS_PANEL_NONE
  // 自选区间：用户所选业务日（YYYY-MM-DD），未选时为空串。
  @State customStartDate: string = ''
  @State customEndDate: string = ''
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  // 码字日历（#956）：独立年份维度，与 Explorer 选中时段并存。
  @State heatmapYear: number = new Date().getFullYear()
  @State heatmap: WritingStatsHeatmap | null = null
  @State heatmapError: string = ''
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  @State useHdsNav: boolean = false
  @State headerActions: ActionSlot[] = []

  private get usesWideLayout(): boolean {
    return this.layoutMode === WideLayoutMode.Wide
  }

  private adaptiveObserver: AdaptiveObserver | null = null
  // Issue #968：最近一次上报的宽窄分流签名，避免重复写诊断。
  private lastReportedLayoutMode: string = ''
  // 请求序号在 Bridge 内维护：旧范围的晚到结果由 isLatestExplorerRequest 过滤。
  private statsBridge: NativeStatsBridge = new NativeStatsBridge(new NativeCoreModule())
  // #936 SYNC_CONTENT_CHANGED 监听器：保存回调本体，退订时只移除本页（emitter.on 返回 void）。
  private syncChangedHandler: ((data: emitter.EventData) => void) | null = null

  async aboutToAppear() {
    hilog.info(0x0000, 'StatsScreen', 'route name=Stats')
    const paramsArr = this.navPathStack.getParamByName(Routes.Stats) as StatsRouteParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params !== null) {
      if (params.period !== undefined) {
        this.selectedPeriod = params.period
      }
      if (params.expandedPanel !== undefined) {
        this.expandedPanel = params.expandedPanel
      }
      if (params.heatmapYear !== undefined) {
        this.heatmapYear = params.heatmapYear
      }
    }
    this.loadExplorer(this.selectedPeriod, '', '')
    this.loadHeatmap(this.heatmapYear)
    this.updateContinuationSnapshot()

    // #936：统计页保活期间，同步下载新内容或重新进入本页时，由同一条事件刷新 Explorer 与当前年份的日历快照。
    this.syncChangedHandler = (_data: emitter.EventData): void => {
      this.refreshStatsSnapshots()
    }
    emitter.on({ eventId: SYNC_CONTENT_CHANGED_EVENT_ID }, this.syncChangedHandler)

    // Issue #784：API23+ 用 HdsAppDestinationApi23 共享根页面壳；API < 23 用 NavDestination .menus() 接入 HeaderTrailing。
    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
    if (!this.useHdsNav) {
      this.headerActions = resolveScreenPolicy(ScreenRole.Stats).actionSlots
    }

    const self = this
    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)
    this.adaptiveState = AdaptiveContext.getInstance().getCurrentState()
  }

  // Issue #988：统计 tab 接续恢复当前时段、宽屏展开位置和日历年份。
  private updateContinuationSnapshot(): void {
    const payload: ContinuationPayload = {
      screen: 'stats',
      tabIndex: 2,
      statsPeriod: this.selectedPeriod,
      statsExpandedPanel: this.expandedPanel,
      statsHeatmapYear: this.heatmapYear
    }
    AppContinuationService.getInstance().updateCurrentScreen(payload)
  }

  // Issue #968：诊断只记录实际挂载的统计正文根容器，而不是根据布尔值预计会挂哪一个。
  // WideStatisticsPage / CompactStatsContent 的根容器 onAppear 回报这里，并带上同一份窗口 vp。
  private recordMountedContent(content: string): void {
    const signature: string = `${this.adaptiveState.widthVp}x${this.adaptiveState.heightVp}:${this.usesWideLayout}:${content}`
    if (signature === this.lastReportedLayoutMode) {
      return
    }
    this.lastReportedLayoutMode = signature
    const fields: DiagnosticField[] = [
      { key: 'widthVp', value: String(this.adaptiveState.widthVp) },
      { key: 'heightVp', value: String(this.adaptiveState.heightVp) },
      { key: 'prefersWideLayout', value: String(this.usesWideLayout) },
      { key: 'mountedContent', value: content },
      { key: 'showTitleBar', value: String(!this.usesWideLayout) }
    ]
    HarmonyDiagnosticEvents.record('Info', 'App', 'layout.stats_content', 'StatsScreen', fields)
  }

  aboutToDisappear() {
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
    if (this.syncChangedHandler !== null) {
      emitter.off(SYNC_CONTENT_CHANGED_EVENT_ID, this.syncChangedHandler)
      this.syncChangedHandler = null
    }
  }

  /**
   * 请求一份 Explorer 快照。只请求 #944 的 Explorer DTO；
   * 旧范围晚到结果（请求序号已过期）直接丢弃，不覆盖当前时段。
   */
  private async loadExplorer(preset: string, customStart: string, customEnd: string): Promise<void> {
    this.isLoading = true
    this.errorMessage = ''
    const response: StatsExplorerRequestResult = await this.statsBridge.getWritingStatsExplorer(preset, customStart, customEnd)
    if (!this.statsBridge.isLatestExplorerRequest(response.requestSeq)) {
      return
    }
    this.isLoading = false
    const result: ResultEnvelope<WritingStatsExplorer> = response.envelope
    if (result.success && result.data !== undefined) {
      this.explorer = result.data
    } else {
      // envelope 只有 errorCode / userMessage（Core 不发明 messageKey）。
      this.errorMessage = result.userMessage !== undefined && result.userMessage.length > 0
        ? result.userMessage
        : (result.errorCode !== undefined ? result.errorCode : '加载统计失败')
    }
  }

  /**
   * 请求指定年份的每日码字快照（#956）。与 Explorer 是两个独立维度：
   * 只加载该年的每日列表，不重算折线；旧请求晚到结果由请求序号过滤。
   */
  private async loadHeatmap(year: number): Promise<void> {
    this.heatmapError = ''
    const response: StatsHeatmapRequestResult = await this.statsBridge.getWritingStatsHeatmap(year)
    if (!this.statsBridge.isLatestHeatmapRequest(response.requestSeq)) {
      return
    }
    const result: ResultEnvelope<WritingStatsHeatmap> = response.envelope
    if (result.success && result.data !== undefined) {
      this.heatmap = result.data
    } else {
      this.heatmap = null
      this.heatmapError = result.userMessage !== undefined && result.userMessage.length > 0
        ? result.userMessage
        : (result.errorCode !== undefined ? result.errorCode : '加载码字日历失败')
    }
  }

  /**
   * #936 统一刷新入口：重新拉取 Explorer 与当前年份的码字日历快照。
   * 两个请求各自的晚到结果由 Bridge 请求序号过滤，导航返回 / 同步内容变更都不会显示旧数据。
   */
  private refreshStatsSnapshots(): void {
    this.loadExplorer(this.selectedPeriod, this.customStartDate, this.customEndDate)
    this.loadHeatmap(this.heatmapYear)
  }

  /** 切换热力图年份：只加载该年快照，不改变 Explorer 选中时段。 */
  private changeHeatmapYear(year: number): void {
    if (year === this.heatmapYear) {
      return
    }
    this.heatmapYear = year
    // 先清空，避免短暂显示上一年数据；加载完成后写入新年份快照。
    this.heatmap = null
    this.loadHeatmap(year)
    this.updateContinuationSnapshot()
  }

  /** 宽屏日历区查询失败后的原地重试：只重取当前年份日历，不打断 Explorer 时段。 */
  private retryHeatmap(): void {
    this.loadHeatmap(this.heatmapYear)
  }

  /** 热力图日格选择：页内文字由组件显示，这里记录选择用于诊断。 */
  private onHeatmapSelectDay(date: string, chars: number): void {
    hilog.debug(0x0000, 'StatsScreen', 'heatmap day selected date=%{public}s chars=%{public}d', date, chars)
  }

  /** 窄屏顶部范围切换 / 宽屏点非自选卡：切时段并重新取快照。 */
  private selectPeriod(preset: string): void {
    this.selectedPeriod = preset
    this.expandedPanel = STATS_PANEL_NONE
    this.loadExplorer(preset, '', '')
    this.updateContinuationSnapshot()
  }

  /** 宽屏点时段卡：切到该时段并在原卡区展开三栏，不导航新页面。 */
  private openPeriodInWide(preset: string): void {
    this.selectedPeriod = preset
    this.expandedPanel = STATS_PANEL_RANGE
    this.loadExplorer(preset, '', '')
    this.updateContinuationSnapshot()
  }

  /** 宽屏点生涯横条：展开 / 收起生涯纪录 + 折线图。 */
  private toggleLifetimePanel(): void {
    if (this.expandedPanel === STATS_PANEL_LIFETIME) {
      this.expandedPanel = STATS_PANEL_NONE
      this.updateContinuationSnapshot()
      return
    }
    this.selectedPeriod = STATS_PRESET_LIFETIME
    this.expandedPanel = STATS_PANEL_LIFETIME
    this.loadExplorer(STATS_PRESET_LIFETIME, '', '')
    this.updateContinuationSnapshot()
  }

  private closeExpandedPanel(): void {
    this.expandedPanel = STATS_PANEL_NONE
  }

  private retrySelected(): void {
    if (this.selectedPeriod === STATS_PRESET_CUSTOM) {
      if (this.customStartDate.length > 0 && this.customEndDate.length > 0) {
        this.loadExplorer(STATS_PRESET_CUSTOM, this.customStartDate, this.customEndDate)
      } else {
        this.openCustomRangePicker()
      }
      return
    }
    this.loadExplorer(this.selectedPeriod, '', '')
  }

  /** 自选区间：先选开始日期，再选结束日期；确认后原样交给 Core。 */
  private openCustomRangePicker(): void {
    const uiContext: UIContext = this.getUIContext()
    uiContext.showDatePickerDialog({
      selected: this.initialPickerDate(this.customStartDate),
      onDateAccept: (startValue: Date): void => {
        const startText: string = formatIsoDate(startValue)
        this.showEndDatePicker(uiContext, startText)
      }
    })
  }

  private showEndDatePicker(uiContext: UIContext, startText: string): void {
    let endInitial: Date = this.initialPickerDate(startText)
    if (this.customEndDate.length > 0) {
      const parsedEnd: Date | null = parseIsoDate(this.customEndDate)
      if (parsedEnd !== null) {
        endInitial = parsedEnd
      }
    }
    uiContext.showDatePickerDialog({
      selected: endInitial,
      onDateAccept: (endValue: Date): void => {
        const endText: string = formatIsoDate(endValue)
        this.customStartDate = startText
        this.customEndDate = endText
        this.selectedPeriod = STATS_PRESET_CUSTOM
        // 宽屏确认自选后在原卡区展开三栏；窄屏保持单页换快照。
        this.expandedPanel = this.usesWideLayout ? STATS_PANEL_RANGE : STATS_PANEL_NONE
        // 用户所选日期原样传 Core：未来结束日不在这里改写成今天。
        this.loadExplorer(STATS_PRESET_CUSTOM, startText, endText)
      }
    })
  }

  /** 日期选择器的初始位置：优先已有自选，其次生涯首次写作日，否则今天。 */
  private initialPickerDate(existing: string): Date {
    const parsed: Date | null = parseIsoDate(existing)
    if (parsed !== null) {
      return parsed
    }
    if (this.explorer !== null && this.explorer.lifetime.firstWritingDate !== null) {
      const first: Date | null = parseIsoDate(this.explorer.lifetime.firstWritingDate)
      if (first !== null) {
        return first
      }
    }
    return new Date()
  }

  /**
   * 页面内容：加载 / 错误状态宽窄共用；拿到快照后按窗口形态分流——
   * 宽窄两套只读同一份 explorer / selectedPeriod / expandedPanel。
   */
  @Builder
  StatsContent() {
    Column() {
      if (this.explorer === null) {
        if (this.isLoading) {
          CompactPageState({
            kind: CompactPageStateKind.Loading,
            title: '加载中...'
          })
        } else {
          CompactPageState({
            kind: CompactPageStateKind.Error,
            title: this.errorMessage.length > 0 ? this.errorMessage : '加载统计失败',
            onRetry: () => {
              this.retrySelected()
            }
          })
        }
      } else if (this.usesWideLayout) {
        WideStatisticsPage({
          explorer: this.explorer,
          selectedPeriod: this.selectedPeriod,
          expandedPanel: this.expandedPanel,
          isLoading: this.isLoading,
          errorMessage: this.errorMessage,
          heatmap: this.heatmap,
          heatmapYear: this.heatmapYear,
          heatmapError: this.heatmapError,
          onMounted: (): void => {
            this.recordMountedContent('WideStatisticsPage')
          },
          onChangeHeatmapYear: (year: number): void => {
            this.changeHeatmapYear(year)
          },
          onSelectHeatmapDay: (date: string, chars: number): void => {
            this.onHeatmapSelectDay(date, chars)
          },
          onOpenPeriod: (preset: string): void => {
            this.openPeriodInWide(preset)
          },
          onPickCustomRange: (): void => {
            this.openCustomRangePicker()
          },
          onToggleLifetime: (): void => {
            this.toggleLifetimePanel()
          },
          onCloseExpanded: (): void => {
            this.closeExpandedPanel()
          },
          onRetry: (): void => {
            this.retrySelected()
          },
          onRetryHeatmap: (): void => {
            this.retryHeatmap()
          }
        })
      } else {
        CompactStatsContent({
          explorer: this.explorer,
          selectedPeriod: this.selectedPeriod,
          isLoading: this.isLoading,
          errorMessage: this.errorMessage,
          heatmap: this.heatmap,
          heatmapYear: this.heatmapYear,
          heatmapError: this.heatmapError,
          onMounted: (): void => {
            this.recordMountedContent('CompactStatsContent')
          },
          onChangeHeatmapYear: (year: number): void => {
            this.changeHeatmapYear(year)
          },
          onSelectHeatmapDay: (date: string, chars: number): void => {
            this.onHeatmapSelectDay(date, chars)
          },
          onSelectPeriod: (preset: string): void => {
            this.selectPeriod(preset)
          },
          onPickCustomRange: (): void => {
            this.openCustomRangePicker()
          },
          onRetry: (): void => {
            this.retrySelected()
          }
        })
      }
    }
    .width('100%')
    .height('100%')
    // 与首页同口径：页面底色用系统 background_secondary（#F2F3F5），让悬浮底栏有底可滤。
    // 官方《组件颜色与界面背景颜色相近》faqs-arkui-1119。
    .backgroundColor($r('sys.color.background_secondary'))
  }

  build() {
    // Issue #784：API23+ 用 HdsAppDestinationApi23 共享根页面壳消费 HeaderTrailing；
    // API < 23 用 NavDestination .menus() 接入 HeaderTrailing 动作。
    // 页面内不新增导航：返回 / 动作走统一导航壳，分子页返回由 expandedPanel 状态表达。
    if (this.useHdsNav) {
      HdsAppDestinationApi23({
        title: '统计',
        screenRole: ScreenRole.Stats,
        screenTabIndex: 2,
        showTitleBar: !this.usesWideLayout,
        onShown: (): void => {
          this.refreshStatsSnapshots()
        }
      }) {
        this.StatsContent()
      }
    } else {
      NavDestination() {
        this.StatsContent()
      }
      .hideTitleBar(this.usesWideLayout)
      .hideBackButton(true)
      .onBackPressed(() => handleRootBack())
      .title('统计')
      .menus(buildNavMenuItems(this.headerActions))
      .onShown(() => {
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.Stats, 2)
        this.refreshStatsSnapshots()
      })
    }
  }
}
