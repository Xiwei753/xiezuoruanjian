// StatsScreen — 统计页 UI。
// 数据来自 Core 的 NativeStatsBridge（getWritingStats），仅做 UI 展示，
// 不复制业务状态机（遵守根 AGENTS.md 平台端边界）。

import hilog from '@ohos.hilog'
import { NativeStatsBridge } from '../../../corebridge/native/NativeStatsBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { WritingStats } from '../../../corebridge/dto/ProjectDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { HarmonyThemeAdapter, ThemeConfig } from '../../../ui/theme/HarmonyThemeAdapter'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { ScreenRole, ActionSlot, resolveScreenPolicy } from '../../../app/navigation/ScreenSemantics'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsAppDestinationApi23 } from '../../../app/navigation/impl/api23/HdsAppDestinationApi23'
import { buildNavMenuItems } from '../../../app/navigation/NavigationHeaderActionMenu'
import { handleRootBack } from '../../../app/navigation/RootBackHandler'
// Issue #932：窄屏统计页统一页面底色 / 边距 / 分组卡度量，并复用公共加载/错误状态。
// Issue #932 复核：复核要求本次只统一窄屏——宽屏统计恢复改动前（f3154e32~1）的裸行形态，
// 因此统计正文拆成 CompactStatsContent / WideStatsContent，两者共用同一份统计数值与 StatRow。
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import {
  COMPACT_CARD_PADDING_VP,
  COMPACT_CARD_SPACING_VP,
  COMPACT_CARD_SUPPORTING_FONT_VP,
  COMPACT_CARD_TITLE_FONT_VP,
  COMPACT_LIST_BOTTOM_INSET_VP
} from '../../../ui/theme/LayoutTokens'
import { CompactPageState, CompactPageStateKind } from '../../../ui/components/CompactPageState'

// Issue #932 复核：宽屏统计行沿用改动前（f3154e32~1）的几何——16vp 字号、12vp 上下行距，
// 标签沿用默认前景色；窄屏行继续用 COMPACT_CARD_* 卡内度量。宽窄共用同一个 StatRow 行构件。
const WIDE_STAT_ROW_FONT_VP: number = 16
const WIDE_STAT_ROW_VERTICAL_PADDING_VP: number = 12

