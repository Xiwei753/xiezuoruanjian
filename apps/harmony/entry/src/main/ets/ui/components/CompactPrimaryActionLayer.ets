// CompactPrimaryActionLayer — 窄屏页面内容层右下角主操作（FAB）的统一承载与定位。
//
// Issue #932：作品页、星图列表、工作区窄屏各写了一份「内容 + 右下角 FAB」的定位
// （Stack BottomEnd + margin / RelativeContainer alignRules），位置算法重复且几何来源
// 不一致。这里提供唯一的窄屏内容层：
//   - 底层 contentBuilder 绘制页面内容；
//   - 右下角按 visible 绘制现有 PrimaryFab，位置只用 pagePadding + FAB 底部偏移。
// 本组件不决定业务动作、不做新建作品/卷/星图的逻辑，onTap 由页面传入。
// 按钮外观不在这里重画：PrimaryFab 已实现统一的毛玻璃 / 按压视效 / 反色描边。

import { PRIMARY_FAB_BOTTOM_INSET_VP } from '../theme/LayoutTokens'
import { PrimaryFab } from './PrimaryFab'

@ComponentV2
export struct CompactPrimaryActionLayer {
  /** 是否显示主操作 FAB（由页面按动作表与加载/错误状态决定） */
  @Param visible: boolean = false
  /** 页面水平内边距（vp），FAB 右边距与页面内容同口径（AdaptiveState.pagePadding） */
  @Param pagePadding: number = 0
  /** 主操作回调，由页面提供（新建作品 / 新建卷 / 新建星图） */
  @Param onTap: () => void = () => {}

  @Builder
  emptyContent() {}

  /** 页面内容；FAB 只叠加在内容之上，不参与页面自身布局 */
  @BuilderParam contentBuilder: () => void = this.emptyContent

  build() {
    Stack({ alignContent: Alignment.BottomEnd }) {
      this.contentBuilder()

      if (this.visible) {
        PrimaryFab({ onTap: this.onTap })
          .margin({
            right: this.pagePadding,
            bottom: PRIMARY_FAB_BOTTOM_INSET_VP
          })
      }
    }
    .width('100%')
    .height('100%')
  }
}
