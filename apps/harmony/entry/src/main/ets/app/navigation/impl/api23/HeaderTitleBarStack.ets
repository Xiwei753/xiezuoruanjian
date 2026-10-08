// HeaderTitleBarStack — 标题栏 stackBuilder 自绘区域的统一实现。
//
// 三个 NavDestination 壳共用：根页面壳（作品/星图列表/星图详情/统计）、
// 二级页面壳（工作区）、二级页面壳（写作页）。三处以前各写各的，
// 根页面已是胶囊而两个二级页面还是原生三圆，观感不统一。
//
// 为什么用 stackBuilder 而不是 titleBar.content.menu：HDS 的 menu.value 每一项都是
// 独立的圆形面，三项之间断开，没有「容器」概念可套一个整体外框
// —— HdsNavigationIconItemStyle 只有 backgroundColor / iconColor / textColor
// 三个颜色字段，没有圆角、没有外层形状。官方为此留的口子是
// titleBar.content.stackBuilder（6.0.0(20) 起）。详见 HeaderActionCapsule 顶部注释。
//
// 两个实测出来的坑（根页面壳上踩过，这里一并处理掉）：
//   1. stackBuilder 是自绘区域，不吃 HDS 给 mainTitle 准备的内边距，左右要自己补，
//      否则标题顶到屏幕左边缘（实测左边界 53px → 3px）。
//   2. HDS 返回按钮画在标题栏自己的层上，**不会**因为 stackBuilder 存在就自动避让，
//      会直接压住自绘标题（实测返回按钮面占 x 50~174px）。所以显示返回按钮时
//      必须自己让出槽位：16 内边距 + 40 返回按钮 + 4 间距 = 60vp。

import { ActionSlot } from '../../../../corebridge/dto/PlatformDtos'
import { HeaderActionCapsule } from './HeaderActionCapsule'

/** 标题左右内边距（vp），与 HDS 标题栏默认值一致 */
const TITLE_BAR_PADDING_HORIZONTAL_VP: number = 16
/** HDS 返回按钮占位（vp） */
const BACK_BUTTON_SLOT_VP: number = 40
/** 返回按钮与标题之间的间距（vp） */
const BACK_BUTTON_GAP_VP: number = 4

@ComponentV2
export struct HeaderTitleBarStack {
  @Param title: string | Resource = ''
  @Param headerActions: ActionSlot[] = []
  /** 标题栏是否显示返回按钮；显示时左侧要让出返回按钮的槽位 */
  @Param showBackButton: boolean = false

  build() {
    Row() {
      Text(this.title)
        .layoutWeight(1)
        .fontSize(26)
        .fontWeight(FontWeight.Bold)
        .fontColor($r('sys.color.font_primary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })

      // Issue #825 复核第1项：HeaderTrailing 动作恒显示（属于 Core 的 ToolbarTrailing 角色）。
      HeaderActionCapsule({ actionSlots: this.headerActions, inHdsTitleBar: true })
    }
    .width('100%')
    .alignItems(VerticalAlign.Center)
    .padding({
      left: this.showBackButton
        ? TITLE_BAR_PADDING_HORIZONTAL_VP + BACK_BUTTON_SLOT_VP + BACK_BUTTON_GAP_VP
        : TITLE_BAR_PADDING_HORIZONTAL_VP,
      right: TITLE_BAR_PADDING_HORIZONTAL_VP
    })
  }
}