@Component
export struct StatsScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
  @State stats: WritingStats | null = null
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  @State useHdsNav: boolean = false
  @State headerActions: ActionSlot[] = []

  private adaptiveObserver: AdaptiveObserver | null = null

  async aboutToAppear() {
    hilog.info(0x0000, 'StatsScreen', 'route name=Stats')
    this.loadStats()

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

  aboutToDisappear() {
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  async loadStats() {
    this.isLoading = true
    this.errorMessage = ''
    try {
      const bridge = new NativeStatsBridge(new NativeCoreModule())
      const result: ResultEnvelope<WritingStats> = await bridge.getWritingStats()
      if (result.success && result.data) {
        this.stats = result.data
      } else {
        // envelope 只有 errorCode / userMessage（Core 不发明 messageKey）。
        this.errorMessage = result.userMessage || result.errorCode || '加载统计失败'
      }
    } catch (err) {
      this.errorMessage = `加载统计失败: ${err}`
    } finally {
      this.isLoading = false
    }
  }

  // ─── Issue #932：统计行构件 + 窄屏分组卡 / 宽屏旧形态 ───

  /**
   * 指标行：左标签右数值，宽窄两形态共用同一行构件，只做视觉呈现，不重算任何统计口径。
   * - 窄屏（wideForm = false）：沿用窄屏卡片 token（Issue #932 窄屏统一结果）。
   * - 宽屏（wideForm = true）：沿用改动前 f3154e32~1 的 16vp 字号 / 12vp 行距 / 默认标签色，
   *   不把窄屏卡内度量画到宽屏。
   */
  @Builder
  StatRow(label: string, value: string, wideForm: boolean) {
    Row() {
      Text(label)
        .fontSize(wideForm ? WIDE_STAT_ROW_FONT_VP : COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor(wideForm ? $r('sys.color.font_primary') : $r('sys.color.font_secondary'))
        .layoutWeight(1)
      Text(value)
        .fontSize(wideForm ? WIDE_STAT_ROW_FONT_VP : COMPACT_CARD_TITLE_FONT_VP)
        .fontWeight(FontWeight.Medium)
    }
    .width('100%')
    .padding({
      top: wideForm ? WIDE_STAT_ROW_VERTICAL_PADDING_VP : 8,
      bottom: wideForm ? WIDE_STAT_ROW_VERTICAL_PADDING_VP : 8
    })
  }

  /**
   * 页面内容：加载 / 错误状态与页面底色宽窄共用（与卡片几何无关），统计正文按窗口形态分流。
   * Issue #932 复核：窄屏走统一后的分组卡形态，宽屏保持改动前（f3154e32~1）的旧形态。
   */
  @Builder
  StatsContent() {
    Column() {
      if (this.isLoading) {
        CompactPageState({
          kind: CompactPageStateKind.Loading,
          title: '加载中...'
        })
      } else if (this.errorMessage) {
        CompactPageState({
          kind: CompactPageStateKind.Error,
          title: this.errorMessage,
          onRetry: () => {
            this.loadStats()
          }
        })
      } else if (this.stats) {
        // Issue #932 复核：本次只统一窄屏。宽屏走恢复后的旧形态，不把窄屏分组卡与
        // COMPACT_* 卡内度量画到宽屏；两个形态吃同一个 stats，统计口径只有一份。
        if (this.adaptiveState.prefersWideLayout) {
          this.WideStatsContent(this.stats)
        } else {
          this.CompactStatsContent(this.stats)
        }
      }
    }
    .width('100%')
    .height('100%')
    // 与首页同口径：页面底色用系统 background_secondary（#F2F3F5），让悬浮底栏有底可滤。
    // 官方《组件颜色与界面背景颜色相近》faqs-arkui-1119。
    .backgroundColor($r('sys.color.background_secondary'))
  }

  /** 窄屏统计（Issue #932 统一结果）：统计范围 / 字数指标 / 活跃指标三张分组卡。 */
  @Builder
  CompactStatsContent(stats: WritingStats) {
    Scroll() {
      Column() {
        // Issue #753 评论 5814008597 问题2：WritingStats 已换成 Core 的 WritingStatsSummaryDto（最近 30 天汇总）。
        // 旧字段 totalChars/totalWords/totalChapters/todayChars/todayDuration/averageSpeed 全部不存在，
        // 改为展示新 DTO 的全部字段，并在首行展示统计范围。
        // Issue #932：统计指标不再裸露 Row 一直铺到底，改为分组卡呈现。
        Column() {
          this.StatRow('统计范围', `${stats.range.startDate} ~ ${stats.range.endDate}`, false)
        }
        .width('100%')
        .padding(COMPACT_CARD_PADDING_VP)
        .borderRadius(DesignTokens.cardRadius)
        .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
        .margin({ bottom: COMPACT_CARD_SPACING_VP })

        // 字数指标分组卡
        Column() {
          this.StatRow('手写字数', `${stats.totalHumanTypedChars}`, false)
          this.StatRow('粘贴字数', `${stats.totalPastedChars}`, false)
          this.StatRow('删除字数', `${stats.totalDeletedChars}`, false)
          this.StatRow('AI插入字数', `${stats.totalAiInsertedChars}`, false)
          this.StatRow('净增字数', `${stats.totalNetDeltaChars}`, false)
        }
        .width('100%')
        .padding(COMPACT_CARD_PADDING_VP)
        .borderRadius(DesignTokens.cardRadius)
        .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
        .margin({ bottom: COMPACT_CARD_SPACING_VP })

        // 活跃指标分组卡
        Column() {
          this.StatRow('活跃时长(秒)', `${stats.totalActiveSeconds}`, false)
          this.StatRow('写作会话数', `${stats.totalSessions}`, false)
          this.StatRow('写作天数', `${stats.daysCount}`, false)
        }
        .width('100%')
        .padding(COMPACT_CARD_PADDING_VP)
        .borderRadius(DesignTokens.cardRadius)
        .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
        .margin({ bottom: COMPACT_CARD_SPACING_VP })
      }
      .width('100%')
      .padding({
        left: this.adaptiveState.pagePadding,
        right: this.adaptiveState.pagePadding,
        top: this.adaptiveState.pagePadding,
        // Issue #932：列表末端留白统一走窄屏 token，避免贴住悬浮底栏。
        bottom: COMPACT_LIST_BOTTOM_INSET_VP
      })
    }
    .layoutWeight(1)
    .scrollBar(BarState.Off)
    .align(Alignment.Top)
  }

  /**
   * 宽屏统计：恢复改动前 f3154e32~1 的旧形态——指标行裸铺到底，无分组卡、无卡片底色与圆角，
   * 页面边距仍是旧版的 16 / 24，行几何走 WIDE_STAT_ROW_*；数值与窄屏共用同一份 WritingStats。
   */
  @Builder
  WideStatsContent(stats: WritingStats) {
    Scroll() {
      Column() {
        this.StatRow('统计范围', `${stats.range.startDate} ~ ${stats.range.endDate}`, true)
        this.StatRow('手写字数', `${stats.totalHumanTypedChars}`, true)
        this.StatRow('粘贴字数', `${stats.totalPastedChars}`, true)
        this.StatRow('删除字数', `${stats.totalDeletedChars}`, true)
        this.StatRow('AI插入字数', `${stats.totalAiInsertedChars}`, true)
        this.StatRow('净增字数', `${stats.totalNetDeltaChars}`, true)
        this.StatRow('活跃时长(秒)', `${stats.totalActiveSeconds}`, true)
        this.StatRow('写作会话数', `${stats.totalSessions}`, true)
        this.StatRow('写作天数', `${stats.daysCount}`, true)
      }
      .width('100%')
      // Issue #932 复核：宽屏沿用改动前的页面边距（16 / 24），不套窄屏的 pagePadding 与末端留白 token。
      .padding({ left: 16, right: 16, bottom: 24 })
    }
    .layoutWeight(1)
    .scrollBar(BarState.Off)
    .align(Alignment.Top)
  }

  build() {
    // Issue #784：API23+ 用 HdsAppDestinationApi23 共享根页面壳消费 HeaderTrailing；
    // API < 23 用 NavDestination .menus() 接入 HeaderTrailing 动作。
    if (this.useHdsNav) {
      HdsAppDestinationApi23({
        title: '统计',
        screenRole: ScreenRole.Stats,
        screenTabIndex: 2,
        showTitleBar: !this.adaptiveState.prefersWideLayout
      }) {
        this.StatsContent()
      }
    } else {
      NavDestination() {
        this.StatsContent()
      }
      .hideTitleBar(this.adaptiveState.prefersWideLayout)
      .hideBackButton(true)
      .onBackPressed(() => handleRootBack())
      .title('统计')
      .menus(buildNavMenuItems(this.headerActions))
      .onShown(() => {
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.Stats, 2)
      })
    }
  }
}
