// WidePaneDivider — 宽屏写作工作台的可拖动面板分隔条（Issue #972）。
//
// 左右各一条真实分隔条：
//   - 左条只改变 chapterNavWidthVp；
//   - 右条只改变 toolPaneWidthVp。
// 拖动用 PanGesture 累计位移 + 起手宽度计算目标值，不每帧叠加增量。
// 宽度上下限由 WritingScreen 传入（现有 CHAPTER_NAV_* / TOOL_PANE_* 常量），
// 松开后 WritingScreen 仍会跑原有 reconcilePaneSpace。
//
// 面板折叠后本组件变成贴边 handle：左栏折叠仍可从最左边重新拉开，
// 右栏折叠仍保留这条 handle 与最右常驻工具轨。

import { DesignTokens } from '../../../../ui/theme/DesignTokens'

// 参考图里的分栏线只有约 2vp；布局上不再额外占一条 12vp 的空白带。
// 视觉按钮可以溢出到线两侧，命中区通过 responseRegion 扩到 18vp。
const PANE_HANDLE_WIDTH_VP: number = 2
const PANE_HANDLE_VISUAL_WIDTH_VP: number = 14
const PANE_HANDLE_HEIGHT_VP: number = 48
const PANE_HANDLE_HIT_WIDTH_VP: number = 18

@ComponentV2
export struct WidePaneDivider {
  /** 'left' | 'right' */
  @Param side: string = 'left'
  @Param collapsed: boolean = false
  /** 当前面板宽度（vp），拖动起手时作为基准。 */
  @Param widthVp: number = 0
  @Param minWidthVp: number = 0
  @Param maxWidthVp: number = 0
  @Param onResize: (widthVp: number) => void = () => {}
  @Param onCollapse: () => void = () => {}
  @Param onExpand: () => void = () => {}

  private dragStartWidthVp: number = 0

  private handleGlyph(): string {
    if (this.collapsed) {
      return this.side === 'left' ? '›' : '‹'
    }
    return this.side === 'left' ? '‹' : '›'
  }

  private triggerHandle(): void {
    if (this.collapsed) {
      this.onExpand()
    } else {
      this.onCollapse()
    }
  }

  build() {
    Stack({ alignContent: Alignment.Center }) {
      Column()
        .width(1)
        .height('100%')
        .backgroundColor($r('sys.color.comp_divider'))

      // 视觉把手向两侧溢出，不参与 Row 的宽度分配；命中区由外层 Stack 扩展。
      Button({ type: ButtonType.Normal, stateEffect: true }) {
        Column() {
          Text(this.handleGlyph())
            .fontSize(14)
            .fontColor($r('sys.color.icon_secondary'))
        }
        .width(PANE_HANDLE_VISUAL_WIDTH_VP)
        .height(PANE_HANDLE_HEIGHT_VP)
        .borderRadius(DesignTokens.radiusPill)
        .backgroundColor($r('sys.color.background_secondary'))
        .justifyContent(FlexAlign.Center)
        .alignItems(HorizontalAlign.Center)
      }
      .width(PANE_HANDLE_VISUAL_WIDTH_VP)
      .height(PANE_HANDLE_HEIGHT_VP)
      .backgroundColor(Color.Transparent)
      .hitTestBehavior(HitTestMode.None)
      .position({ x: -(PANE_HANDLE_VISUAL_WIDTH_VP - PANE_HANDLE_WIDTH_VP) / 2 })
    }
    .width(PANE_HANDLE_WIDTH_VP)
    .height('100%')
    // 布局只占 2vp，横向命中区扩到 18vp，折叠后仍可从贴边把手拖开。
    .responseRegion({
      x: -(PANE_HANDLE_HIT_WIDTH_VP - PANE_HANDLE_WIDTH_VP) / 2,
      y: 0,
      width: PANE_HANDLE_HIT_WIDTH_VP,
      height: '100%'
    })
    .onClick(() => {
      this.triggerHandle()
    })
    .gesture(
      PanGesture()
        .onActionStart(() => {
          this.dragStartWidthVp = this.widthVp
        })
        .onActionUpdate((event: GestureEvent) => {
          if (this.collapsed) {
            return
          }
          // 左条向右拖变宽；右条向左拖变宽。event.offsetX 是相对起手点的累计位移。
          const delta: number = this.side === 'left' ? event.offsetX : -event.offsetX
          const target: number = this.dragStartWidthVp + delta
          const clamped: number = Math.max(this.minWidthVp, Math.min(this.maxWidthVp, target))
          this.onResize(clamped)
        })
    )
  }
}
