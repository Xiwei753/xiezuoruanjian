// CompactStatsContent — 窄屏统计正文（Issue #945，按「手机统计七天 / 手机统计」手稿）。
//
// 从上到下：顶部时间范围切换（7 天 / 30 天 / 半年 / 生涯 / 自选）→ 大号总人工码字 +
// 日期区间 → 纪录 2×2（最多 / 最快 / 最长 / 连续）→ 码字日历热力图
// （#956，同一年每日真值 + 年份切换）。
// 内容可垂直滚动并给 HDS 悬浮底栏留空间。生涯与近七天只是同一页面换快照，
// 不另建导航栈。
//
// 本组件只画已解码的 Core Explorer 快照：不做统计口径计算、不补假数据；
// 区间切换 / 自选日期 / 重试 / 码字日历年份切换都通过回调交给 StatsScreen。

import { WritingStatsExplorer, WritingStatsHeatmap, WritingStatsRecord, WritingStatsSelectedPeriod, STATS_PRESET_LAST_7_DAYS, STATS_PRESET_LAST_30_DAYS, STATS_PRESET_LAST_6_MONTHS, STATS_PRESET_LIFETIME, STATS_PRESET_CUSTOM } from '../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import {
  COMPACT_CARD_PADDING_VP,
  COMPACT_CARD_SUPPORTING_FONT_VP,
  COMPACT_CARD_TITLE_FONT_VP,
  FLOATING_BAR_CONTENT_BOTTOM_INSET_VP
} from '../../../ui/theme/LayoutTokens'
import { EMPTY_STATS_EXPLORER, formatDateHuman, formatDateRangeHuman, formatDurationHuman, formatHistoryScope, formatInt, formatMeasuredDuration, formatRecordValue } from './StatsFormat'
import { WritingStatsCalendarHeatmap } from './WritingStatsCalendarHeatmap'
import { ContributionStatsSection } from './ContributionStatsSection'

interface RangeOption {
  label: string
  preset: string
}

const RANGE_OPTIONS: RangeOption[] = [
  { label: '7 天', preset: STATS_PRESET_LAST_7_DAYS },
  { label: '30 天', preset: STATS_PRESET_LAST_30_DAYS },
  { label: '半年', preset: STATS_PRESET_LAST_6_MONTHS },
  { label: '生涯', preset: STATS_PRESET_LIFETIME },
  { label: '自选', preset: STATS_PRESET_CUSTOM }
]

@Component
export struct CompactStatsContent {
  @Prop explorer: WritingStatsExplorer = EMPTY_STATS_EXPLORER
  @Prop selectedPeriod: string = STATS_PRESET_LAST_7_DAYS
  @Prop isLoading: boolean = false
  @Prop errorMessage: string = ''
  onSelectPeriod: (preset: string) => void = (_preset: string) => {}
  onPickCustomRange: () => void = () => {}
  onRetry: () => void = () => {}
  /** Issue #968：根容器实际挂载后回报，诊断不依赖预计分支。 */
  onMounted: () => void = () => {}
  // #956 码字日历：消费同一份每日快照 + 年份切换 / 日格选择回调，组件自己不查 Core。
  @Prop heatmap: WritingStatsHeatmap | null = null
  @Prop heatmapYear: number = new Date().getFullYear()
  @Prop heatmapError: string = ''
  onChangeHeatmapYear: (year: number) => void = (_year: number) => {}
  onSelectHeatmapDay: (date: string, chars: number) => void = (_date: string, _chars: number) => {}

  private selected(): WritingStatsSelectedPeriod {
    return this.explorer.selected
  }

  private recordValueText(record: WritingStatsRecord, unit: string): string {
    if (record.value === null) {
      return '—'
    }
    return formatRecordValue(record.value, unit)
  }

  private recordDurationText(record: WritingStatsRecord): string {
    if (record.value === null) {
      return '—'
    }
    return formatDurationHuman(record.value)
  }

  private streakDateText(record: WritingStatsRecord): string {
    if (record.date === null) {
      return ''
    }
    return formatDateRangeHuman(record.date, record.endDate)
  }

  private recordDateText(record: WritingStatsRecord): string {
    if (record.date === null) {
      return ''
    }
    return formatDateHuman(record.date)
  }

  build() {
    Scroll() {
      Column() {
        if (this.errorMessage.length > 0) {
          this.ErrorStrip()
        }
        this.RangeSwitch()
        this.SummarySection()
        this.RecordsSection()
        ContributionStatsSection({ stats: this.explorer.contributions })
        this.HeatmapSection()
      }
      .width('100%')
      .padding({
        left: 16,
        right: 16,
        top: 12,
        // 为 HDS 悬浮底栏预留共享内容避让区；系统安全区不在这里重复计算。
        bottom: FLOATING_BAR_CONTENT_BOTTOM_INSET_VP
      })
      .alignItems(HorizontalAlign.Start)
    }
    .layoutWeight(1)
    .width('100%')
    .scrollBar(BarState.Off)
    .align(Alignment.Top)
    .onAppear(() => {
      this.onMounted()
    })
  }

