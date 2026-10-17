// HdsSettingsDestinationApi23 — API23+ 设置页 NavDestination 壳层。
// Issue #768 评论5835958606：把 HdsNavDestination + HdsNavigationTitleBarOptions
// 收进 impl/api23 组件，共享页面 SettingsScreen 不再直接 import @kit.UIDesignKit，
// 避免高版本模块引用进入所有版本都会加载的共享页面，破坏版本隔离。
// Issue #773 评论5844518092：统一为 V2 @ComponentV2 + @Param，消除 V2 页面把 @Builder
// 交给 V1 壳的 @BuilderParam 后 this 变成壳组件导致 adaptiveState undefined 的 TypeError。
// Issue #932：设置页改用与 App/Workspace/Writing 同一份标题栏实现——
// titleBar.content.stackBuilder 挂 HeaderTitleBarStack，标题自绘、背板填页面底色，
// 材质与其余三处统一为 ADAPTIVE；不再用原生 content.title.mainTitle + IMMERSIVE。
// Issue #932 复核：官方 COMMON_BLUR 下背板默认即透明，透明配置不改变可见像素；
// 白条来自标题栏区域无承接层，现改由 originalStyle 填 sys.color.background_secondary 承接。

import { HdsNavDestination, HdsNavigationTitleBarOptions, hdsMaterial, ScrollEffectType } from '@kit.UIDesignKit'
import { HeaderTitleBarStack } from './HeaderTitleBarStack'

@ComponentV2
export struct HdsSettingsDestinationApi23 {
  @BuilderParam contentBuilder: () => void
  @Param title: string | Resource = ''
  @Param onBack: (() => void) | null = null
  @Param onShown: (() => void) | null = null

  // stackBuilder 是 CustomBuilder，组件调用必须写在 @Builder 里。
  // 直接在箭头函数体里写 HeaderTitleBarStack({...}) 会被当普通函数调用，运行时崩：
  // TypeError: class constructor cannot called without 'new'。
  @Builder
  headerTitleBar() {
    HeaderTitleBarStack({
      title: this.title,
      // 设置页标题栏没有尾部动作，传空数组：HeaderTitleBarStack 不再渲染空胶囊。
      headerActions: [],
      showBackButton: true
    })
  }

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
      content: {
        // 标题改由 stackBuilder 自绘，与其余三个壳同一观感；
        // mainTitle 传空串，返回按钮 label 置空（HDS 会把文案画进返回按钮的圆形里）。
        title: { mainTitle: '' },
        backIcon: { label: '' },
        stackBuilder: (): void => {
          this.headerTitleBar()
        }
      },
      style: {
        // Issue #932 复核：官方文档明确模糊样式类型（COMMON_BLUR / GRADUAL_BLUR）
        // 下，标题栏背板默认值本来就是透明色——原先这里写 Color.Transparent 属于 no-op，
        // 不改变任何可见像素，也去不掉白条；白条来自标题栏区域背后没有承接层、
        // 直接露出 HDS 自己的 surface。现改为把背板填成页面底色
        // （sys.color.background_secondary），让标题栏与页面同底，HDS surface 不再透出。
        // 仍然保留真正的 HDS 标题栏节点——自绘标题挂在
        // titleBar.content.stackBuilder 下，不隐藏标题栏、不把内容挪出标题栏树。
        // 历史真机结论（Issue #932）：backgroundColor 不能设在 HdsNavDestination 本体上，
        // 会被 HDS surface 覆盖；背板是标题栏自己的样式入口，不是给 NavDestination 设 backgroundColor。
        scrollEffectOpts: {
          enableScrollEffect: false,
          scrollEffectType: ScrollEffectType.COMMON_BLUR
        },
        originalStyle: {
          backgroundStyle: { backgroundColor: $r('sys.color.background_secondary') }
        },
        // 与 App/Workspace/Writing 三个壳统一为 ADAPTIVE（不再用 IMMERSIVE）。
        systemMaterialEffect: {
          materialType: hdsMaterial.MaterialType.ADAPTIVE,
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
