// WideStatsContent — 宽屏统计正文（Issue #945 / #968，按「统计页 / 近七天 / 统计页生涯」手稿）。
//
// - 顶部生涯横条：总字、起止日期、平均字/分、平均字/天、共计自然日天数 + 展开箭头；
// - 收起（expandedPanel = none）：下方四张时段卡横排（近七天 / 近三十天 / 近半年 / 自选区间）；
// - 点时段卡：卡区原地变成「左摘要 / 中纪录 / 右图表」三栏，点返回箭头还原；
// - 点生涯横条：横条原地变成「生涯头部 / 左纪录 / 右大图表」，四卡暂收起；
// - 四卡 / 展开区之后是常驻「素笺写作已陪伴你 N 天」，再往下才是码字日历；
// - 码字日历是独立的滚动模块，不与上面的生涯 / 四卡结构混排。
//
// 一层导航（左侧栏 / 顶部动作）由 PrimaryTabShell 提供，本组件不另画同步 / 设置，
// 也不新增页面内导航；数据、纪录与速度全部来自 Core Explorer 快照。

import { WritingStatsExplorer, WritingStatsHeatmap, WritingStatsPeriodCard, WritingStatsRecord, WritingStatsSelectedPeriod, STATS_PRESET_LAST_7_DAYS, STATS_PRESET_LAST_30_DAYS, STATS_PRESET_LAST_6_MONTHS, STATS_PRESET_LIFETIME, STATS_PRESET_CUSTOM } from '../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { COMPACT_CARD_PADDING_VP, COMPACT_CARD_SUPPORTING_FONT_VP, COMPACT_CARD_TITLE_FONT_VP } from '../../../ui/theme/LayoutTokens'
import { EMPTY_STATS_EXPLORER, STATS_PANEL_NONE, STATS_PANEL_RANGE, STATS_PANEL_LIFETIME, formatAverage, formatDateHuman, formatDateRangeHuman, formatDurationHuman, formatHistoryScope, formatInt, formatRecordValue } from './StatsFormat'
import { WritingStatsTrendChart } from './WritingStatsTrendChart'
import { WritingStatsCalendarHeatmap } from './WritingStatsCalendarHeatmap'

// 宽屏页面边距与卡片间距（vp）。
const WIDE_PAGE_PADDING_VP: number = 24
const WIDE_CONTENT_VERTICAL_PADDING_VP: number = 16
const WIDE_CARD_GAP_VP: number = 12
// 顶部生涯横条的最大宽度（vp）：手稿里它比四卡区窄、居中。
const WIDE_LIFETIME_BAR_MAX_WIDTH_VP: number = 720
// 近似高度只用于从整页高度里扣掉固定的页头 / 页脚，真实高度仍由内容撑开。
const WIDE_LIFETIME_BAR_RESERVED_HEIGHT_VP: number = 76
const WIDE_COMPANIONSHIP_RESERVED_HEIGHT_VP: number = 56
// 短窗口下四卡 / 展开区的最小可读高度；不足时整页纵向滚动，不压缩卡片与图表。
const WIDE_OVERVIEW_MIN_HEIGHT_VP: number = 250
const WIDE_OVERVIEW_FALLBACK_HEIGHT_VP: number = 380

@Component
export struct WideStatsContent {
  @Prop explorer: WritingStatsExplorer = EMPTY_STATS_EXPLORER
  @Prop selectedPeriod: string = STATS_PRESET_LAST_7_DAYS
  @Prop expandedPanel: string = STATS_PANEL_NONE
  @Prop isLoading: boolean = false
  @Prop errorMessage: string = ''
  /** 点时段卡（非自选）：切到该时段并展开三栏。 */
  onOpenPeriod: (preset: string) => void = (_preset: string) => {}
  /** 点自选区间卡：先走系统日期选择，确认后再展开。 */
  onPickCustomRange: () => void = () => {}
  /** 点生涯横条 / 箭头：展开或收起生涯面板。 */
  onToggleLifetime: () => void = () => {}
  /** 点展开面板的返回箭头：回到四卡总览。 */
  onCloseExpanded: () => void = () => {}
  onRetry: () => void = () => {}
  // #956 码字日历：消费同一份每日快照 + 年份切换 / 日格选择回调，组件自己不查 Core。
  @Prop heatmap: WritingStatsHeatmap | null = null
  @Prop heatmapYear: number = new Date().getFullYear()
  @Prop heatmapError: string = ''
  onChangeHeatmapYear: (year: number) => void = (_year: number) => {}
  onSelectHeatmapDay: (date: string, chars: number) => void = (_date: string, _chars: number) => {}

