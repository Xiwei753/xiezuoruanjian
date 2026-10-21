// WidePrimaryRail — 宽屏一级导航固定左栏。
//
// Issue #972：宽屏外壳从零重写。这里只负责「素笺」品牌与作品 / 星图 / 统计三项，
// 选中态使用系统强调色与系统灰色背景，不依赖 Tabs、HdsTabs 或底部导航。
// 点击只向壳层发出用户 Tab 请求，路由仍由 PrimaryTabShell 的三个 NavPathStack 承载。

import { DesignTokens } from '../../../ui/theme/DesignTokens'

// 左侧栏始终占窗口宽度的固定比例，避免把某一台平板的 vp 宽度写死。
const WIDE_RAIL_WIDTH_RATIO: string = '11.5%'
const RAIL_HORIZONTAL_PADDING_VP: number = 12
const RAIL_ITEM_HEIGHT_VP: number = 44

interface WideRailEntry {
  index: number
  label: string
  icon: Resource
}

const RAIL_ENTRIES: WideRailEntry[] = [
  { index: 0, label: '作品', icon: $r('sys.media.ohos_ic_public_text') },
  { index: 1, label: '星图', icon: $r('app.media.ic_star') },
  { index: 2, label: '统计', icon: $r('sys.media.ohos_ic_public_more') }
]

@Component
export struct WidePrimaryRail {
  /** 当前一级 Tab：0 作品 / 1 星图 / 2 统计 */
  @Prop currentTab: number = 0
  /** 顶部安全区高度（vp） */
  @Prop safeTopVp: number = 0
  /** 与右侧动作条共用的页头高度；由 WideAppShell 唯一决定 */
  @Prop headerHeight: string = '14.2%'
  /** 用户明确点击一级导航时交给壳层记录并切换 */
  onTabRequest: (index: number) => void = (_index: number): void => {}

  private isSelected(index: number): boolean {
    return this.currentTab === index
  }

  @Builder
  RailItem(entry: WideRailEntry) {
    Row({ space: 10 }) {
      Column()
        .width(3)
        .height(22)
        .borderRadius(DesignTokens.radiusXs)
        .backgroundColor($r('sys.color.ohos_id_color_emphasize'))
        .opacity(this.isSelected(entry.index) ? 1 : 0)

      Image(entry.icon)
        .width(20)
        .height(20)
        .fillColor(this.isSelected(entry.index)
          ? $r('sys.color.ohos_id_color_emphasize')
          : $r('sys.color.font_secondary'))

      Text(entry.label)
        .fontSize(16)
        .fontWeight(this.isSelected(entry.index) ? FontWeight.Medium : FontWeight.Normal)
        .fontColor(this.isSelected(entry.index)
          ? $r('sys.color.font_primary')
          : $r('sys.color.font_secondary'))
        .textAlign(TextAlign.Start)
        .layoutWeight(1)
    }
    .width('100%')
    .height(RAIL_ITEM_HEIGHT_VP)
    .padding({ left: 9, right: 12 })
    .alignItems(VerticalAlign.Center)
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor(this.isSelected(entry.index)
      ? $r('sys.color.comp_background_gray')
      : Color.Transparent)
    .onClick(() => {
      this.onTabRequest(entry.index)
    })
  }

  build() {
    Column() {
      Row() {
        Text('素笺')
          .fontSize(24)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
          .textAlign(TextAlign.Start)
      }
      .width('100%')
      .height(this.headerHeight)
      .padding({ left: 20, right: 20, top: this.safeTopVp })
      .alignItems(VerticalAlign.Center)
      .border({ width: { bottom: 0.5 }, color: $r('sys.color.comp_divider') })

      Column({ space: 4 }) {
        ForEach(RAIL_ENTRIES, (entry: WideRailEntry) => {
          this.RailItem(entry)
        }, (entry: WideRailEntry) => `${entry.index}`)

        Blank()
      }
      .width('100%')
      .layoutWeight(1)
      .padding({
        left: RAIL_HORIZONTAL_PADDING_VP,
        right: RAIL_HORIZONTAL_PADDING_VP,
        top: 12,
        bottom: 16
      })
    }
    .width(WIDE_RAIL_WIDTH_RATIO)
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
