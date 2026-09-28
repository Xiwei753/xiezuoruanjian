// HdsSettingsDestinationApi23 — API23+ 设置页 NavDestination 壳层。
// Issue #768 评论5835958606：把 HdsNavDestination + HdsNavigationTitleBarOptions
// 收进 impl/api23 组件，共享页面 SettingsScreen 不再直接 import @kit.UIDesignKit，
// 避免高版本模块引用进入所有版本都会加载的共享页面，破坏版本隔离。
// Issue #773 评论5844518092：统一为 V2 @ComponentV2 + @Param，消除 V2 页面把 @Builder
// 交给 V1 壳的 @BuilderParam 后 this 变成壳组件导致 adaptiveState undefined 的 TypeError。

import { HdsNavDestination, HdsNavigationTitleBarOptions, hdsMaterial } from '@kit.UIDesignKit'

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
    .titleBar({
      enableComponentSafeArea: true,
      // 窗口开了 setWindowLayoutFullScreen 之后，标题栏必须显式继续避让顶部状态栏，
      // 否则「素笺」这类标题会被压到 18px 高（实测 [200,157][325,230] → [200,106][325,124]）。
      // 本文件只在 API >= 23 渲染（见各 Screen 的 useHdsNav 判定），该字段 API 20+ 恒可用。
      avoidLayoutSafeArea: true,
      content: { title: { mainTitle: this.title } },
      style: {
        systemMaterialEffect: {
          materialType: hdsMaterial.MaterialType.IMMERSIVE,
          materialLevel: hdsMaterial.MaterialLevel.ADAPTIVE
        }
      }
    } as HdsNavigationTitleBarOptions)
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
