// WideStatsContent — 宽屏统计正文（Issue #945，按「统计页 / 近七天 / 统计页生涯」手稿）。
//
// - 顶部生涯横条：总字、起止日期、平均字/分、平均字/天、共计自然日天数 + 展开箭头；
// - 收起（expandedPanel = none）：下方四张时段卡横排（近七天 / 近三十天 / 近半年 / 自选区间）；
// - 点时段卡：卡区原地变成「左摘要 / 中纪录 / 右图表」三栏，点返回箭头还原；
// - 点生涯横条：原地变成「左生涯纪录 / 右图表」，四卡暂收起；
// - 底部常驻「素笺写作已陪伴你 N 天」（N = Core elapsedCalendarDays，不是码字天数）。
//
// 一层导航（左侧栏 / 顶部动作）由 PrimaryTabShell 提供，本组件不另画同步 / 设置，
// 也不新增页面内导航；数据、纪录与速度全部来自 Core Explorer 快照。

import { WritingStatsExplorer, WritingStatsPeriodCard, WritingStatsRecord, WritingStatsSelectedPeriod, STATS_PRESET_LAST_7_DAYS, STATS_PRESET_LAST_30_DAYS, STATS_PRESET_LAST_6_MONTHS, STATS_PRESET_LIFETIME, STATS_PRESET_CUSTOM } from '../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { COMPACT_CARD_PADDING_VP, COMPACT_CARD_SUPPORTING_FONT_VP, COMPACT_CARD_TITLE_FONT_VP } from '../../../ui/theme/LayoutTokens'
import { EMPTY_STATS_EXPLORER, STATS_PANEL_NONE, STATS_PANEL_RANGE, STATS_PANEL_LIFETIME, formatAverage, formatDateHuman, formatDateRangeHuman, formatDurationHuman, formatInt, formatRecordValue } from './StatsFormat'
import { WritingStatsTrendChart } from './WritingStatsTrendChart'

// 宽屏页面边距与卡片间距（vp）。
const WIDE_PAGE_PADDING_VP: number = 24
const WIDE_CARD_GAP_VP: number = 12
// 顶部生涯横条的最大宽度（vp）：手稿里它比四卡区窄、居中。
const WIDE_LIFETIME_BAR_MAX_WIDTH_VP: number = 900

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

  build() {
    Column() {
      if (this.errorMessage.length > 0) {
        this.ErrorStrip()
      }
      this.LifetimeBar()
      if (this.expandedPanel === STATS_PANEL_NONE) {
        this.PeriodCardsRow()
      } else {
        this.ExpandedPanel()
      }
      this.CompanionshipFooter()
    }
    .width('100%')
    .height('100%')
    .padding({
      left: WIDE_PAGE_PADDING_VP,
      right: WIDE_PAGE_PADDING_VP,
      top: 16,
      bottom: 16
    })
    .alignItems(HorizontalAlign.Center)
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
          .fontSize(22)
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

  /** 折叠态：四张时段卡横排。 */
  @Builder
  PeriodCardsRow() {
    Row({ space: WIDE_CARD_GAP_VP }) {
      ForEach(this.explorer.periodCards, (card: WritingStatsPeriodCard) => {
        this.PeriodCard(card)
      }, (card: WritingStatsPeriodCard) => card.preset)
    }
    .width('100%')
    .layoutWeight(1)
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
        Text(`${formatInt(card.totalHumanTypedChars)} 字`)
          .fontSize(26)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .margin({ top: 10 })

        Row() {
          Text(`${formatAverage(card.averageCharsPerMinute)} 字/分`)
            .fontSize(14)
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
        .margin({ top: 6 })

        Text(`${formatAverage(card.averageCharsPerDay)} 字/天`)
          .fontSize(14)
          .fontColor($r('sys.color.font_secondary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .margin({ top: 4 })

        Text(this.cardRangeText(card))
          .fontSize(12)
          .fontColor($r('sys.color.font_tertiary'))
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .margin({ top: 6 })
      } else {
        Text('未选择')
          .fontSize(16)
          .fontColor($r('sys.color.font_secondary'))
          .margin({ top: 10 })
        Text('点按选择日期区间')
          .fontSize(12)
          .fontColor($r('sys.color.font_tertiary'))
          .margin({ top: 4 })
      }

      Blank()
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

  /** 展开态：时段卡 → 左摘要 / 中纪录 / 右图表；生涯 → 左生涯纪录 / 右图表。 */
  @Builder
  ExpandedPanel() {
    Row({ space: WIDE_CARD_GAP_VP }) {
      if (this.expandedPanel === STATS_PANEL_LIFETIME) {
        this.RecordsColumn(4)
        this.ChartColumn(6)
      } else {
        this.SelectedSummaryColumn(3)
        this.RecordsColumn(4)
        this.ChartColumn(6)
      }
    }
    .width('100%')
    .layoutWeight(1)
    .alignItems(VerticalAlign.Top)
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

      Text(`${formatInt(this.selected().totalHumanTypedChars)} 字`)
        .fontSize(26)
        .fontWeight(FontWeight.Bold)
        .fontColor($r('sys.color.font_primary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .margin({ top: 10 })

      Text(`${formatAverage(this.selected().averageCharsPerMinute)} 字/分 · ${formatAverage(this.selected().averageCharsPerDay)} 字/天`)
        .fontSize(13)
        .fontColor($r('sys.color.font_secondary'))
        .margin({ top: 6 })

      Text(`写作 ${formatInt(this.selected().writingDays)} 天`)
        .fontSize(13)
        .fontColor($r('sys.color.font_secondary'))
        .margin({ top: 4 })

      Text(formatDateRangeHuman(this.selected().range.startDate, this.selected().range.endDate))
        .fontSize(12)
        .fontColor($r('sys.color.font_tertiary'))
        .maxLines(2)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .margin({ top: 6 })

      Blank()
    }
    .layoutWeight(weight)
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(COMPACT_CARD_PADDING_VP)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
  }

  /** 纪录列：最多 / 最快 / 最长 / 连续，单位分别为字、字/时、时分、天。 */
  @Builder
  RecordsColumn(weight: number) {
    Column() {
      Text('本机记录')
        .fontSize(12)
        .fontColor($r('sys.color.font_tertiary'))
        .margin({ bottom: 8 })

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
    .padding(COMPACT_CARD_PADDING_VP)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
  }

  @Builder
  RecordRow(label: string, dateText: string, valueText: string) {
    Row() {
      Text(label)
        .fontSize(14)
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
        .fontSize(14)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))
        .layoutWeight(2)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .textAlign(TextAlign.End)
        .margin({ left: 8 })
    }
    .width('100%')
    .padding({ top: 8, bottom: 8 })
    .alignItems(VerticalAlign.Center)
  }

  /** 图表列：宽屏数据区按剩余空间弹性伸缩（layoutWeight）。 */
  @Builder
  ChartColumn(weight: number) {
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
    .layoutWeight(weight)
    .height('100%')
    .padding(COMPACT_CARD_PADDING_VP)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
  }

  /** 底部常驻：自然日历天数（含首次写作日到今天两端），不是码字天数。 */
  @Builder
  CompanionshipFooter() {
    Row() {
      Text(`素笺写作已陪伴你 ${formatInt(this.explorer.lifetime.elapsedCalendarDays)} 天`)
        .fontSize(14)
        .fontColor($r('sys.color.font_secondary'))
    }
    .width('100%')
    .padding({ left: 16, right: 16, top: 12, bottom: 12 })
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
