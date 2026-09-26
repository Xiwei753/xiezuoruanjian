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

  @Builder
  PageMap(name: string) {
    if (name === Routes.Home) {
      // Issue #768 评论5838624438：星图快捷入口改为切换一级 tab，不再 push 进 homeStack。
      HomeScreen({
        onOpenStarMap: (): void => {
          this.currentTab = TAB_STARMAP
        }
      })
    } else if (name === Routes.Workspace) {
      // Issue #768 评论5838296806：显式传 homeStack，不再依赖全局 activeTabStack
      WorkspaceScreen({ navPathStack: this.homeStack })
    } else if (name === Routes.Writing) {
      WritingScreen()
    } else if (name === Routes.StarMap) {
      StarMapScreen()
    } else if (name === Routes.Settings) {
      // Issue #768 评论5838296806：显式传 homeStack，不再依赖全局 activeTabStack
      SettingsScreen({ navPathStack: this.homeStack })
    } else if (name === Routes.Stats) {
      StatsScreen()
    }
  }

  // Issue #774 评论5847359998：三个 TabContent 提取为 @Builder，单棵树唯一复用。
  @Builder
  HdsTabContents() {
    // 作品 tab（TAB_HOME=0）— 保留图标 + 文字
    TabContent() {
      HdsNavigation(this.homeStack) {
        // 内容由 homeStack 驱动
      }
      .navDestination(this.PageMap)
      .mode(NavigationMode.Stack)
      .hideTitleBar(true)
    }
    .tabBar(new BottomTabBarStyle($r('sys.media.ohos_ic_public_text'), '作品'))

    // 星图 tab（TAB_STARMAP=1）— 保留图标 + 文字
    TabContent() {
      HdsNavigation(this.starMapStack) {
        // 内容由 starMapStack 驱动
      }
      .navDestination(this.PageMap)
      .mode(NavigationMode.Stack)
      .hideTitleBar(true)
    }
    .tabBar(new BottomTabBarStyle($r('app.media.ic_star'), '星图'))

    // 统计 tab（TAB_STATS=2）— 保留图标 + 文字
    TabContent() {
      HdsNavigation(this.statsStack) {
        // 内容由 statsStack 驱动
      }
      .navDestination(this.PageMap)
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
    .barFloatingStyle(this.buildFloatingStyle())
    // 正文/设置页禁止横向滑动切 tab，防止绕过离开事务。
    .scrollable(this.currentShowPrimaryNav)
    .onContentWillChange((currentIndex: number, comingIndex: number): boolean => {
      if (!this.currentShowPrimaryNav) {
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
