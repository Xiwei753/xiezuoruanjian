// HdsSettingsDestinationApi23 — API23+ 设置页 NavDestination 壳层。
// Issue #768 评论5835958606：把 HdsNavDestination + HdsNavigationTitleBarOptions
// 收进 impl/api23 组件，共享页面 SettingsScreen 不再直接 import @kit.UIDesignKit，
// 避免高版本模块引用进入所有版本都会加载的共享页面，破坏版本隔离。

import { HdsNavDestination, HdsNavigationTitleBarOptions } from '@kit.UIDesignKit'

@Component
export struct HdsSettingsDestinationApi23 {
  @BuilderParam contentBuilder: () => void
  title: string | Resource = ''
  onBack: (() => void) | null = null
  onShown: (() => void) | null = null

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