  // 页面实际可用高度：四卡 / 展开区据此占用除生涯条、陪伴条之外的剩余空间。
  @State private availableHeightVp: number = 0

  private selected(): WritingStatsSelectedPeriod {
    return this.explorer.selected
  }

  private presetLabel(preset: string): string {
    if (preset === STATS_PRESET_LAST_30_DAYS) {
      return '近三十天'
    }
    if (preset === STATS_PRESET_LAST_6_MONTHS) {
      return '近半年'
    }
    if (preset === STATS_PRESET_LIFETIME) {
      return '生涯'
    }
    if (preset === STATS_PRESET_CUSTOM) {
      return '自选区间'
    }
    return '近七天'
  }

  /** 预设卡的短日期段（MM/DD - MM/DD）；自选卡用完整日期（可能跨年）。 */
  private cardRangeText(card: WritingStatsPeriodCard): string {
    if (card.range === null) {
      return ''
    }
    if (card.preset === STATS_PRESET_CUSTOM) {
      return formatDateRangeHuman(card.range.startDate, card.range.endDate)
    }
    const startParts: string[] = card.range.startDate.split('-')
    const endParts: string[] = card.range.endDate.split('-')
    if (startParts.length === 3 && endParts.length === 3) {
      return `${startParts[1]}/${startParts[2]} - ${endParts[1]}/${endParts[2]}`
    }
    return formatDateRangeHuman(card.range.startDate, card.range.endDate)
  }

  private trendGranularityLabel(): string {
    const granularity: string = this.selected().trend.granularity
    if (granularity === 'week') {
      return '按周'
    }
    if (granularity === 'month') {
      return '按月'
    }
    return '按天'
  }

  /**
   * 四卡 / 展开区高度 = 页面高度 - 页边距 - 生涯条 - 陪伴条。
   * 短窗口仍保底 250vp，让 Scroll 接管纵向滚动，而不是把卡片压成一条。
   */
  private get overviewHeightVp(): number {
    if (this.availableHeightVp <= 0) {
      return WIDE_OVERVIEW_FALLBACK_HEIGHT_VP
    }
    const lifetimeReserve: number = this.expandedPanel === STATS_PANEL_LIFETIME
      ? 0
      : WIDE_LIFETIME_BAR_RESERVED_HEIGHT_VP
    const available: number = this.availableHeightVp
      - WIDE_CONTENT_VERTICAL_PADDING_VP * 2
      - lifetimeReserve
      - WIDE_COMPANIONSHIP_RESERVED_HEIGHT_VP
    return Math.max(WIDE_OVERVIEW_MIN_HEIGHT_VP, available)
  }

