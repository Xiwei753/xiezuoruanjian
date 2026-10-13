// PrimaryFab — 全局统一的「主操作」悬浮按钮（圆形 + 号）。
//
// 三个页面共用同一个组件：作品页（新建作品）、工作区页（新建卷）、星图页（新建星图）。
// 之前三处各自写 Button，背景色一个没设（系统蓝）、一个写死 themeConfig.colors.primary（纯黑），
// 才出现「作品页和星图页加号颜色不一样」。统一到这里以后配色只有一份。
//
// Issue #888：恢复 API 26 原生沉浸光感材质。
//   API 26 且设备支持沉浸材质时：Button 本身直接挂 .systemMaterial()，
//   由系统提供材质层（模糊、高光、跟手光、阴影），不再用固定卡片背景和 1vp 边框。
//   不支持沉浸材质时：退回系统卡片底色 + 系统描边 token。
//   两个分支都是真实的 ButtonType.Circle 按钮，材质挂在 Button 上而非外层容器。
//
// 手势：按住时锁住底栏的横向滑动切页，避免按住加号横向拖动时页面被带走。
// 锁的实现见 app/navigation/PrimaryTabGestureLock.ets。

import { PRIMARY_FAB_SIZE_VP } from '../theme/LayoutTokens'
import { PrimaryTabGestureLock } from '../../app/navigation/PrimaryTabGestureLock'
import { HarmonyMaterialService } from '../../platform/material/HarmonyMaterialService'

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
  /** 设备是否支持沉浸式系统材质（API 26+ 且 isImmersiveMaterialSupported） */
  @Local materialSupported: boolean = false

  aboutToAppear() {
    this.materialSupported = HarmonyMaterialService.getInstance().floatingMaterial() !== undefined
  }

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
    if (this.materialSupported) {
      // API 26+ 沉浸材质分支：材质挂在 Button 上，stateEffect 关闭避免与材质按压反馈冲突
      Button({ type: ButtonType.Circle, stateEffect: false }) {
        SymbolGlyph($r('sys.symbol.plus'))
          .fontSize(28)
          .fontColor([$r('sys.color.icon_primary')])
      }
      .width(PRIMARY_FAB_SIZE_VP)
      .height(PRIMARY_FAB_SIZE_VP)
      .borderRadius(PRIMARY_FAB_SIZE_VP / 2)
      .systemMaterial(HarmonyMaterialService.getInstance().floatingMaterial())
      .onTouch((event: TouchEvent) => { this.handleTouch(event) })
      .onClick(() => { this.handleClick() })
    } else {
      // 不支持沉浸材质分支：系统卡片底色 + 系统描边 token
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
}
