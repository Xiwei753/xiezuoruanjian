// WideTopActions — 宽屏右上全局动作条。
//
// Issue #972：宽屏顶栏从零重写为普通 ArkUI Button，不再使用 HDS HeaderActionCapsule
// 或 systemMaterial。动作仍完全来自平台动作表 ScreenSemantics，并通过唯一 dispatcher 执行。

import { ActionRole, ActionSlot } from '../ScreenSemantics'
import { sortedHeaderTrailingActions } from '../NavigationHeaderActionModel'
import { NavigationHeaderActionDispatcher } from '../NavigationHeaderActionDispatcher'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

const TOP_ACTION_HEIGHT_VP: number = 40

@Component
export struct WideTopActions {
  /** 平台动作表下发的页头动作槽 */
  @Prop actionSlots: ActionSlot[] = []
  /** 与左侧品牌共用的页头高度；由 WideAppShell 唯一决定 */
  @Prop headerHeight: string = '14.2%'
  /** 顶部安全区高度（vp） */
  @Prop safeTopVp: number = 0
  /** 是否显示右上同步 / 搜索 / 设置。写作页等由壳层整体隐藏一级导航时不会渲染。 */
  @Prop visible: boolean = true

  private iconOf(role: ActionRole): Resource {
    if (role === ActionRole.Sync) {
      return $r('app.media.ic_refresh')
    }
    if (role === ActionRole.Search) {
      return $r('app.media.ic_search')
    }
    return $r('app.media.ic_settings')
  }

  private labelOf(role: ActionRole): string {
    if (role === ActionRole.Sync) {
      return '同步'
    }
    if (role === ActionRole.Search) {
      return '搜索'
    }
    return '设置'
  }

  @Builder
  ActionButton(slot: ActionSlot) {
    Button({ type: ButtonType.Normal }) {
      Row({ space: 6 }) {
        Image(this.iconOf(slot.role))
          .width(20)
          .height(20)
          .fillColor($r('sys.color.font_primary'))

        Text(this.labelOf(slot.role))
          .fontSize(14)
          .fontColor($r('sys.color.font_primary'))
      }
      .alignItems(VerticalAlign.Center)
    }
    .height(TOP_ACTION_HEIGHT_VP)
    .padding({ left: 12, right: 12 })
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor(Color.Transparent)
    .stateEffect(true)
    .accessibilityText(this.labelOf(slot.role))
    .onClick(() => {
      NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
    })
  }

  build() {
    if (this.visible) {
      Row({ space: 4 }) {
        ForEach(sortedHeaderTrailingActions(this.actionSlots), (slot: ActionSlot) => {
          this.ActionButton(slot)
        }, (slot: ActionSlot) => `${slot.role}`)
      }
      .width('100%')
      .height(this.headerHeight)
      .padding({
        left: 24,
        right: 24,
        top: this.safeTopVp
      })
      .justifyContent(FlexAlign.End)
      .alignItems(VerticalAlign.Center)
      .border({ width: { bottom: 0.5 }, color: $r('sys.color.comp_divider') })
    }
  }
}
