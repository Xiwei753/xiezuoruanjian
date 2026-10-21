// WideLifetimeSummary — 宽屏统计顶部生涯概览条（Issue #972）。
//
// 收起：居中横条，左侧「生涯」+ 总人写字数，中间首末写作日期与平均速度 / 日均字数，
// 右侧写作天数 / 历时天数与展开提示。
// 展开：横条原位置向下变成一张大卡，上半部仍是同一份生涯摘要，下半部左侧四项生涯纪录、
// 右侧大趋势折线图；不弹窗、不导航，四张时段卡由父页面隐藏。
//
// 数据全部来自 Core Explorer 的 lifetime / selected 快照，组件只做格式化。

import {
  WritingStatsExplorer,
  WritingStatsRecord,
  WritingStatsSelectedPeriod,
  STATS_PRESET_LIFETIME
} from '../../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import {
  COMPACT_CARD_SUPPORTING_FONT_VP,
  COMPACT_CARD_TITLE_FONT_VP
} from '../../../../ui/theme/LayoutTokens'
import {
  EMPTY_STATS_EXPLORER,
  formatAverage,
  formatDateHuman,
  formatDateRangeHuman,
  formatDurationHuman,
  formatHistoryScope,
  formatInt,
  formatRecordValue
} from '../StatsFormat'
import { WritingStatsTrendChart } from '../WritingStatsTrendChart'

// 收起态横条占统计内容宽度的比例（参考约 657 / 973vp）；展开后占满整行。
const LIFETIME_COLLAPSED_WIDTH: string = '67.5%'
// 展开态整框宽高比（参考约 973 x 458vp）；内部高度由宽度派生，不写死平板高度。
const LIFETIME_EXPANDED_ASPECT: number = 2.12

@Component
export struct WideLifetimeSummary {
  @Prop explorer: WritingStatsExplorer = EMPTY_STATS_EXPLORER
  @Prop expanded: boolean = false
  @Prop isLoading: boolean = false
  onToggle: () => void = () => {}
  onRetry: () => void = () => {}

  private selected(): WritingStatsSelectedPeriod {
    return this.explorer.selected
  }

  /** 展开区只能消费真正的生涯快照；切时段过程中不把旧时段数据标成生涯。 */
  private lifetimeSelected(): boolean {
    return this.explorer.selected.preset === STATS_PRESET_LIFETIME
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
    return formatDateHuman(record.date)
  }

  private streakDateText(record: WritingStatsRecord): string {
    if (record.date === null) {
      return ''
    }
    return formatDateRangeHuman(record.date, record.endDate)
  }

  build() {
    if (this.expanded) {
      this.ExpandedFrame()
    } else {
      this.CollapsedFrame()
    }
  }

  /** 收起态：同一份摘要条，宽度约内容宽的 67.5%，高度由文字与内边距自然撑开。 */
  @Builder
  CollapsedFrame() {
    Column() {
      this.SummaryHeader()
    }
    .width(LIFETIME_COLLAPSED_WIDTH)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .clip(true)
  }

  /** 展开态：整内容宽，宽高比约 2.12；上下分栏占满剩余高度。 */
  @Builder
  ExpandedFrame() {
    Column() {
      this.SummaryHeader()

      Divider()
        .color($r('sys.color.comp_divider'))

      if (this.lifetimeSelected()) {
        this.LifetimeDetail()
      } else if (this.isLoading) {
        this.DetailPlaceholder('正在加载生涯记录...', false)
      } else {
        this.DetailPlaceholder('生涯记录暂不可用', true)
      }
    }
    .width('100%')
    .aspectRatio(LIFETIME_EXPANDED_ASPECT)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .clip(true)
  }