  /** 页内错误提示（自选日期非法 / Core 查询失败都在这里显示，附重试）。 */
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
    .margin({ bottom: 12 })
  }

  /** 顶部时间范围切换：选中范围突出显示，自选走系统日期选择。 */
  @Builder
  RangeSwitch() {
    // Flex wrap：极窄窗口下 5 个范围 chip 允许折行，不裁切边界。
    Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start }) {
      ForEach(RANGE_OPTIONS, (option: RangeOption) => {
        Text(option.label)
          .fontSize(14)
          .fontColor(this.selectedPeriod === option.preset
            ? $r('sys.color.font_on_primary')
            : $r('sys.color.font_primary'))
          .backgroundColor(this.selectedPeriod === option.preset
            ? $r('sys.color.ohos_id_color_emphasize')
            : $r('sys.color.comp_background_gray'))
          .borderRadius(DesignTokens.radiusPill)
          .padding({ left: 14, right: 14, top: 6, bottom: 6 })
          .margin({ right: 8, bottom: 8 })
          .onClick(() => {
            if (option.preset === STATS_PRESET_CUSTOM) {
              this.onPickCustomRange()
            } else {
              this.onSelectPeriod(option.preset)
            }
          })
      }, (option: RangeOption) => option.preset)
    }
    .width('100%')
    .margin({ bottom: 8 })
  }

  /** 大号总人工码字 + 日期区间 + 历史口径标注（本机 / 已同步设备 / 同步不完整）。 */
  @Builder
  SummarySection() {
    Column() {
      Row({ space: 8 }) {
        Text(formatInt(this.selected().totalHumanTypedChars))
          .fontSize(44)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
        Text('字')
          .fontSize(18)
          .fontColor($r('sys.color.font_secondary'))
        if (this.isLoading) {
          LoadingProgress()
            .width(18)
            .height(18)
            .margin({ left: 4 })
        }
      }
      .width('100%')
      .alignItems(VerticalAlign.Bottom)

      Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.SpaceBetween }) {
        Text(formatDateRangeHuman(this.selected().range.startDate, this.selected().range.endDate))
          .fontSize(14)
          .fontColor($r('sys.color.font_secondary'))
        Text(this.selected().hasMeasuredTiming
          ? `陪伴 ${formatInt(this.explorer.lifetime.elapsedCalendarDays)} 天 · 实际计时 ${formatMeasuredDuration(this.selected().measuredWritingSeconds)}`
          : `陪伴 ${formatInt(this.explorer.lifetime.elapsedCalendarDays)} 天 · 仅统计新记录`)
          .fontSize(14)
          .fontColor($r('sys.color.font_secondary'))
      }
      .width('100%')
      .margin({ top: 6 })

      Text(formatHistoryScope(this.explorer.historyScope))
        .fontSize(11)
        .fontColor($r('sys.color.font_tertiary'))
        .margin({ top: 4 })
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .margin({ bottom: COMPACT_CARD_PADDING_VP })
  }

  /** 纪录 2×2：最多码字 / 最快码字 / 最长码字 / 连续码字。 */
  @Builder
  RecordsSection() {
    Column() {
      Row() {
        this.RecordCell(
          '最多码字',
          this.recordValueText(this.selected().records.bestDay, '字'),
          this.recordDateText(this.selected().records.bestDay)
        )
        Column()
          .width(1)
          .backgroundColor($r('sys.color.comp_divider'))
          .alignSelf(ItemAlign.Stretch)
        this.RecordCell(
          '最快码字',
          this.recordValueText(this.selected().records.fastestHour, '字'),
          this.recordDateText(this.selected().records.fastestHour)
        )
      }
      .width('100%')
      .alignItems(VerticalAlign.Top)

      Divider().color($r('sys.color.comp_divider')).margin({ top: 10, bottom: 10 })

      Row() {
        this.RecordCell(
          '最长码字',
          this.recordDurationText(this.selected().records.longestSession),
          this.recordDateText(this.selected().records.longestSession)
        )
        Column()
          .width(1)
          .backgroundColor($r('sys.color.comp_divider'))
          .alignSelf(ItemAlign.Stretch)
        this.RecordCell(
          '连续码字',
          this.recordValueText(this.selected().records.longestStreak, '天'),
          this.streakDateText(this.selected().records.longestStreak)
        )
      }
      .width('100%')
      .alignItems(VerticalAlign.Top)
    }
    .width('100%')
    .padding(COMPACT_CARD_PADDING_VP)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
  }

  /** 每格：标题在上；日期在左、数值在右；连续码字的日期区间允许自然换行。 */
  @Builder
  RecordCell(label: string, valueText: string, dateText: string) {
    Column({ space: 6 }) {
      Text(label)
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.font_secondary'))
      Row() {
        if (dateText.length > 0) {
          Text(dateText)
            .fontSize(12)
            .fontColor($r('sys.color.font_tertiary'))
            .layoutWeight(1)
        } else {
          Blank()
            .layoutWeight(1)
        }
        Text(valueText)
          .fontSize(COMPACT_CARD_TITLE_FONT_VP)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .margin({ left: 8 })
          .flexShrink(0)
      }
      .width('100%')
      .alignItems(VerticalAlign.Top)
    }
    .layoutWeight(1)
    .alignItems(HorizontalAlign.Start)
    .padding({ left: 8, right: 8 })
  }

  /** #956 码字日历：每年的每日真值热力图；年份切换与日格选择复用回调。 */
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
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(COMPACT_CARD_PADDING_VP)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .margin({ top: COMPACT_CARD_PADDING_VP })
  }
}
