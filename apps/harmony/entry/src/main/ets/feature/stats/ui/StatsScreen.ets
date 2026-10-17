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
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import {
  COMPACT_CARD_PADDING_VP,
  COMPACT_CARD_SPACING_VP,
  COMPACT_CARD_SUPPORTING_FONT_VP,
  COMPACT_CARD_TITLE_FONT_VP,
  COMPACT_LIST_BOTTOM_INSET_VP
} from '../../../ui/theme/LayoutTokens'
import { CompactPageState, CompactPageStateKind } from '../../../ui/components/CompactPageState'

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

  // ─── Issue #932：窄屏统计分组卡 ───

  /** 指标行：左标签右数值；字号沿用窄屏卡片 token，只做视觉分组，不重算任何统计口径。 */
  @Builder
  StatRow(label: string, value: string) {
    Row() {
      Text(label)
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.font_secondary'))
        .layoutWeight(1)
      Text(value)
        .fontSize(COMPACT_CARD_TITLE_FONT_VP)
        .fontWeight(FontWeight.Medium)
    }
    .width('100%')
    .padding({ top: 8, bottom: 8 })
  }

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
        Scroll() {
          Column() {
            // Issue #753 评论 5814008597 问题2：WritingStats 已换成 Core 的 WritingStatsSummaryDto（最近 30 天汇总）。
            // 旧字段 totalChars/totalWords/totalChapters/todayChars/todayDuration/averageSpeed 全部不存在，
            // 改为展示新 DTO 的全部字段，并在首行展示统计范围。
            // Issue #932：统计指标不再裸露 Row 一直铺到底，改为分组卡呈现。
            Column() {
              this.StatRow('统计范围', `${this.stats.range.startDate} ~ ${this.stats.range.endDate}`)
            }
            .width('100%')
            .padding(COMPACT_CARD_PADDING_VP)
            .borderRadius(DesignTokens.cardRadius)
            .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
            .margin({ bottom: COMPACT_CARD_SPACING_VP })

            // 字数指标分组卡
            Column() {
              this.StatRow('手写字数', `${this.stats.totalHumanTypedChars}`)
              this.StatRow('粘贴字数', `${this.stats.totalPastedChars}`)
              this.StatRow('删除字数', `${this.stats.totalDeletedChars}`)
              this.StatRow('AI插入字数', `${this.stats.totalAiInsertedChars}`)
              this.StatRow('净增字数', `${this.stats.totalNetDeltaChars}`)
            }
            .width('100%')
            .padding(COMPACT_CARD_PADDING_VP)
            .borderRadius(DesignTokens.cardRadius)
            .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
            .margin({ bottom: COMPACT_CARD_SPACING_VP })

            // 活跃指标分组卡
            Column() {
              this.StatRow('活跃时长(秒)', `${this.stats.totalActiveSeconds}`)
              this.StatRow('写作会话数', `${this.stats.totalSessions}`)
              this.StatRow('写作天数', `${this.stats.daysCount}`)
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
