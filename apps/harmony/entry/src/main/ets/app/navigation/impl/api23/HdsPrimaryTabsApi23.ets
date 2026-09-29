// HdsPrimaryTabsApi23 — API23+ 一级底部导航壳层，使用 HdsTabs 悬浮页签。
// Issue #768：API23+ 改用 UI Design Kit 的 HdsTabs 替代普通 Tabs + BottomTabBarStyle。
// Issue #768 评论5835171266 第1项：不再自己创建 NavPathStack、不注册 coordinator、
// 不创建 @Link @Watch currentTab、不 @Provide activeStack。
// 三个 NavPathStack 由父组件 PrimaryTabShell 唯一持有并传入，currentTab 通过 @Link 双向绑定
// 回父组件（不加 @Watch，不写 coordinator），切 tab 时只有 PrimaryTabShell 的 @Watch 触发 syncActiveTab。
//
// Issue #768 评论5835171266 第3项：HdsTabs 加 barFloatingStyle + systemMaterialEffect(IMMERSIVE)
// 实现真正的悬浮沉浸光感效果；Navigation 改为 HdsNavigation。
//
// Issue #774 评论5847359998：单棵树改造——只保留一棵 HdsTabs，不再因底栏显隐重建整棵页面树。
// 底栏显隐通过 controller.applyHideAnimation/applyShowAnimation 控制，不销毁 TabContent。

import { Routes } from '../../Routes'
import hilog from '@ohos.hilog'
import { HomeScreen } from '../../../../feature/project/ui/HomeScreen'
import { WorkspaceScreen } from '../../../../feature/project/ui/WorkspaceScreen'
import { WritingScreen } from '../../../../feature/editor/ui/WritingScreen'
import { StarMapScreen } from '../../../../feature/starmap/ui/StarMapScreen'
import { SettingsScreen } from '../../../../feature/settings/ui/SettingsScreen'
import { StatsScreen } from '../../../../feature/stats/ui/StatsScreen'
import {
  HdsTabs,
  HdsNavigation,
  HdsTabsFloatingStyle,
  HdsTabsController,
  HdsAnimationMode,
  hdsMaterial,
} from '@kit.UIDesignKit'

// 一级导航 Tab 索引常量
const TAB_HOME: number = 0    // 作品
const TAB_STARMAP: number = 1 // 星图
const TAB_STATS: number = 2   // 统计

// Issue #774 评论5847359998：悬浮栏底部固定设计间距（vp）。
// 不再用 safeBottomVp（系统导航指示条避让高度），那是另一个语义。
const FLOATING_BAR_BOTTOM_MARGIN: number = 28

@Component
export struct HdsPrimaryTabsApi23 {
  // Issue #768 评论5835171266 第1项：三个 NavPathStack 由父组件 PrimaryTabShell 传入，
  // 本组件不创建自己的 stack，不注册 coordinator。
  // 使用普通变量传递（非 @Prop），因为 NavPathStack 是引用类型，@Prop 会深拷贝导致
  // 子组件操作的是副本而非原始 stack。普通变量传递引用，Navigation(stack) 直接使用同一实例。
  homeStack: NavPathStack = new NavPathStack()
  starMapStack: NavPathStack = new NavPathStack()
  statsStack: NavPathStack = new NavPathStack()

  // Issue #768 评论5835171266 第1项：保留 @Link currentTab（用于 HdsTabs 的 index 和 onChange 写入），
  // 但不加 @Watch、不调 syncActiveTab、不写 coordinator。
  // HdsTabs.onChange 写 this.currentTab = index → @Link 双向绑定到父组件 →
  // 父组件 PrimaryTabShell 的 @Watch 触发 syncActiveTab → 只有一处写 coordinator。
  @Link currentTab: number

  // 来自 AdaptiveState 的底部安全区域高度（vp）。
  // Issue #773 评论5844518092 第2项：不再用于 barBottomMargin（改用固定设计值 FLOATING_BAR_BOTTOM_MARGIN）；
  // safeBottomVp 保留是因为 PrimaryTabShell 仍传入，后续若无人消费可移除。
  @Prop safeBottomVp: number = 0

  // 是否显示一级导航栏，由外部 ScreenPolicy 决定。
  // Issue #774 评论5847359998：加 @Watch 回调，通过 controller.applyHideAnimation/applyShowAnimation
  // 控制底栏显隐，不再靠 if/else 两套 HdsTabs 重建整棵页面树。
  @Prop @Watch('onShowPrimaryNavChange') currentShowPrimaryNav: boolean = true

