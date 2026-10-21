// WidePeriodCards — 宽屏统计默认总览的四张高时段卡（Issue #972）。
//
// 横向四等分：近七天 / 近三十天 / 近半年 / 自选区间。每张卡按原图从上到下显示
// 周期名、总人写字数、平均速度（带展开 chevron）、日均字数、日期范围。
// 点预设卡由 StatsScreen 切快照并在原位置展开三栏；自选卡直接打开系统日期选择。
//
// 本组件不持有统计状态、不重算 Core 数据，只格式化 Explorer.periodCards。

import {
  WritingStatsPeriodCard,
  STATS_PRESET_LAST_7_DAYS,
  STATS_PRESET_LAST_30_DAYS,
  STATS_PRESET_LAST_6_MONTHS,
  STATS_PRESET_LIFETIME,
  STATS_PRESET_CUSTOM
} from '../../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { COMPACT_CARD_TITLE_FONT_VP } from '../../../../ui/theme/LayoutTokens'
import { formatAverage, formatAveragePerHour, formatDateRangeHuman, formatInt } from '../StatsFormat'

// 四卡横排的间距与单卡宽高比：卡间距约为内容宽的 3.2%，单卡约 218 x 351vp。
// 卡片宽度由 layoutWeight 等分，高度由 aspectRatio 派生，行高自然跟随卡片。
// 四卡等分后单卡约占内容宽 23.65%，剩余 5.4%（每道约 1.8%）由 SpaceBetween 均分。
const WIDE_CARD_WIDTH_PERCENT: string = '23.65%'
const WIDE_CARD_ASPECT: number = 0.647
// 空态占位与四卡整行同 footprint（973 x 351vp，宽高比约 2.78）。
const WIDE_CARD_ROW_ASPECT: number = 2.78
// 卡内边距约为内容宽的 1.6%（参考约 16vp @973vp）。
const WIDE_CARD_PADDING_PERCENT: string = '1.6%'

/** 时段卡标题：Core 固定 preset 字符串 → 中文。 */
export function widePresetLabel(preset: string): string {
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

/** 预设卡用短日期段（MM/DD - MM/DD）；自选区间保留完整日期，跨年也清楚。 */
function cardRangeText(card: WritingStatsPeriodCard): string {
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

@Component
export struct WidePeriodCards {
  @Prop cards: WritingStatsPeriodCard[] = []
  @Prop selectedPeriod: string = STATS_PRESET_LAST_7_DAYS
  @Prop isLoading: boolean = false
  onOpenPeriod: (preset: string) => void = (_preset: string) => {}
  onPickCustomRange: () => void = () => {}
  onRetry: () => void = () => {}

  build() {
    Column() {
      if (this.cards.length === 0) {
        this.EmptyCards()
      } else {
        Row() {
          ForEach(this.cards, (card: WritingStatsPeriodCard) => {
            this.PeriodCard(card)
          }, (card: WritingStatsPeriodCard) => card.preset)
        }
        .width('100%')
        .justifyContent(FlexAlign.SpaceBetween)
        .alignItems(VerticalAlign.Top)
      }
    }
    .width('100%')
  }

  /** Explorer 没有四卡时不留空白：占位与四卡整行保持同一比例，并保留重试入口。 */
  @Builder
  EmptyCards() {
    Column({ space: 10 }) {
      Text('暂无周期统计')
        .fontSize(COMPACT_CARD_TITLE_FONT_VP)
        .fontColor($r('sys.color.font_secondary'))
      Button($r('app.string.retry'))
        .fontSize(12)
        .onClick(() => {
          this.onRetry()
        })
    }
    .width('100%')
    .aspectRatio(WIDE_CARD_ROW_ASPECT)
    .justifyContent(FlexAlign.Center)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
  }

  /** 单张高卡：宽度由 layoutWeight 等分，高度按 0.62 宽高比派生。 */
  @Builder
  PeriodCard(card: WritingStatsPeriodCard) {
    Column() {
      Row() {
        Text(widePresetLabel(card.preset))
          .fontSize(COMPACT_CARD_TITLE_FONT_VP)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .layoutWeight(1)
        if (this.isLoading && this.selectedPeriod === card.preset) {
          LoadingProgress()
            .width(16)
            .height(16)
        }
      }
      .width('100%')

      Blank()

      if (card.hasSelection && card.range !== null) {
        Text(`${formatInt(card.totalHumanTypedChars)} 字`)
          .fontSize(32)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        Row() {
          Text(`${formatAveragePerHour(card.averageCharsPerMinute)} 字/时`)
            .fontSize(16)
            .fontColor($r('sys.color.font_secondary'))
            .layoutWeight(1)
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          SymbolGlyph($r('sys.symbol.chevron_up'))
            .fontSize(18)
            .fontColor([$r('sys.color.icon_secondary')])
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)
        .margin({ top: 16 })

        Text(`${formatAverage(card.averageCharsPerDay)} 字/天`)
          .fontSize(16)
          .fontColor($r('sys.color.font_secondary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .margin({ top: 10 })

        Blank()

        Text(cardRangeText(card))
          .fontSize(13)
          .fontColor($r('sys.color.font_tertiary'))
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .width('100%')
          .textAlign(TextAlign.End)
      } else {
        Text('暂无数据')
          .fontSize(22)
          .fontColor($r('sys.color.font_secondary'))
        Text(card.preset === STATS_PRESET_CUSTOM ? '点按选择日期区间' : '点按重新加载')
          .fontSize(13)
          .fontColor($r('sys.color.font_tertiary'))
          .margin({ top: 8 })
        Blank()
      }
    }
    .width(WIDE_CARD_WIDTH_PERCENT)
    .aspectRatio(WIDE_CARD_ASPECT)
    .alignItems(HorizontalAlign.Start)
    .padding(WIDE_CARD_PADDING_PERCENT)
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
}
