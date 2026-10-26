// WidePeriodExpanded — 宽屏统计的时段展开态（Issue #972）。
//
// 点四张时段卡之一后，原卡区原地换成同一个大外框，内部三列：
// 左侧所选时段摘要（右上箭头收起）、中间四项纪录、右侧大趋势折线图。
// 不导航新路由、不拆成三张独立小卡；生涯展开态由 WideLifetimeSummary 单独承载。
//
// 数据来自 Explorer.selected 快照；组件不重新计算纪录或速度。

import {
  WritingStatsSelectedPeriod,
  WritingStatsRecord,
  WritingStatsHeatmapDay,
  STATS_PRESET_LAST_7_DAYS
} from '../../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import {
  COMPACT_CARD_SUPPORTING_FONT_VP,
  COMPACT_CARD_TITLE_FONT_VP
} from '../../../../ui/theme/LayoutTokens'
import {
  EMPTY_STATS_EXPLORER,
  formatAverage,
  formatAveragePerHour,
  formatDateRangeHuman,
  formatDurationHuman,
  formatHistoryScope,
  formatInt,
  formatMeasuredDuration,
  formatRecordValue
} from '../StatsFormat'
import { widePresetLabel } from './WidePeriodCards'
import { WidePeriodHeatmap } from './WidePeriodHeatmap'

// 展开大框宽高比（参考约 973 x 351vp）；高度由内容宽度派生，短窗口整体交给页面滚动。
const WIDE_EXPANDED_ASPECT: number = 2.78
// 参考图三列宽度约 218:373:382vp，换算为 2.2 : 3.8 : 4.0。
const WIDE_SUMMARY_WEIGHT: number = 2.2
const WIDE_RECORDS_WEIGHT: number = 3.8
const WIDE_HEATMAP_WEIGHT: number = 4.0

@Component
export struct WidePeriodExpanded {
  @Prop selectedPeriod: string = STATS_PRESET_LAST_7_DAYS
  @Prop selected: WritingStatsSelectedPeriod = EMPTY_STATS_EXPLORER.selected
  @Prop historyScope: string = 'localEvents'
  @Prop isLoading: boolean = false
  @Prop periodHeatmapDays: WritingStatsHeatmapDay[] = []
  @Prop periodHeatmapLoading: boolean = false
  @Prop periodHeatmapError: string = ''
  onClose: () => void = () => {}
  onRetry: () => void = () => {}
  onRetryPeriodHeatmap: () => void = () => {}

