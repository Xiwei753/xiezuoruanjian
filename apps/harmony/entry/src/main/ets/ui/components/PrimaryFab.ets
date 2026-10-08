// PrimaryFab — 全局统一的「主操作」悬浮按钮（圆形 + 号）。
//
// 三个页面共用同一个组件：作品页（新建作品）、工作区页（新建卷）、星图页（新建星图）。
// 之前三处各自写 Button，背景色一个没设（系统蓝）、一个写死 themeConfig.colors.primary（纯黑），
// 才出现「作品页和星图页加号颜色不一样」。统一到这里以后配色只有一份。
//
// Issue #880：修复加号按钮可见性问题。
//   之前用透明描边 Row + zIndex 叠层 + systemMaterial（内容流里不生效），
//   白底上外层 #22FFFFFF 描边几乎完全透明。
//   现在改成真实的 ButtonType.Circle 按钮：
//     - 系统卡片底色 sys.color.ohos_id_color_card_bg（随亮暗模式反转）
//     - 系统描边 token sys.color.icon_secondary（随亮暗模式反转），1vp 清晰边框
//     - stateEffect: true 保留正常点击态
//   边框画在按钮本体上，不再用另一棵假装有光感的树。
//
// 手势：按住时锁住底栏的横向滑动切页，避免按住加号横向拖动时页面被带走。
// 锁的实现见 app/navigation/PrimaryTabGestureLock.ets。

import { PRIMARY_FAB_SIZE_VP } from '../theme/LayoutTokens'
import { PrimaryTabGestureLock } from '../../app/navigation/PrimaryTabGestureLock'

// 判定「按住又拖动」的位移阈值（vp）。低于它算点击，高于它算拖动。
const DRAG_SLOP_VP: number = 8

@ComponentV2
export struct PrimaryFab {
  /** 点击回调 */
  @Param onTap: () => void = () => {}
  /** 按住后是否已经拖动过（拖过就吞掉这次点击） */
  @Local dragged: boolean = false
  /** 按下位置（组件内坐标），用于判定是否拖动 */
  @Local downX: number = 0
  @Local downY: number = 0

  build() {
    Button({ type: ButtonType.Circle, stateEffect: true }) {
      SymbolGlyph($r('sys.symbol.plus'))
        .fontSize(28)
        .fontColor([$r('sys.color.icon_primary')])
    }
    .width(PRIMARY_FAB_SIZE_VP)
    .height(PRIMARY_FAB_SIZE_VP)
    .borderRadius(PRIMARY_FAB_SIZE_VP / 2)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .border({ width: 1, color: $r('sys.color.icon_secondary') })
    .onTouch((event: TouchEvent) => {
      const touch = event.touches[0]
      if (event.type === TouchType.Down) {
        this.dragged = false
        this.downX = touch.x
        this.downY = touch.y
        PrimaryTabGestureLock.getInstance().setLocked(true)
        event.stopPropagation()
        return
      }
      if (event.type === TouchType.Move) {
        if (Math.abs(touch.x - this.downX) > DRAG_SLOP_VP ||
          Math.abs(touch.y - this.downY) > DRAG_SLOP_VP) {
          this.dragged = true
        }
        return
      }
      PrimaryTabGestureLock.getInstance().setLocked(false)
    })
    .onClick(() => {
      if (this.dragged) {
        this.dragged = false
        return
      }
      if (this.onTap) {
        this.onTap()
      }
    })
  }
}
