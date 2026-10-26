// WideStatisticsPage — 宽屏统计正文（Issue #972）。
//
// 结构严格按 docs/ui/reference/widescreen 的统计三张原图：
// - 顶部居中的生涯概览条；点按原地展开成全宽纪录卡，展开时四卡隐藏；
// - 默认四张高时段卡横排；点卡后原位置换成「左摘要 / 中纪录 / 右大图」三栏；
// - 周期区之后是常驻陪伴横条；
// - 生涯展开态只展示生涯纪录；年度码字热力图在陪伴横条后独立全宽显示。
//
// 尺寸不再按某块平板写死：内容列取可用宽度的 88.3% 居中，纵向留白和段间距
// 用 aspectRatio 由内容宽度派生，卡片/展开框各自用宽高比自适应。
//
// 本组件只消费 StatsScreen 已经解码好的 Core 快照与回调：不查 Core、不重算统计，
// 也不创建二级页面；侧栏 / 顶栏由宽屏导航壳提供。

import {
  WritingStatsExplorer,
  WritingStatsHeatmap,
  WritingStatsHeatmapDay,
  STATS_PRESET_LAST_7_DAYS
} from '../../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { COMPACT_CARD_SUPPORTING_FONT_VP } from '../../../../ui/theme/LayoutTokens'
import {
  EMPTY_STATS_EXPLORER,
  STATS_PANEL_LIFETIME,
  STATS_PANEL_NONE,
  STATS_PANEL_RANGE
} from '../StatsFormat'
import { WideLifetimeSummary } from './WideLifetimeSummary'
import { WidePeriodCards } from './WidePeriodCards'
import { WidePeriodExpanded } from './WidePeriodExpanded'
import { WideCompanionship } from './WideCompanionship'
import { WideCalendarHeatmap } from './WideCalendarHeatmap'

// 统计内容占宽屏正文可用宽度的比例（参考图约 973 / 1102vp）。
const WIDE_CONTENT_WIDTH: string = '88.3%'
// 以下 aspectRatio = 内容宽度 / 目标留白高度，留白随内容宽度等比缩放。
// 顶部约 23vp（973 / 23 ≈ 42.3），段间约 26vp（973 / 26 ≈ 37.4），底部约 32vp。
const WIDE_TOP_INSET_ASPECT: number = 42.3
const WIDE_SECTION_GAP_ASPECT: number = 37.4
// 生涯展开后概览条本身占满原四卡区域，陪伴条与它之间约 55vp（973 / 55 ≈ 17.7），
// 比普通段间距大，参考图里这一段留白明显更宽。
const WIDE_LIFETIME_EXPANDED_GAP_ASPECT: number = 17.7
const WIDE_BOTTOM_INSET_ASPECT: number = 30.4

@Component
export struct WideStatisticsPage {
  @Prop explorer: WritingStatsExplorer = EMPTY_STATS_EXPLORER
  @Prop selectedPeriod: string = STATS_PRESET_LAST_7_DAYS
  @Prop expandedPanel: string = STATS_PANEL_NONE
  @Prop isLoading: boolean = false
  @Prop errorMessage: string = ''
  @Prop heatmap: WritingStatsHeatmap | null = null
  @Prop heatmapYear: number = new Date().getFullYear()
  @Prop heatmapError: string = ''
  @Prop periodHeatmapDays: WritingStatsHeatmapDay[] = []
  @Prop periodHeatmapLoadedYears: number[] = []
  @Prop periodHeatmapLoading: boolean = false
  @Prop periodHeatmapError: string = ''

  onMounted: () => void = () => {}
  onChangeHeatmapYear: (year: number) => void = (_year: number) => {}
  onSelectHeatmapDay: (date: string, chars: number) => void = (_date: string, _chars: number) => {}
  onOpenPeriod: (preset: string) => void = (_preset: string) => {}
  onPickCustomRange: () => void = () => {}
  onToggleLifetime: () => void = () => {}
  onCloseExpanded: () => void = () => {}
  onRetry: () => void = () => {}
  onRetryHeatmap: () => void = () => {}
  onRetryPeriodHeatmap: () => void = () => {}
  onVisiblePeriodHeatmapYears: (years: number[]) => void = (_years: number[]) => {}