  /** 快照是否已经对应刚点开的时段；否则不能把旧时段数字贴上新标题。 */
  private hasSnapshot(): boolean {
    return this.selected.preset === this.selectedPeriod
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

  build() {
    Column() {
      if (this.hasSnapshot()) {
        this.ExpandedDetail()
      } else {
        this.ExpandedPlaceholder()
      }
    }
    .width('100%')
    .aspectRatio(WIDE_EXPANDED_ASPECT)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .clip(true)
  }

  @Builder
  ExpandedDetail() {
    Row() {
      this.SummaryColumn()
      this.RecordsColumn()
      this.PeriodHeatmapColumn()
    }
    .width('100%')
    .layoutWeight(1)
    .alignItems(VerticalAlign.Top)
  }

  /** 左列：所选时段总量 / 速度 / 日均 / 写作天数 / 日期段，右上箭头收起。 */
  @Builder
  SummaryColumn() {
    Column() {
      Row() {
        Text(widePresetLabel(this.selected.preset))
          .fontSize(COMPACT_CARD_TITLE_FONT_VP)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .layoutWeight(1)
        if (this.isLoading) {
          LoadingProgress()
            .width(16)
            .height(16)
            .margin({ right: 8 })
        }
        SymbolGlyph($r('sys.symbol.chevron_down'))
          .fontSize(20)
          .fontColor([$r('sys.color.icon_secondary')])
          .onClick(() => {
            this.onClose()
          })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      Blank()

      Text(`${formatInt(this.selected.totalHumanTypedChars)} 字`)
        .fontSize(32)
        .fontWeight(FontWeight.Bold)
        .fontColor($r('sys.color.font_primary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })

      Text(`${formatAveragePerHour(this.selected.averageCharsPerHour)} 字/时`)
        .fontSize(16)
        .fontColor($r('sys.color.font_secondary'))
        .margin({ top: 12 })

      Text(`${formatAverage(this.selected.averageCharsPerDay)} 字/天`)
        .fontSize(16)
        .fontColor($r('sys.color.font_secondary'))
        .margin({ top: 8 })

      Text(this.selected.hasMeasuredTiming
        ? `写作 ${formatInt(this.selected.writingDays)} 天 · 实际计时 ${formatMeasuredDuration(this.selected.measuredWritingSeconds)}`
        : `写作 ${formatInt(this.selected.writingDays)} 天 · 仅统计新记录`)
        .fontSize(14)
        .fontColor($r('sys.color.font_tertiary'))
        .margin({ top: 8 })

      Blank()

      Text(formatDateRangeHuman(this.selected.range.startDate, this.selected.range.endDate))
        .fontSize(13)
        .fontColor($r('sys.color.font_secondary'))
        .maxLines(2)
        .textOverflow({ overflow: TextOverflow.Ellipsis })

      Text(formatHistoryScope(this.historyScope))
        .fontSize(11)
        .fontColor($r('sys.color.font_tertiary'))
        .margin({ top: 4 })
    }
    .layoutWeight(WIDE_SUMMARY_WEIGHT)
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: '2.4%', right: '1.6%', top: '2%', bottom: '1.8%' })
  }

  /** 中列：最多 / 最快 / 最长 / 连续四项纪录，和原图一样只排标签与值。 */
  @Builder
  RecordsColumn() {
    Column() {
      Text('本时段纪录')
        .fontSize(COMPACT_CARD_TITLE_FONT_VP)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))
        .margin({ bottom: 10 })

      this.RecordRow('最多码字', this.recordValueText(this.selected.records.bestDay, '字'))
      this.RecordRow('单小时最多码字', this.recordValueText(this.selected.records.fastestHour, '字'))
      this.RecordRow('最长码字', this.recordDurationText(this.selected.records.longestSession))
      this.RecordRow('连续码字', this.recordValueText(this.selected.records.longestStreak, '天'))

      Blank()
    }
    .layoutWeight(WIDE_RECORDS_WEIGHT)
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: '1.6%', right: '1.6%', top: '2%', bottom: '1.8%' })
  }

  @Builder
  RecordRow(label: string, valueText: string) {
    Row() {
      Text(label)
        .fontSize(15)
        .fontColor($r('sys.color.font_primary'))
        .layoutWeight(1)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
      Text(valueText)
        .fontSize(16)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))
        .layoutWeight(1)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .textAlign(TextAlign.End)
        .margin({ left: 8 })
    }
    .width('100%')
    .padding({ top: 12, bottom: 12 })
    .alignItems(VerticalAlign.Center)
  }

  /** 右列：时段实际日数据热力图，与下方全年热力图使用相同 Core 日真值。 */
  @Builder
  PeriodHeatmapColumn() {
    Column() {
      WidePeriodHeatmap({
        startDate: this.selected.range.startDate,
        endDate: this.selected.range.endDate,
        days: this.periodHeatmapDays,
        isLoading: this.periodHeatmapLoading,
        errorMessage: this.periodHeatmapError,
        onRetry: (): void => {
          this.onRetryPeriodHeatmap()
        }
      })
    }
    .layoutWeight(WIDE_HEATMAP_WEIGHT)
    .height('100%')
    .padding({ left: '0.4%', right: '2.4%', top: '2%', bottom: '2%' })
  }

  /** 快照未到 / 加载失败：外框保持，中间给明确占位；失败时可原地重试。 */
  @Builder
  ExpandedPlaceholder() {
    Row() {
      Column() {
        Row() {
          Text(widePresetLabel(this.selectedPeriod))
            .fontSize(COMPACT_CARD_TITLE_FONT_VP)
            .fontWeight(FontWeight.Medium)
            .fontColor($r('sys.color.font_primary'))
            .layoutWeight(1)
          SymbolGlyph($r('sys.symbol.chevron_down'))
            .fontSize(20)
            .fontColor([$r('sys.color.icon_secondary')])
            .onClick(() => {
              this.onClose()
            })
        }
        .width('100%')
      }
      .layoutWeight(WIDE_SUMMARY_WEIGHT)
      .height('100%')
      .padding({ left: '2.4%', right: '1.6%', top: '2%' })

      Column({ space: 10 }) {
        if (this.isLoading) {
          LoadingProgress()
            .width(22)
            .height(22)
          Text(`正在加载${widePresetLabel(this.selectedPeriod)}统计...`)
            .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
            .fontColor($r('sys.color.font_tertiary'))
        } else {
          Text('该时段统计暂不可用')
            .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
            .fontColor($r('sys.color.font_secondary'))
          Button($r('app.string.retry'))
            .fontSize(12)
            .onClick(() => {
              this.onRetry()
            })
        }
      }
      .layoutWeight(WIDE_RECORDS_WEIGHT + WIDE_HEATMAP_WEIGHT)
      .height('100%')
      .justifyContent(FlexAlign.Center)
      .alignItems(HorizontalAlign.Center)
    }
    .width('100%')
    .layoutWeight(1)
  }
}