  build() {
    Column() {
      if (this.errorMessage.length > 0) {
        this.ErrorStrip()
      }
      // 生涯展开时，标题与摘要并进大面板，不再重复画一条横条。
      if (this.expandedPanel !== STATS_PANEL_LIFETIME) {
        this.LifetimeBar()
      }
      // 四卡 / 展开区 → 陪伴条 → 热力图。热力图保持独立模块，不插进四卡结构里。
      Scroll() {
        Column() {
          if (this.expandedPanel === STATS_PANEL_NONE) {
            this.PeriodCardsRow()
          } else {
            this.ExpandedPanel()
          }
          this.CompanionshipFooter()
          this.HeatmapSection()
        }
        .width('100%')
        .alignItems(HorizontalAlign.Center)
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Off)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .padding({
      left: WIDE_PAGE_PADDING_VP,
      right: WIDE_PAGE_PADDING_VP,
      top: WIDE_CONTENT_VERTICAL_PADDING_VP,
      bottom: WIDE_CONTENT_VERTICAL_PADDING_VP
    })
    .alignItems(HorizontalAlign.Center)
    .onAreaChange((_oldArea: Area, newArea: Area): void => {
      const nextHeight: number = Number(newArea.height)
      if (nextHeight > 0 && Math.abs(nextHeight - this.availableHeightVp) >= 0.5) {
        this.availableHeightVp = nextHeight
      }
    })
  }

  /** #968 码字日历：全年网格，宽度自适应；年份切换与日格选择仍走 Core 快照回调。 */
  @Builder
  HeatmapSection() {
    Column() {
      if (this.heatmap !== null) {
        WritingStatsCalendarHeatmap({
          heatmap: this.heatmap,
          onChangeYear: (year: number) => {
            this.onChangeHeatmapYear(year)
          },
          onSelectDay: (date: string, chars: number) => {
            this.onSelectHeatmapDay(date, chars)
          }
        })
      } else if (this.heatmapError.length > 0) {
        Text(this.heatmapError)
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .fontColor($r('sys.color.ohos_id_color_alert'))
          .width('100%')
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
      } else {
        Row({ space: 8 }) {
          LoadingProgress()
            .width(16)
            .height(16)
          Text(`${this.heatmapYear} 年 · 加载中...`)
            .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
            .fontColor($r('sys.color.font_tertiary'))
        }
        .width('100%')
        .justifyContent(FlexAlign.Center)
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(COMPACT_CARD_PADDING_VP)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .margin({ top: WIDE_CARD_GAP_VP, bottom: WIDE_CARD_GAP_VP })
  }

  /** 页内错误提示：自选日期非法 / Core 查询失败都在这里显示，附重试。 */
  @Builder
  ErrorStrip() {
    Row({ space: 8 }) {
      Text(this.errorMessage)
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.ohos_id_color_alert'))
        .layoutWeight(1)
        .maxLines(2)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
      Button($r('app.string.retry'))
        .fontSize(12)
        .height(28)
        .onClick(() => {
          this.onRetry()
        })
    }
    .width('100%')
    .padding({ left: 12, right: 12, top: 8, bottom: 8 })
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor($r('sys.color.comp_background_gray'))
    .margin({ bottom: WIDE_CARD_GAP_VP })
  }

  /** 顶部生涯横条：默认紧凑；点按展开生涯纪录 + 图表，再点收起。 */
  @Builder
  LifetimeBar() {
    Row() {
      Row({ space: 16 }) {
        Text('生涯')
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
        Text(`${formatInt(this.explorer.lifetime.totalHumanTypedChars)} 字`)
          .fontSize(24)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
        Column({ space: 2 }) {
          Text(formatDateRangeHuman(this.explorer.lifetime.firstWritingDate, this.explorer.lifetime.lastWritingDate))
            .fontSize(13)
            .fontColor($r('sys.color.font_secondary'))
          Text(`平均 ${formatAverage(this.explorer.lifetime.averageCharsPerMinute)} 字/分 · ${formatAverage(this.explorer.lifetime.averageCharsPerDay)} 字/天 · 共计 ${formatInt(this.explorer.lifetime.elapsedCalendarDays)} 天`)
            .fontSize(13)
            .fontColor($r('sys.color.font_secondary'))
        }
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Start)
        if (this.isLoading) {
          LoadingProgress()
            .width(18)
            .height(18)
        }
        SymbolGlyph(this.expandedPanel === STATS_PANEL_LIFETIME ? $r('sys.symbol.chevron_down') : $r('sys.symbol.chevron_up'))
          .fontSize(20)
          .fontColor([$r('sys.color.icon_secondary')])
      }
      .width('100%')
      .padding({ left: 20, right: 20, top: 14, bottom: 14 })
      .borderRadius(DesignTokens.cardRadius)
      .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
      .constraintSize({ maxWidth: WIDE_LIFETIME_BAR_MAX_WIDTH_VP })
      .onClick(() => {
        this.onToggleLifetime()
      })
    }
    .width('100%')
    .justifyContent(FlexAlign.Center)
    .margin({ bottom: WIDE_CARD_GAP_VP })
  }

  /** 折叠态：四张时段卡横排，按页面剩余高度撑满。 */
  @Builder
  PeriodCardsRow() {
    Row({ space: WIDE_CARD_GAP_VP }) {
      ForEach(this.explorer.periodCards, (card: WritingStatsPeriodCard) => {
        this.PeriodCard(card)
      }, (card: WritingStatsPeriodCard) => card.preset)
    }
    .width('100%')
    .height(this.overviewHeightVp)
    .alignItems(VerticalAlign.Top)
  }

  /** 时段卡：总量 / 字/分 / 字/天 / 日期段；自选未选时不给数字。 */
  @Builder
  PeriodCard(card: WritingStatsPeriodCard) {
    Column() {
      Text(this.presetLabel(card.preset))
        .fontSize(COMPACT_CARD_TITLE_FONT_VP)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))

      if (card.hasSelection && card.range !== null) {
        Blank()
        Text(`${formatInt(card.totalHumanTypedChars)} 字`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .margin({ top: 12 })

        Row() {
          Text(`${formatAverage(card.averageCharsPerMinute)} 字/分`)
            .fontSize(15)
            .fontColor($r('sys.color.font_secondary'))
            .layoutWeight(1)
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          SymbolGlyph($r('sys.symbol.chevron_up'))
            .fontSize(16)
            .fontColor([$r('sys.color.icon_secondary')])
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)
        .margin({ top: 18 })

        Text(`${formatAverage(card.averageCharsPerDay)} 字/天`)
          .fontSize(15)
          .fontColor($r('sys.color.font_secondary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .margin({ top: 12 })

        Blank()
        Text(this.cardRangeText(card))
          .fontSize(13)
          .fontColor($r('sys.color.font_tertiary'))
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
      } else {
        Blank()
        Text('未选择')
          .fontSize(20)
          .fontColor($r('sys.color.font_secondary'))
        Text('点按选择日期区间')
          .fontSize(13)
          .fontColor($r('sys.color.font_tertiary'))
          .margin({ top: 8 })
        Blank()
      }
    }
    .layoutWeight(1)
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(COMPACT_CARD_PADDING_VP)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .onClick(() => {
      if (card.preset === STATS_PRESET_CUSTOM) {
        this.onPickCustomRange()
      } else {
        this.onOpenPeriod(card.preset)
      }
    })
  }

  /** 展开态：时段卡 → 左侧摘要 / 中纪录 / 右大图；生涯 → 头部 / 左纪录 / 右大图。 */
  @Builder
  ExpandedPanel() {
    if (this.expandedPanel === STATS_PANEL_LIFETIME) {
      this.LifetimeExpandedPanel()
    } else {
      this.RangeExpandedPanel()
    }
  }

  /** 生涯展开：横条原地变成一张大卡，标题在上，纪录在左下，图表占右侧。 */
  @Builder
  LifetimeExpandedPanel() {
    Column() {
      Row({ space: 16 }) {
        Text('生涯')
          .fontSize(24)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
        Text(`${formatInt(this.explorer.lifetime.totalHumanTypedChars)} 字`)
          .fontSize(30)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
        Column({ space: 3 }) {
          Text(formatDateRangeHuman(this.explorer.lifetime.firstWritingDate, this.explorer.lifetime.lastWritingDate))
            .fontSize(13)
            .fontColor($r('sys.color.font_secondary'))
          Text(`平均 ${formatAverage(this.explorer.lifetime.averageCharsPerMinute)} 字/分 · ${formatAverage(this.explorer.lifetime.averageCharsPerDay)} 字/天 · 共计 ${formatInt(this.explorer.lifetime.elapsedCalendarDays)} 天`)
            .fontSize(13)
            .fontColor($r('sys.color.font_secondary'))
        }
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Start)
        if (this.isLoading) {
          LoadingProgress()
            .width(18)
            .height(18)
        }
        SymbolGlyph($r('sys.symbol.chevron_down'))
          .fontSize(20)
          .fontColor([$r('sys.color.icon_secondary')])
          .onClick(() => {
            this.onCloseExpanded()
          })
      }
      .width('100%')
      .padding({ left: 24, right: 24, top: 20, bottom: 18 })
      .alignItems(VerticalAlign.Center)

      Divider().color($r('sys.color.comp_divider'))

      Row({ space: WIDE_CARD_GAP_VP }) {
        this.RecordsBody(4)
        this.ChartBody(6)
      }
      .width('100%')
      .layoutWeight(1)
      .padding({ left: 16, right: 16, top: 12, bottom: 16 })
      .alignItems(VerticalAlign.Top)
    }
    .width('100%')
    .height(this.overviewHeightVp)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .clip(true)
  }

  /** 时段展开：一张大卡内部分成左摘要 / 中纪录 / 右大图，不再摆三张独立小卡。 */
  @Builder
  RangeExpandedPanel() {
    Row() {
      this.SelectedSummaryColumn(3)
      Divider()
        .vertical(true)
        .height('100%')
        .color($r('sys.color.comp_divider'))
      this.RecordsBody(4)
      this.ChartBody(5)
    }
    .width('100%')
    .height(this.overviewHeightVp)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .clip(true)
  }

  /** 左摘要：已选时段的总量与均速 + 返回四卡的箭头。 */
  @Builder
  SelectedSummaryColumn(weight: number) {
    Column() {
      Row() {
        Text(this.presetLabel(this.selected().preset))
          .fontSize(COMPACT_CARD_TITLE_FONT_VP)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .layoutWeight(1)
        SymbolGlyph($r('sys.symbol.chevron_down'))
          .fontSize(18)
          .fontColor([$r('sys.color.icon_secondary')])
          .onClick(() => {
            this.onCloseExpanded()
          })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      Blank()
      Text(`${formatInt(this.selected().totalHumanTypedChars)} 字`)
        .fontSize(30)
        .fontWeight(FontWeight.Bold)
        .fontColor($r('sys.color.font_primary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .margin({ top: 12 })

      Text(`${formatAverage(this.selected().averageCharsPerMinute)} 字/分 · ${formatAverage(this.selected().averageCharsPerDay)} 字/天`)
        .fontSize(14)
        .fontColor($r('sys.color.font_secondary'))
        .margin({ top: 10 })

      Text(`写作 ${formatInt(this.selected().writingDays)} 天`)
        .fontSize(14)
        .fontColor($r('sys.color.font_secondary'))
        .margin({ top: 6 })

      Blank()
      Text(formatDateRangeHuman(this.selected().range.startDate, this.selected().range.endDate))
        .fontSize(12)
        .fontColor($r('sys.color.font_tertiary'))
        .maxLines(2)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
    }
    .layoutWeight(weight)
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: 20, right: 20, top: 20, bottom: 16 })
  }

  /** 纪录正文：最多 / 最快 / 最长 / 连续，单位分别为字、字/时、时分、天。 */
  @Builder
  RecordsBody(weight: number) {
    Column() {
      Text(formatHistoryScope(this.explorer.historyScope))
        .fontSize(12)
        .fontColor($r('sys.color.font_tertiary'))
        .margin({ bottom: 10 })

      this.RecordRow('最多码字', this.recordDateText(this.selected().records.bestDay),
        this.recordValueText(this.selected().records.bestDay, '字'))
      this.RecordRow('最快码字', this.recordDateText(this.selected().records.fastestHour),
        this.recordValueText(this.selected().records.fastestHour, '字/时'))
      this.RecordRow('最长码字', this.recordDateText(this.selected().records.longestSession),
        this.recordDurationText(this.selected().records.longestSession))
      this.RecordRow('连续码字', this.streakDateText(this.selected().records.longestStreak),
        this.recordValueText(this.selected().records.longestStreak, '天'))

      Blank()
    }
    .layoutWeight(weight)
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: 20, right: 20, top: 20, bottom: 16 })
  }

  @Builder
  RecordRow(label: string, dateText: string, valueText: string) {
    Row() {
      Text(label)
        .fontSize(15)
        .fontColor($r('sys.color.font_primary'))
        .layoutWeight(3)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
      if (dateText.length > 0) {
        Text(dateText)
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
          .layoutWeight(3)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .textAlign(TextAlign.End)
          .margin({ left: 8 })
      }
      Text(valueText)
        .fontSize(15)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))
        .layoutWeight(2)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .textAlign(TextAlign.End)
        .margin({ left: 8 })
    }
    .width('100%')
    .padding({ top: 10, bottom: 10 })
    .alignItems(VerticalAlign.Center)
  }

  /** 图表正文：宽屏数据区按剩余空间弹性伸缩。 */
  @Builder
  ChartBody(weight: number) {
    Column() {
      Column() {
        Row() {
          Text('写作趋势')
            .fontSize(COMPACT_CARD_TITLE_FONT_VP)
            .fontWeight(FontWeight.Medium)
            .fontColor($r('sys.color.font_primary'))
            .layoutWeight(1)
          Text(this.trendGranularityLabel())
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))
        }
        .width('100%')
        .margin({ bottom: 8 })

        WritingStatsTrendChart({ trend: this.selected().trend })
          .width('100%')
          .layoutWeight(1)
      }
      .width('100%')
      .height('100%')
      .padding(16)
      .borderRadius(DesignTokens.radiusLg)
      .border({ width: 1, color: $r('sys.color.comp_divider') })
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .layoutWeight(weight)
    .height('100%')
    .padding({ left: 12, right: 16, top: 16, bottom: 16 })
  }

  /** 底部常驻：自然日历天数（含首次写作日到今天两端），不是码字天数。 */
  @Builder
  CompanionshipFooter() {
    Row() {
      Text(`素笺写作已陪伴你 ${formatInt(this.explorer.lifetime.elapsedCalendarDays)} 天`)
        .fontSize(15)
        .fontColor($r('sys.color.font_secondary'))
    }
    .width('100%')
    .padding({ left: 20, right: 20, top: 14, bottom: 14 })
    .margin({ top: WIDE_CARD_GAP_VP })
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
  }

  private recordValueText(record: WritingStatsRecord, unit: string): string {
    if (record.value === null) {
      return '暂无纪录'
    }
    return formatRecordValue(record.value, unit)
  }

  private recordDurationText(record: WritingStatsRecord): string {
    if (record.value === null) {
      return '暂无纪录'
    }
    return formatDurationHuman(record.value)
  }

  private recordDateText(record: WritingStatsRecord): string {
    if (record.date === null) {
      return ''
    }
    return formatDateHuman(record.date)
  }

  private streakDateText(record: WritingStatsRecord): string {
    if (record.date === null) {
      return ''
    }
    return formatDateRangeHuman(record.date, record.endDate)
  }
}