  build() {
    Column() {
      Scroll() {
        Row() {
          Column() {
            this.TopInset()

            if (this.errorMessage.length > 0) {
              this.ErrorStrip()
              this.SectionGap()
            }

            Row() {
              WideLifetimeSummary({
                explorer: this.explorer,
                expanded: this.expandedPanel === STATS_PANEL_LIFETIME,
                isLoading: this.isLoading,
                onToggle: (): void => {
                  this.onToggleLifetime()
                },
                onRetry: (): void => {
                  this.onRetry()
                }
              })
            }
            .width('100%')
            .justifyContent(FlexAlign.Center)

            if (this.expandedPanel === STATS_PANEL_LIFETIME) {
              // 生涯展开态没有四卡区，陪伴条直接跟在展开框后，但留白比普通段间距大。
              this.LifetimeExpandedGap()
            } else {
              this.SectionGap()

              if (this.expandedPanel === STATS_PANEL_RANGE) {
                WidePeriodExpanded({
                  selectedPeriod: this.selectedPeriod,
                  selected: this.explorer.selected,
                  historyScope: this.explorer.historyScope,
                  isLoading: this.isLoading,
                  periodHeatmapDays: this.periodHeatmapDays,
                  periodHeatmapLoadedYears: this.periodHeatmapLoadedYears,
                  periodHeatmapLoading: this.periodHeatmapLoading,
                  periodHeatmapError: this.periodHeatmapError,
                  onClose: (): void => {
                    this.onCloseExpanded()
                  },
                  onRetry: (): void => {
                    this.onRetry()
                  },
                  onRetryPeriodHeatmap: (): void => {
                    this.onRetryPeriodHeatmap()
                  },
                  onVisiblePeriodHeatmapYears: (years: number[]): void => {
                    this.onVisiblePeriodHeatmapYears(years)
                  }
                })
              } else {
                WidePeriodCards({
                  cards: this.explorer.periodCards,
                  selectedPeriod: this.selectedPeriod,
                  isLoading: this.isLoading,
                  onOpenPeriod: (preset: string): void => {
                    this.onOpenPeriod(preset)
                  },
                  onPickCustomRange: (): void => {
                    this.onPickCustomRange()
                  },
                  onRetry: (): void => {
                    this.onRetry()
                  }
                })
              }
              this.SectionGap()
            }

            WideCompanionship({
              elapsedCalendarDays: this.explorer.lifetime.elapsedCalendarDays
            })

            this.SectionGap()

            WideCalendarHeatmap({
              heatmap: this.heatmap,
              heatmapYear: this.heatmapYear,
              heatmapError: this.heatmapError,
              onChangeYear: (year: number): void => {
                this.onChangeHeatmapYear(year)
              },
              onSelectDay: (date: string, chars: number): void => {
                this.onSelectHeatmapDay(date, chars)
              },
              onRetry: (): void => {
                this.onRetryHeatmap()
              }
            })

            this.BottomInset()
          }
          .width(WIDE_CONTENT_WIDTH)
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .justifyContent(FlexAlign.Center)
        .alignItems(VerticalAlign.Top)
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Off)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .onAppear(() => {
      this.onMounted()
    })
  }

  /** 顶部留白：高度 = 内容宽度 / 42.3（约 23vp @973vp）。 */
  @Builder
  TopInset() {
    Row()
      .width('100%')
      .aspectRatio(WIDE_TOP_INSET_ASPECT)
  }

  /** 模块间距：高度 = 内容宽度 / 37.4（约 26vp @973vp）。 */
  @Builder
  SectionGap() {
    Row()
      .width('100%')
      .aspectRatio(WIDE_SECTION_GAP_ASPECT)
  }

  /** 生涯展开框与陪伴条之间的留白：约 55vp @973vp。 */
  @Builder
  LifetimeExpandedGap() {
    Row()
      .width('100%')
      .aspectRatio(WIDE_LIFETIME_EXPANDED_GAP_ASPECT)
  }

  /** 底部留白：高度 = 内容宽度 / 30.4（约 32vp @973vp）。 */
  @Builder
  BottomInset() {
    Row()
      .width('100%')
      .aspectRatio(WIDE_BOTTOM_INSET_ASPECT)
  }

  /** 页内错误条：Core 查询失败 / 自选日期非法时保留重试入口。 */
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
        .onClick(() => {
          this.onRetry()
        })
    }
    .width('100%')
    .padding({ left: 12, right: 12, top: 8, bottom: 8 })
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor($r('sys.color.comp_background_gray'))
  }
}
