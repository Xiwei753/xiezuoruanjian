// PrimaryFab — 全局统一的「主操作」悬浮按钮（圆形 + 号）。
//
// 三个页面共用同一个组件：作品页（新建作品）、工作区页（新建卷）、星图页（新建星图）。
// 之前三处各自写 Button，背景色一个没设（系统蓝）、一个写死 themeConfig.colors.primary（纯黑），
// 才出现「作品页和星图页加号颜色不一样」。统一到这里以后配色只有一份。
//
// Issue #905：删除错误的「内容流启用系统沉浸光感」路线。
//   #888 把 .systemMaterial() 挂在内容流的 Button 上，系统明确拒绝并打
//   `Material inactive: out of scope. Use component in navigation title bar or Tabbar.`。
//   华为《开启沉浸光感》明确：普通 Button 的 API 26 材质只在 Navigation/NavDestination
//   标题栏或合规底部 TabBar 生效，内容流右下 FAB 不属于该范围。
//   换外层 Row/Stack 或重排 modifier 顺序都不能解锁材质，所以这里不再维护明知失效的材质分支。
//   设计边界：FAB 固定在屏幕右下时，现有公开 SDK 给不了它与 HDS titleBar/tabBar 同一种
//   原生跟手光；要真实同款光效必须调整它的 HDS 承载位置（另议）。
//   现在使用系统表面色 token 的一套 Button + 系统自带按钮按压反馈，
//   恢复可读的系统按钮外观与操作，不再自绘「假跟手光」。
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

  /** 共享触摸处理：按下锁切页、移动判拖动、抬起解锁 */
  private handleTouch(event: TouchEvent): void {
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
  }

  /** 共享点击处理：拖动过则吞掉，否则执行 onTap */
  private handleClick(): void {
    if (this.dragged) {
      this.dragged = false
      return
    }
    if (this.onTap) {
      this.onTap()
    }
  }

  build() {
    // 内容流位置：系统卡片底色 + 系统描边 token + 系统自带按压反馈（stateEffect）。
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
    .onTouch((event: TouchEvent) => { this.handleTouch(event) })
    .onClick(() => { this.handleClick() })
  }
}
