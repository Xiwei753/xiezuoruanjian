// CompactPrimaryActionLayer — 窄屏页面内容层右下角主操作（FAB）的统一承载与定位。
//
// Issue #932：作品页、星图列表、工作区窄屏各写了一份「内容 + 右下角 FAB」的定位
// （Stack BottomEnd + margin / RelativeContainer alignRules），位置算法重复且几何来源
// 不一致。这里提供唯一的窄屏内容层：
//   - 底层 contentBuilder 绘制页面内容；
//   - 右下角按 visible 绘制现有 PrimaryFab，位置只用 pagePadding + FAB 底部偏移。
// 本组件不决定业务动作、不做新建作品/卷/星图的逻辑，onTap 由页面传入。
// 按钮外观不在这里重画：PrimaryFab 已实现统一的毛玻璃 / 按压视效 / 圆环边缘高光。

import { PRIMARY_FAB_BOTTOM_INSET_VP, PRIMARY_FAB_SIZE_VP } from '../theme/LayoutTokens'
import { GripLayoutSide, GripLayoutSideStore } from '../adaptive/GripLayoutSide'
import { PrimaryFab } from './PrimaryFab'

const GRIP_SWITCH_DURATION_MS: number = 280

@ComponentV2
export struct CompactPrimaryActionLayer {
  /** 是否显示主操作 FAB（由页面按动作表与加载/错误状态决定） */
  @Param visible: boolean = false
  /** 页面水平内边距（vp），FAB 右边距与页面内容同口径（AdaptiveState.pagePadding） */
  @Param pagePadding: number = 0
  /** 主操作回调，由页面提供（新建作品 / 新建卷 / 新建星图） */
  @Param onTap: () => void = () => {}

  // Issue #986：智感握姿决定 FAB 停靠侧；默认右侧，双手/放下设备保持上一次明确握持侧。
  @Local private gripSide: GripLayoutSide = 'right'
  @Local private fabX: number = 0
  @Local private fabY: number = 0
  private containerWidthVp: number = 0
  private containerHeightVp: number = 0
  private gripSideInitialized: boolean = false
  private unsubscribeGrip: (() => void) | null = null

  private fabPositionX(side: GripLayoutSide): number {
    if (side === 'left') {
      return this.pagePadding
    }
    return Math.max(0, this.containerWidthVp - this.pagePadding - PRIMARY_FAB_SIZE_VP)
  }

  private updateContainerSize(area: Area): void {
    const width: number = Number(area.width)
    const height: number = Number(area.height)
    if (width === this.containerWidthVp && height === this.containerHeightVp) {
      return
    }
    this.containerWidthVp = width
    this.containerHeightVp = height
    this.fabX = this.fabPositionX(this.gripSide)
    this.fabY = Math.max(0, height - PRIMARY_FAB_BOTTOM_INSET_VP - PRIMARY_FAB_SIZE_VP)
  }

  @Monitor('pagePadding')
  onPagePaddingChanged(): void {
    this.fabX = this.fabPositionX(this.gripSide)
  }

  @Builder
  emptyContent() {}

  /** 页面内容；FAB 只叠加在内容之上，不参与页面自身布局 */
  @BuilderParam contentBuilder: () => void = this.emptyContent

  aboutToAppear(): void {
    this.unsubscribeGrip = GripLayoutSideStore.getInstance().subscribe((side: GripLayoutSide): void => {
      if (!this.gripSideInitialized) {
        // 首帧直接落到当前握持侧，避免页面刚出现时从默认右侧滑过来。
        this.gripSideInitialized = true
        this.gripSide = side
        this.fabX = this.fabPositionX(side)
        return
      }
      if (side === this.gripSide) {
        return
      }
      this.getUIContext().animateTo({
        duration: GRIP_SWITCH_DURATION_MS,
        curve: Curve.EaseInOut
      }, () => {
        this.gripSide = side
        this.fabX = this.fabPositionX(side)
      })
    })
  }

  aboutToDisappear(): void {
    if (this.unsubscribeGrip !== null) {
      this.unsubscribeGrip()
      this.unsubscribeGrip = null
    }
  }

  build() {
    Stack({ alignContent: Alignment.TopStart }) {
      this.contentBuilder()

      if (this.visible) {
        PrimaryFab({ onTap: this.onTap })
          .position({ x: this.fabX, y: this.fabY })
      }
    }
    .width('100%')
    .height('100%')
    .onAreaChange((oldArea: Area, newArea: Area) => { this.updateContainerSize(newArea) })
  }
}
