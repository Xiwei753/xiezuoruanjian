// PaneFoldButton — 面板顶部的折叠钮（Issue #829 第二轮）。
//
// 手稿里折叠有两种入口，不是同一个东西：
//   - 面板顶部的三角钮：面板「还开着」时主动收起；
//   - 窗口贴边把手：面板「已经收起」后重新拉开。
// 两张手稿都画了两者，所以贴边把手保留，这里只补顶部的折叠钮。
//
// 本组件是纯 UI：不含 Core / 编辑会话 / 同步状态，只发 triggered 回调。

import { DesignTokens } from '../../../ui/theme/DesignTokens'

/** 折叠钮边长（vp），与手稿里的小圆钮比例一致 */
const FOLD_BUTTON_SIZE_VP: number = 28

@ComponentV2
export struct PaneFoldButton {
  /** '⌄'（向下，展开态收起）/ '⌃'（向上，收起态拉开） */
  @Param glyph: string = '⌄'
  @Param onTriggered: () => void = () => {}

  @Builder
  build() {
    Button({ type: ButtonType.Normal }) {
      Text(this.glyph)
        .fontSize(14)
        .fontColor($r('sys.color.icon_primary'))
    }
    .width(FOLD_BUTTON_SIZE_VP)
    .height(FOLD_BUTTON_SIZE_VP)
    .borderRadius(DesignTokens.radiusPill)
    .backgroundColor($r('sys.color.comp_background_gray'))
    .onClick(() => {
      this.onTriggered()
    })
  }
}
