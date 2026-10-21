// WideCalendarHeatmap — 宽屏统计的独立码字日历区（Issue #972）。
//
// 位于陪伴横条之后，单独占一块卡片，不参与上方四卡 / 展开框的高度分配，
// 因此日历再宽也不会把四张高卡压扁。
// 网格直接复用 WritingStatsCalendarHeatmap：52/53 个自然周 × 7 天，
// 方格随实际可用宽度等分；数据只来自 Core 的 WritingStatsHeatmap 快照。

import { WritingStatsHeatmap } from '../../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { COMPACT_CARD_SUPPORTING_FONT_VP } from '../../../../ui/theme/LayoutTokens'
import { WritingStatsCalendarHeatmap } from '../WritingStatsCalendarHeatmap'

// 状态占位的上下留白取父内容宽度的 2%，不再固定 80vp 高度。
const WIDE_HEATMAP_STATUS_PADDING: string = '2%'
// 卡片内边距取父内容宽度的 1.6% / 1.4%（参考约 16vp / 14vp @973vp）。
const WIDE_HEATMAP_HORIZONTAL_PADDING: string = '1.6%'
const WIDE_HEATMAP_VERTICAL_PADDING: string = '1.4%'

@Component
export struct WideCalendarHeatmap {
  @Prop heatmap: WritingStatsHeatmap | null = null
  @Prop heatmapYear: number = new Date().getFullYear()
  @Prop heatmapError: string = ''
  onChangeYear: (year: number) => void = (_year: number) => {}
  onSelectDay: (date: string, chars: number) => void = (_date: string, _chars: number) => {}
  onRetry: () => void = () => {}

  build() {
    Column() {
      if (this.heatmap !== null && this.heatmap.days.length > 0) {
        WritingStatsCalendarHeatmap({
          heatmap: this.heatmap,
          onChangeYear: (year: number): void => {
            this.onChangeYear(year)
          },
          onSelectDay: (date: string, chars: number): void => {
            this.onSelectDay(date, chars)
          }
        })
      } else if (this.heatmap !== null) {
        this.EmptyPlaceholder()
      } else if (this.heatmapError.length > 0) {
        this.ErrorPlaceholder()
      } else {
        this.LoadingPlaceholder()
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({
      left: WIDE_HEATMAP_HORIZONTAL_PADDING,
      right: WIDE_HEATMAP_HORIZONTAL_PADDING,
      top: WIDE_HEATMAP_VERTICAL_PADDING,
      bottom: WIDE_HEATMAP_VERTICAL_PADDING
    })
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
  }

  /** 该年快照为空：明确说明没有可画的日历，并允许重新取一次。 */
  @Builder
  EmptyPlaceholder() {
    Row({ space: 12 }) {
      Text(`${this.heatmapYear} 年暂无码字日历数据`)
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.font_tertiary'))
        .layoutWeight(1)
      Button($r('app.string.retry'))
        .fontSize(12)
        .onClick(() => {
          this.onRetry()
        })
    }
    .width('100%')
    .padding({ top: WIDE_HEATMAP_STATUS_PADDING, bottom: WIDE_HEATMAP_STATUS_PADDING })
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  LoadingPlaceholder() {
    Row({ space: 8 }) {
      LoadingProgress()
        .width(16)
        .height(16)
      Text(`${this.heatmapYear} 年 · 码字日历加载中...`)
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.font_tertiary'))
    }
    .width('100%')
    .padding({ top: WIDE_HEATMAP_STATUS_PADDING, bottom: WIDE_HEATMAP_STATUS_PADDING })
    .justifyContent(FlexAlign.Center)
  }

  /** 日历查询失败：错误文案留在独立区域内，并给原地重试入口。 */
  @Builder
  ErrorPlaceholder() {
    Row({ space: 12 }) {
      Text(this.heatmapError)
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.ohos_id_color_alert'))
        .layoutWeight(1)
        .maxLines(2)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
      Button($r('app.string.retry'))
        .fontSize(12)
        .onClick(() => {
          this.onRetry()
        })
    }
    .width('100%')
    .padding({ top: WIDE_HEATMAP_STATUS_PADDING, bottom: WIDE_HEATMAP_STATUS_PADDING })
    .alignItems(VerticalAlign.Center)
  }
}
