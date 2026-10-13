// FloatingPageHeader — 内容流悬浮标题栏（窄屏专用）。
//
// Issue #888：删除顶栏整条白底后的替代方案。
// 之前 HDS 原生标题栏占独立一行，背后是不透明白色 surface，与页面内容割裂。
// 现在改为悬浮在页面主 Stack 最上层：标题和动作胶囊背后透出的都是同一个
// background_secondary 页面底色，不再有独立标题栏高度占位。
//
// 与 HeaderTitleBarStack 的区别：
//   HeaderTitleBarStack 是 HDS 标题栏 stackBuilder 自绘区域的内容，仍在标题栏层；
//   FloatingPageHeader 是内容流里的悬浮节点，不在标题栏层，不需要 HDS titleBar。
//
// 只接收 title、showBackButton、onBack 和 ActionSlot[]。
// 外层没有不透明整行背景，也不参与独立标题栏高度占位。
// 内层是一行「标题/返回按钮 + 同步/搜索/设置」。

import { ActionSlot } from '../../corebridge/dto/PlatformDtos'
import { HeaderActionCapsule } from '../../app/navigation/impl/api23/HeaderActionCapsule'

/** 标题左右内边距（vp），与页面内容区一致 */
const HEADER_PADDING_HORIZONTAL_VP: number = 16
/** 返回按钮占位宽度（vp） */
const BACK_BUTTON_SLOT_VP: number = 40
/** 返回按钮与标题之间的间距（vp） */
const BACK_BUTTON_GAP_VP: number = 4
/** 返回按钮图标尺寸（vp） */
const BACK_ICON_SIZE_VP: number = 24
/** 标题字号（vp），与 HeaderTitleBarStack 一致 */
const TITLE_FONT_SIZE_VP: number = 26

@ComponentV2
export struct FloatingPageHeader {
  /** 标题文字 */
  @Param title: string | Resource = ''
  /** 是否显示返回按钮 */
  @Param showBackButton: boolean = false
  /** 返回按钮回调 */
  @Param onBack: (() => void) | null = null
  /** 页头动作列表（同步/搜索/设置等） */
  @Param actionSlots: ActionSlot[] = []

  build() {
    Row() {
      // 返回按钮（仅二级页面显示）
      if (this.showBackButton) {
        Button({ type: ButtonType.Normal }) {
          SymbolGlyph($r('sys.symbol.chevron_left'))
            .fontSize(BACK_ICON_SIZE_VP)
            .fontColor([$r('sys.color.icon_primary')])
        }
        .width(BACK_BUTTON_SLOT_VP)
        .height(BACK_BUTTON_SLOT_VP)
        .borderRadius(BACK_BUTTON_SLOT_VP / 2)
        .backgroundColor(Color.Transparent)
        .stateEffect(false)
        .onClick(() => {
          if (this.onBack) {
            this.onBack()
          }
        })
      }

      // 标题
      Text(this.title)
        .layoutWeight(1)
        .fontSize(TITLE_FONT_SIZE_VP)
        .fontWeight(FontWeight.Bold)
        .fontColor($r('sys.color.font_primary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .margin({
          left: this.showBackButton ? BACK_BUTTON_GAP_VP : 0
        })

      // 页头动作胶囊（同步/搜索/设置）
      HeaderActionCapsule({
        actionSlots: this.actionSlots,
        inHdsTitleBar: false
      })
    }
    .width('100%')
    .alignItems(VerticalAlign.Center)
    .padding({
      left: this.showBackButton
        ? HEADER_PADDING_HORIZONTAL_VP
        : HEADER_PADDING_HORIZONTAL_VP,
      right: HEADER_PADDING_HORIZONTAL_VP
    })
    // 悬浮标题栏没有不透明背景，背后透出页面底色 background_secondary
  }
}
