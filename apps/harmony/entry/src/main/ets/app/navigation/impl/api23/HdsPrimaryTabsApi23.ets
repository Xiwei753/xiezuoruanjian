// HdsPrimaryTabsApi23 — API23+ 一级底部导航壳层，使用 HdsTabs 悬浮页签。
// Issue #768：API23+ 改用 UI Design Kit 的 HdsTabs 替代普通 Tabs + BottomTabBarStyle。
// 三个 tab 仍复用现有 home/starMap/stats 三个 NavPathStack，只替换平台 chrome，不重建业务导航状态。
//
// 与 PrimaryTabShell 的差异：
// - 使用 HdsTabs（悬浮页签）替代普通 Tabs
// - safeBottomVp 由外部传入（来自 AdaptiveState），不在组件内部订阅 AdaptiveContext
// - currentShowPrimaryNav 由外部传入，不在组件内部订阅 NavigationChromeStore / ScreenPolicyBridge
// - 主题/响应式观察者由外部管理，此组件只负责渲染

import { Routes } from '../../Routes'
import hilog from '@ohos.hilog'
import { HomeScreen } from '../../../feature/project/ui/HomeScreen'
import { WorkspaceScreen } from '../../../feature/project/ui/WorkspaceScreen'
import { WritingScreen } from '../../../feature/editor/ui/WritingScreen'
import { StarMapScreen } from '../../../feature/starmap/ui/StarMapScreen'
import { SettingsScreen } from '../../../feature/settings/ui/SettingsScreen'
import { StatsScreen } from '../../../feature/stats/ui/StatsScreen'
import { NavigationTransactionCoordinator } from '../../NavigationTransactionCoordinator'

// 一级导航 Tab 索引常量
const TAB_HOME: number = 0    // 作品
const TAB_STARMAP: number = 1 // 星图
const TAB_STATS: number = 2   // 统计

// 正常底栏高度（vp）
const NORMAL_BAR_HEIGHT: number = 56
// 隐藏底栏时的高度（vp）
const HIDDEN_BAR_HEIGHT: number = 0

// Issue #767：工厂函数，在字段初始化时创建已经带根路由的 NavPathStack。
function createStackWithRoot(rootRoute: string): NavPathStack {
  const stack: NavPathStack = new NavPathStack()
  stack.pushPath({ name: rootRoute })
  return stack
}

@Component
export struct HdsPrimaryTabsApi23 {
  // 每个 tab 各自持有自己的 NavPathStack，字段初始化时创建并带根路由。
  @State homeStack: NavPathStack = createStackWithRoot(Routes.Home)
  @State starMapStack: NavPathStack = createStackWithRoot(Routes.StarMap)
  @State statsStack: NavPathStack = createStackWithRoot(Routes.Stats)

  // 当前选中的 tab 索引（与 AppNavigation 通过 @Link 共享）
  @Link @Watch('onCurrentTabChanged') currentTab: number

  // 来自 AdaptiveState 的底部安全区域高度（vp），用于悬浮页签底部间距
  @Prop safeBottomVp: number = 0

  // 是否显示一级导航栏，由外部 ScreenPolicy 决定
  @Prop currentShowPrimaryNav: boolean = true

  // @Provide 活动栈，供 @Consume("navPathStack") 的页面使用。
  @Provide('navPathStack') activeStack: NavPathStack = this.homeStack

  aboutToAppear(): void {
    hilog.info(0x0000, 'HdsPrimaryTabsApi23', 'aboutToAppear: stacks initialized with root routes')

    // 向 coordinator 显式登记三个 tab 的 NavPathStack。
    const coordinator = NavigationTransactionCoordinator.getInstance()
    coordinator.registerTabStack(TAB_HOME, this.homeStack)
    coordinator.registerTabStack(TAB_STARMAP, this.starMapStack)
    coordinator.registerTabStack(TAB_STATS, this.statsStack)

    // 按真实 currentTab 同步活动栈。
    this.syncActiveTab(this.currentTab)

    // 注册完 homeStack 后尝试消费待处理的接续恢复。
    coordinator.tryConsumePendingRestore()
  }