  // Issue #774 评论5847638284 第2项：交互锁——异步查 policy 期间禁止切 tab，
  // 但不触发 @Watch 导致底栏无意义 hide→show。与视觉显示分开。
  @Prop currentPrimaryNavInteractive: boolean = true

  // Issue #773 评论5844518092 第2项：长期持有的 HdsTabsController，创建 HdsTabs 时经 options 传入，
  // 修复真机日志 "GetHdsTabsController no hdsTabsControllerClass prop"。
  private controller: HdsTabsController = new HdsTabsController()

  // Issue #774 评论5847359998：@Watch 回调——currentShowPrimaryNav 变化时通过 controller 动画控制底栏显隐，
  // 不销毁 TabContent，避免 Settings 进入两次、Home 重新 appear、返回首页 FAB 乱跳。
  onShowPrimaryNavChange(): void {
    if (this.currentShowPrimaryNav) {
      this.controller.applyShowAnimation(HdsAnimationMode.CLICK_ANIMATION)
    } else {
      this.controller.applyHideAnimation(HdsAnimationMode.CLICK_ANIMATION)
    }
  }

  // Issue #784 评论5857397503 第3点：PageMapForStack 按 ownerStack 传入 navPathStack，
  // 不再写死 homeStack，修复从星图/统计进入 Settings 时操作错 NavPathStack 的问题。
  // Issue #784 评论5857856673 问题1：增加 ownerTabIndex 参数，SettingsScreen 用它上报正确的 tab。
  @Builder
  PageMapForStack(name: string, ownerStack: NavPathStack, ownerTabIndex: number) {
    if (name === Routes.Home) {
      // Issue #768 评论5838624438：星图快捷入口改为切换一级 tab，不再 push 进 homeStack。
      HomeScreen({
        onOpenStarMap: (): void => {
          this.currentTab = TAB_STARMAP
        }
      })
    } else if (name === Routes.Workspace) {
      WorkspaceScreen({ navPathStack: ownerStack })
    } else if (name === Routes.Writing) {
      WritingScreen({ navPathStack: ownerStack })
    } else if (name === Routes.StarMap) {
      StarMapScreen()
    } else if (name === Routes.Settings) {
      SettingsScreen({ navPathStack: ownerStack, screenTabIndex: ownerTabIndex })
    } else if (name === Routes.Stats) {
      StatsScreen()
    }
  }

  // ArkUI navDestination 契约：只接受「单参 @Builder」。#784 为了把 ownerStack / ownerTabIndex
  // 传进页面，把 @Builder 包成箭头 lambda 传给 navDestination，框架解析不到 builder
  // （真机日志 "AceNavigation: Builder function is empty" / "load page failed: StarMap"），
  // destination 退化成空节点，页面内容不渲染。
  // 这里为三个 stack 各保留一个单参 @Builder 作为 navDestination 入口，内部再委托给 PageMapForStack。
  @Builder
  HomePageMap(name: string) {
    this.PageMapForStack(name, this.homeStack, TAB_HOME)
  }

  @Builder
  StarMapPageMap(name: string) {
    this.PageMapForStack(name, this.starMapStack, TAB_STARMAP)
  }

  @Builder
  StatsPageMap(name: string) {
    this.PageMapForStack(name, this.statsStack, TAB_STATS)
  }

  // Issue #774 评论5847359998：三个 TabContent 提取为 @Builder，单棵树唯一复用。
  @Builder
  HdsTabContents() {
    // 作品 tab（TAB_HOME=0）— 保留图标 + 文字
    TabContent() {
      HdsNavigation(this.homeStack) {
        // 内容由 homeStack 驱动
      }
      .navDestination(this.HomePageMap)
      .mode(NavigationMode.Stack)
      .hideTitleBar(true)
    }
    .tabBar(new BottomTabBarStyle($r('sys.media.ohos_ic_public_text'), '作品'))

    // 星图 tab（TAB_STARMAP=1）— 保留图标 + 文字
    TabContent() {
      HdsNavigation(this.starMapStack) {
        // 内容由 starMapStack 驱动
      }
      .navDestination(this.StarMapPageMap)
      .mode(NavigationMode.Stack)
      .hideTitleBar(true)
    }
    .tabBar(new BottomTabBarStyle($r('app.media.ic_star'), '星图'))

    // 统计 tab（TAB_STATS=2）— 保留图标 + 文字
    TabContent() {
      HdsNavigation(this.statsStack) {
        // 内容由 statsStack 驱动
      }
      .navDestination(this.StatsPageMap)
      .mode(NavigationMode.Stack)
      .hideTitleBar(true)
    }
    .tabBar(new BottomTabBarStyle($r('sys.media.ohos_ic_public_more'), '统计'))
  }

