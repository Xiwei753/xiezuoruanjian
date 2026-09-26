// HdsSettingsDestinationApi23 — API23+ 设置页 NavDestination 壳层。
// Issue #768 评论5835958606：把 HdsNavDestination + HdsNavigationTitleBarOptions
// 收进 impl/api23 组件，共享页面 SettingsScreen 不再直接 import @kit.UIDesignKit，
// 避免高版本模块引用进入所有版本都会加载的共享页面，破坏版本隔离。
// Issue #773 评论5844518092：统一为 V2 @ComponentV2 + @Param，消除 V2 页面把 @Builder
// 交给 V1 壳的 @BuilderParam 后 this 变成壳组件导致 adaptiveState undefined 的 TypeError。

import { HdsNavDestination, HdsNavigationTitleBarOptions } from '@kit.UIDesignKit'

@ComponentV2
export struct HdsSettingsDestinationApi23 {
  @BuilderParam contentBuilder: () => void
  @Param title: string | Resource = ''
  @Param onBack: (() => void) | null = null
  @Param onShown: (() => void) | null = null

  build() {
    HdsNavDestination() {
      this.contentBuilder()
    }
    .hideTitleBar(false)
    .titleBar({ content: { title: { mainTitle: this.title } } } as HdsNavigationTitleBarOptions)
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
