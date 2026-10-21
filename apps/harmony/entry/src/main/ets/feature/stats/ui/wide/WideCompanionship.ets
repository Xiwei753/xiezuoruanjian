// WideCompanionship — 宽屏统计的陪伴横条（Issue #972）。
//
// 放在「四卡 / 时段展开 / 生涯展开」之后、码字日历之前，横贯主画布。
// 天数直接取 Core lifetime.elapsedCalendarDays（首次写作日到今天，含两端），
// 不在端侧重新计算自然日。

import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { formatInt } from '../StatsFormat'

// 陪伴条高度不写死：上下留白取内容宽度的 4.2%，加上单行字号自然接近 11%
// （参考 108 / 973vp）；左右留白取内容宽度的 2.4%。
const WIDE_COMPANIONSHIP_VERTICAL_PADDING: string = '4.2%'
const WIDE_COMPANIONSHIP_HORIZONTAL_PADDING: string = '2.4%'

@Component
export struct WideCompanionship {
  @Prop elapsedCalendarDays: number = 0

  build() {
    Row() {
      Text(`素笺写作已陪伴您 ${formatInt(this.elapsedCalendarDays)} 天`)
        .fontSize(18)
        .fontColor($r('sys.color.font_secondary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
    }
    .width('100%')
    .padding({
      left: WIDE_COMPANIONSHIP_HORIZONTAL_PADDING,
      right: WIDE_COMPANIONSHIP_HORIZONTAL_PADDING,
      top: WIDE_COMPANIONSHIP_VERTICAL_PADDING,
      bottom: WIDE_COMPANIONSHIP_VERTICAL_PADDING
    })
    .alignItems(VerticalAlign.Center)
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
  }
}
