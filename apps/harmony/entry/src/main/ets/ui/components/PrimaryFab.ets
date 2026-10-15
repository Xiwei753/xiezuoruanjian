// PrimaryFab — 全局统一的「主操作」悬浮按钮（圆形 + 号），固定在内容流右下角。
//
// 三个页面共用同一个组件：作品页（新建作品）、工作区页（新建卷）、星图页（新建星图）。
// 之前三处各自写 Button，背景色一个没设（系统蓝）、一个写死 themeConfig.colors.primary（纯黑），
// 才出现「作品页和星图页加号颜色不一样」。统一到这里以后配色只有一份。
//
// Issue #905：本组件维持**内容流右下角独立主操作按钮**的原始设计位置，不做后退。
//   #888 给内容流 Button 挂 .systemMaterial() 被系统拒绝，真机日志：
//   `Material inactive: out of scope. Use component in navigation title bar or Tabbar.`。
//   华为《开启沉浸光感》的公开生效范围只有 Navigation/NavDestination 标题栏与合规底部
//   TabBar，内容流右下角 FAB 不在范围内；换外层容器、改 modifier 顺序都不能解锁材质。
//   第二~五轮试过的「把 + 搬到 HDS 底部 MiniBar 承载光感」分支已按复核意见整体撤销：
//   不得为了拿不到的原生材质改变用户确定的按钮位置。
//   因此这里既不挂 systemMaterial，也不用 backgroundBlurStyle + shadow 伪造光效；
//   系统卡片底色 + stateEffect 原生按压反馈**不等于**华为沉浸光感，不要写成已补回原生光感——
//   该项在 #905 仍按未完成保留（公开 SDK 下「原位置 + 普通 FAB + 同款原生沉浸光感」无可用实现）。
//
// 手势：按住时锁住底栏的横向滑动切页，避免按住加号横向拖动时页面被带走。
// 锁的实现见 app/navigation/PrimaryTabGestureLock.ets。

import { PRIMARY_FAB_SIZE_VP } from '../theme/LayoutTokens'
import { PrimaryTabGestureLock } from '../../app/navigation/PrimaryTabGestureLock'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../../platform/diagnostics/HarmonyDiagnosticEvents'

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
      // Issue #906：主操作「请求」事件记录在唯一真实点击入口（原 MiniBar 通道已撤销）。
      // 只在未拖动且确实调用 onTap 时记录；只记请求不记完成，业务成功与否由页面创建入口决定。
      const fields: DiagnosticField[] = [{ key: 'source', value: 'content_fab' }]
      HarmonyDiagnosticEvents.record('Info', 'User', 'ui.primary_action_requested', 'PrimaryFab', fields)
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
