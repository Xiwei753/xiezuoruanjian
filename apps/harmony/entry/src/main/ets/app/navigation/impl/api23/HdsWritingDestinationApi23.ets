// HdsWritingDestinationApi23 — API23+ 写作页 NavDestination 壳层。
// Issue #768 评论5835958606：把 HdsNavDestination + HdsNavigationTitleBarOptions
// 收进 impl/api23 组件，共享页面 WritingScreen 不再直接 import @kit.UIDesignKit，
// 避免高版本模块引用进入所有版本都会加载的共享页面，破坏版本隔离。

import { HdsNavDestination, HdsNavigationTitleBarOptions, hdsMaterial } from '@kit.UIDesignKit'

@ComponentV2
export struct HdsWritingDestinationApi23 {
  @BuilderParam contentBuilder: () => void
  @Param title: string | Resource = ''
  @Param onBack: (() => void) | null = null
  @Param onShown: (() => void) | null = null
  // Issue #778 任务8：接收父级传入的 scroller，绑定到 HdsNavDestination 做沉浸联动。
  @Param scroller: Scroller = new Scroller()

  build() {
    HdsNavDestination() {
      this.contentBuilder()
    }
    .hideTitleBar(false)
    .titleBar({
      enableComponentSafeArea: true,
      content: { title: { mainTitle: this.title } },
      style: {
        systemMaterialEffect: {
          materialType: hdsMaterial.MaterialType.IMMERSIVE,
          materialLevel: hdsMaterial.MaterialLevel.ADAPTIVE
        }
      }
    } as HdsNavigationTitleBarOptions)
    .bindToScrollable([this.scroller])
    .onBackPressed(() => {
      if (this.onBack) {
        this.onBack()
      }
      return true
    })
    .onShown(() => {
      if (this.onShown) {
        this.onShown()
      }
    })
  }
}