  // Issue #774 评论5847359998：悬浮页签样式按 SDK d.ts 真实字段结构写，不用 as 强转。
  // barWidth 宽度调大匹配设计稿；补 gradientMask 实现沉浸光感。
  private buildFloatingStyle(): HdsTabsFloatingStyle {
    return {
      barWidth: {
        smallWidth: 280,
        mediumWidth: 400,
        largeWidth: 500,
      },
      barBottomMargin: FLOATING_BAR_BOTTOM_MARGIN,
      // 官方《设置页签栏的模糊样式》：gradientMask 就是手册里的"渐变模糊"，
      // maskColor 是"底部纯色、向上逐渐透明"的遮罩色，maskHeight 控制渐隐高度。
      // 官方 d.ts 确认 maskColor 只能取单色 ResourceColor，做不了渐变。
      // 之前把它改成 Color.Transparent 等于关掉渐变模糊，且像素实测毫无收益，已回退。
      gradientMask: {
        maskColor: $r('sys.color.background_secondary'),
        maskHeight: 32,
      },
      systemMaterialEffect: {
        materialType: hdsMaterial.MaterialType.IMMERSIVE,
        materialLevel: hdsMaterial.MaterialLevel.ADAPTIVE,
      }
    }
  }

  build() {
    // Issue #774 评论5847359998：单棵树——只保留一棵 HdsTabs，不再因底栏显隐重建整棵页面树。
    // 底栏显隐通过 @Watch 回调调 controller.applyHideAnimation/applyShowAnimation 控制。
    // 正常显示时不调 barHeight，让 HDS 自己决定悬浮栏高度。
    HdsTabs({
      index: this.currentTab,
      controller: this.controller,
      barPosition: BarPosition.End
    }) {
      this.HdsTabContents()
    }
    .barOverlap(true)
    .barPosition(BarPosition.End)
    .vertical(false)
    // Issue #794 评论5885532765：显式配置 animationDuration，确保每次点击底栏切换都播放页签切换动画。
    // BottomTabBarStyle 下 animationDuration 默认值为 0（无动画），不显式设置则点击切换无动画。
    // 300ms 是 ArkUI Tabs 非 BottomTabBarStyle 的默认动画时长，与官方 TabBar 动画示例一致。
    // 横向滑动的跟手动画由 interpolatingSpring 弹性曲线驱动，不受 animationDuration 影响。
    .animationDuration(300)
    // 官方《设置页签栏的模糊样式》给的"背景模糊"毛玻璃方案，官方定位是沉浸光感的
    // 轻量兜底：沉浸光感是对身后画面做材质折射，背后无内容就没内容可折；
    // 背景模糊是 BackdropBlur + 磨砂白，对任何背后像素都成立，所以观感更接近系统组件。
    // 前置条件 barOverlap(true) / barPosition(End) / vertical(false) 见上方三行，缺一则模糊不渲染。
    // 禁止再叠加原生 Tabs 的 barBackgroundColor / barBackgroundEffect / barBackgroundBlurStyle，
    // 那会顶掉 HdsTabs 自己的模糊渲染管线（其中 barBackgroundColor 只会留下固定 80vp 半径、颜色被忽略）。
    .barBackgroundBlurStyle(BlurStyle.Thin)
    .barFloatingStyle(this.buildFloatingStyle())
    // 正文/设置页禁止横向滑动切 tab，防止绕过离开事务。
    // Issue #774 评论5847638284 第2项：用交互锁而非视觉显示，避免 @Watch 触发底栏 hide→show。
    .scrollable(this.currentPrimaryNavInteractive)
    .onContentWillChange((currentIndex: number, comingIndex: number): boolean => {
      if (!this.currentPrimaryNavInteractive) {
        return false
      }
      return true
    })
    .onChange((index: number) => {
      // Issue #768 评论5835171266 第1项：onChange 只写 currentTab（@Link 双向绑定到父组件），
      // 父组件 PrimaryTabShell 的 @Watch 会触发 syncActiveTab，不再在此处调 syncActiveTab。
      this.currentTab = index
    })
    .width('100%')
    .height('100%')
  }
}