  aboutToDisappear(): void {
    // 注销自己持有的三个栈，必须按实例相等再删。
    const coordinator = NavigationTransactionCoordinator.getInstance()
    coordinator.unregisterTabStack(TAB_HOME, this.homeStack)
    coordinator.unregisterTabStack(TAB_STARMAP, this.starMapStack)
    coordinator.unregisterTabStack(TAB_STATS, this.statsStack)
  }

  // syncActiveTab 是同步 activeStack 和 Coordinator 的唯一入口。
  private syncActiveTab(index: number): void {
    const stack = index === TAB_HOME
      ? this.homeStack
      : index === TAB_STARMAP
        ? this.starMapStack
        : this.statsStack

    this.activeStack = stack
    const coordinator = NavigationTransactionCoordinator.getInstance()
    coordinator.setActiveTab(index)
    coordinator.setActiveStack(stack)
    hilog.info(0x0000, 'HdsPrimaryTabsApi23', 'syncActiveTab: index=%{public}d, stack updated', index)
  }

  // @Watch 回调，currentTab 被父组件程序修改时触发同步。
  private onCurrentTabChanged(): void {
    this.syncActiveTab(this.currentTab)
  }

  // Tabs onChange 切换后只更新 currentTab，由 @Watch 统一触发 syncActiveTab。
  private onTabChange(index: number): void {
    this.currentTab = index
  }

  // 获取指定 tab 的 NavPathStack。
  getStackForTab(tabIndex: number): NavPathStack {
    if (tabIndex === TAB_HOME) {
      return this.homeStack
    } else if (tabIndex === TAB_STARMAP) {
      return this.starMapStack
    } else {
      return this.statsStack
    }
  }

  @Builder
  PageMap(name: string) {
    if (name === Routes.Home) {
      HomeScreen()
    } else if (name === Routes.Workspace) {
      WorkspaceScreen()
    } else if (name === Routes.Writing) {
      WritingScreen()
    } else if (name === Routes.StarMap) {
      StarMapScreen()
    } else if (name === Routes.Settings) {
      SettingsScreen()
    } else if (name === Routes.Stats) {
      StatsScreen()
    }
  }

  build() {
    // Issue #768：API23+ 使用 HdsTabs 悬浮页签替代普通 Tabs + BottomTabBarStyle。
    // HdsTabs 是 ArkUI 全局组件（API23+），不需要 import @kit.*。
    HdsTabs({ barPosition: BarPosition.End, index: this.currentTab }) {
      // 作品 tab（TAB_HOME=0）
      TabContent() {
        Navigation(this.homeStack) {
          // 内容由 homeStack 驱动
        }
        .navDestination(this.PageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
      }
      .tabBar('作品')

      // 星图 tab（TAB_STARMAP=1）
      TabContent() {
        Navigation(this.starMapStack) {
          // 内容由 starMapStack 驱动
        }
        .navDestination(this.PageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
      }
      .tabBar('星图')

      // 统计 tab（TAB_STATS=2）
      TabContent() {
        Navigation(this.statsStack) {
          // 内容由 statsStack 驱动
        }
        .navDestination(this.PageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
      }
      .tabBar('统计')
    }
    .barHeight(this.currentShowPrimaryNav ? NORMAL_BAR_HEIGHT : HIDDEN_BAR_HEIGHT)
    // 悬浮页签底部间距跟真实安全区走，不在页面里写死。
    .barMargin({ bottom: this.safeBottomVp })
    // 正文/设置页禁止横向滑动切 tab，防止绕过离开事务。
    .scrollable(this.currentShowPrimaryNav)
    .onContentWillChange((currentIndex: number, comingIndex: number): boolean => {
      if (!this.currentShowPrimaryNav) {
        return false
      }
      return true
    })
    .onChange((index: number) => {
      this.onTabChange(index)
    })
    .width('100%')
    .height('100%')
  }
}