  /** 概览摘要：始终显示 lifetime 真值；点整条切换展开 / 收起。 */
  @Builder
  SummaryHeader() {
    Row({ space: 20 }) {
      Column({ space: 2 }) {
        Text('生涯')
          .fontSize(22)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
        Row({ space: 5 }) {
          Text(formatInt(this.explorer.lifetime.totalHumanTypedChars))
            .fontSize(34)
            .fontWeight(FontWeight.Bold)
            .fontColor($r('sys.color.font_primary'))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          Text('字')
            .fontSize(15)
            .fontColor($r('sys.color.font_secondary'))
            .margin({ bottom: 5 })
        }
        .alignItems(VerticalAlign.Bottom)
      }
      .alignItems(HorizontalAlign.Start)

      Column({ space: 4 }) {
        Text(formatDateRangeHuman(
          this.explorer.lifetime.firstWritingDate,
          this.explorer.lifetime.lastWritingDate))
          .fontSize(14)
          .fontColor($r('sys.color.font_secondary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
        Text(`平均 ${formatAverage(this.explorer.lifetime.averageCharsPerMinute)} 字/分 · 日均 ${formatAverage(this.explorer.lifetime.averageCharsPerDay)} 字/天`)
          .fontSize(14)
          .fontColor($r('sys.color.font_secondary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
        Text(`写作 ${formatInt(this.explorer.lifetime.writingDays)} 天 · 历时 ${formatInt(this.explorer.lifetime.elapsedCalendarDays)} 天`)
          .fontSize(14)
          .fontColor($r('sys.color.font_tertiary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)

      if (this.isLoading) {
        LoadingProgress()
          .width(18)
          .height(18)
      }
      SymbolGlyph(this.expanded ? $r('sys.symbol.chevron_up') : $r('sys.symbol.chevron_down'))
        .fontSize(20)
        .fontColor([$r('sys.color.icon_secondary')])
    }
    .width('100%')
    .padding({ left: 24, right: 24, top: 18, bottom: 18 })
    .alignItems(VerticalAlign.Center)
    .onClick(() => {
      this.onToggle()
    })
  }

  /** 展开态：左纪录 / 右大图，同一张外框内两列。 */
  @Builder
  LifetimeDetail() {
    Row({ space: 24 }) {
      Column() {
        Row() {
          Text('生涯纪录')
            .fontSize(COMPACT_CARD_TITLE_FONT_VP)
            .fontWeight(FontWeight.Medium)
            .fontColor($r('sys.color.font_primary'))
            .layoutWeight(1)
          Text(formatHistoryScope(this.explorer.historyScope))
            .fontSize(12)
            .fontColor($r('sys.color.font_tertiary'))
        }
        .width('100%')
        .margin({ bottom: 8 })

        this.RecordRow(
          '最多码字',
          this.recordDateText(this.selected().records.bestDay),
          this.recordValueText(this.selected().records.bestDay, '字'))
        this.RecordRow(
          '最快码字',
          this.recordDateText(this.selected().records.fastestHour),
          this.recordValueText(this.selected().records.fastestHour, '字/时'))
        this.RecordRow(
          '最长码字',
          this.recordDateText(this.selected().records.longestSession),
          this.recordDurationText(this.selected().records.longestSession))
        this.RecordRow(
          '连续码字',
          this.streakDateText(this.selected().records.longestStreak),
          this.recordValueText(this.selected().records.longestStreak, '天'))
      }
      .layoutWeight(1)
      .height('100%')
      .alignItems(HorizontalAlign.Start)
      .padding({ left: 24, right: 4, top: 16, bottom: 18 })

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
      .layoutWeight(1)
      .height('100%')
      .padding(16)
      .borderRadius(DesignTokens.radiusLg)
      .border({ width: 1, color: $r('sys.color.comp_divider') })
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ right: '2.4%', top: '1.6%', bottom: '2%' })
    .alignItems(VerticalAlign.Top)
  }

  @Builder
  RecordRow(label: string, dateText: string, valueText: string) {
    Row() {
      Text(label)
        .fontSize(15)
        .fontColor($r('sys.color.font_primary'))
        .layoutWeight(2)
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
    .padding({ top: 9, bottom: 9 })
    .alignItems(VerticalAlign.Center)
  }

  /** 生涯快照未到 / 查询失败：明确占位，失败时提供重试入口。 */
  @Builder
  DetailPlaceholder(message: string, showRetry: boolean) {
    Column({ space: 10 }) {
      if (showRetry) {
        Text(message)
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .fontColor($r('sys.color.font_secondary'))
        Button($r('app.string.retry'))
          .fontSize(12)
          .onClick(() => {
            this.onRetry()
          })
      } else {
        LoadingProgress()
          .width(22)
          .height(22)
        Text(message)
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .fontColor($r('sys.color.font_tertiary'))
      }
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
    .alignItems(HorizontalAlign.Center)
  }
}
