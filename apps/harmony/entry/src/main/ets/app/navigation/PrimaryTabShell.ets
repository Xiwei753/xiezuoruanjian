// PrimaryTabShell — 一级底部 Tabs 壳层。
// Issue #767：用真正的横向 Tabs({ barPosition: BarPosition.End }) 替代手写底栏。
// 三个 TabContent 分别是作品（TAB_HOME=0）、星图（TAB_STARMAP=1）、统计（TAB_STATS=2）。
// 页签使用 BottomTabBarStyle，不再自己用 Row + Column + Image + Text 画底栏。
//
// 每个 Tab 各自持有自己的 NavPathStack 和 Navigation：
// - 作品栈：Home → Workspace → Writing / Settings
// - 星图栈：StarMap
// - 统计栈：Stats
//
// 一级切换只切 tab index，不再 clear() + pushPath() 重建根页面。
// Writing/Settings 需要隐藏一级底栏时，通过 ScreenPolicy.showPrimaryNavigation 控制 Tabs 的 barHeight 切到 0。
//
// Issue #767 修复 navPathStack render 期间修改问题：
// 用工厂函数在字段初始化时创建已经带根路由的 NavPathStack，
// 不等组件进入 render 生命周期后再改。

import { Routes, WorkspaceRouteParams, WritingRouteParams } from './Routes'
import hilog from '@ohos.hilog'
import { HomeScreen } from '../../feature/project/ui/HomeScreen'
import { WorkspaceScreen } from '../../feature/project/ui/WorkspaceScreen'
import { WritingScreen } from '../../feature/editor/ui/WritingScreen'
import { StarMapScreen } from '../../feature/starmap/ui/StarMapScreen'
import { SettingsScreen } from '../../feature/settings/ui/SettingsScreen'
import { StatsScreen } from '../../feature/stats/ui/StatsScreen'
import { NavigationTransactionCoordinator } from './NavigationTransactionCoordinator'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../ui/adaptive/AdaptiveContext'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../ui/theme/HarmonyThemeAdapter'
import { ScreenRole, WidthClass } from '../../corebridge/dto/PlatformDtos'
import { ScreenPolicyBridge } from '../../ui/adaptive/ScreenPolicyBridge'
import { NavigationChromeStore } from './NavigationChromeStore'

// 一级导航 Tab 索引常量
const TAB_HOME: number = 0    // 作品
const TAB_STARMAP: number = 1 // 星图
const TAB_STATS: number = 2   // 统计

// 正常底栏高度（vp）
const NORMAL_BAR_HEIGHT: number = 56
// 隐藏底栏时的高度（vp）
const HIDDEN_BAR_HEIGHT: number = 0

// Issue #767：工厂函数，在字段初始化时创建已经带根路由的 NavPathStack。
// 避免 aboutToAppear() 里 pushPath 导致 "State variable has changed during render" 错误。
function createStackWithRoot(rootRoute: string): NavPathStack {
  const stack: NavPathStack = new NavPathStack()
  stack.pushPath({ name: rootRoute })
  return stack
}

@Component
export struct PrimaryTabShell {
  // Issue #767：每个 tab 各自持有自己的 NavPathStack，字段初始化时创建并带根路由。
  // 不在 aboutToAppear 里 pushPath，避免 render 期间修改状态变量。
  @State homeStack: NavPathStack = createStackWithRoot(Routes.Home)
  @State starMapStack: NavPathStack = createStackWithRoot(Routes.StarMap)
  @State statsStack: NavPathStack = createStackWithRoot(Routes.Stats)

  // 当前选中的 tab 索引（与 AppNavigation 通过 @Link 共享）
  // Issue #767 评论5832129578 第1项：加 @Watch 监听，程序修改 currentTab 时也能触发 syncActiveTab。
  @Link @Watch('onCurrentTabChanged') currentTab: number

  // 响应式布局状态（用于判断是否显示底部 Tab Bar）
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  // 主题配置（底部 Tab 配色）
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  // 当前页面对应的 ScreenRole
  @State currentScreenRole: ScreenRole = ScreenRole.ProjectList
  // 是否显示底部一级导航栏，由 Core ScreenPolicy.showPrimaryNavigation 决定
  @State currentShowPrimaryNav: boolean = true

  // Issue #767：@Provide 活动栈，供 @Consume("navPathStack") 的页面使用。
  // 初始值引用 homeStack（默认 tab 是作品，homeStack 在此之前已声明初始化）。
  // Tabs onChange 切换时更新为对应 tab 的 stack。
  @Provide('navPathStack') activeStack: NavPathStack = this.homeStack

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private navChromeObserver: ((role: ScreenRole) => void) | null = null

  aboutToAppear(): void {
    hilog.info(0x0000, 'PrimaryTabShell', 'aboutToAppear: stacks initialized with root routes')

    // Issue #767：向 coordinator 显式登记三个 tab 的 NavPathStack。
    // 这样 AppNavigation 的接续恢复可以通过 getTabStack(TAB_HOME) 拿到正确的栈，
    // 而不是依赖可能尚未同步的 activeTabStack。
    const coordinator = NavigationTransactionCoordinator.getInstance()
    coordinator.registerTabStack(TAB_HOME, this.homeStack)
    coordinator.registerTabStack(TAB_STARMAP, this.starMapStack)
    coordinator.registerTabStack(TAB_STATS, this.statsStack)

    // Issue #767 评论5831949869 第2项：注册完三个栈后按真实 currentTab 同步，
    // 不要硬写 TAB_HOME——组件可能只是重新 appear、状态仍是 StarMap/Stats。
    // Issue #767 评论5832129578 第1项：aboutToAppear 时组件还在初始化，@Watch 不会触发，
    // 所以直接调 syncActiveTab 而不是 onTabChange。
    this.syncActiveTab(this.currentTab)

    // Issue #767：注册完 homeStack 后尝试消费待处理的接续恢复。
    // 如果 AppNavigation 已注册 restoreListener 且有 pendingRestore，此时才真正消费。
    coordinator.tryConsumePendingRestore()

    // 注册主题观察者，Tab 配色随主题变化。
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    // 注册响应式布局观察者，窗口尺寸变化时更新 adaptiveState。
    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)
    this.adaptiveState = AdaptiveContext.getInstance().getCurrentState()

    // 订阅 NavigationChromeStore，根据 ScreenRole 变化更新底栏显隐
    this.navChromeObserver = (role: ScreenRole) => {
      self.currentScreenRole = role
      self.updateShowPrimaryNav(role)
    }
    NavigationChromeStore.getInstance().subscribe(this.navChromeObserver!)
  }

  aboutToDisappear(): void {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
    if (this.navChromeObserver) {
      NavigationChromeStore.getInstance().unsubscribe(this.navChromeObserver)
    }
    // Issue #767 评论5831949869 第1项：注销自己持有的三个栈。
    // 必须按实例相等再删，避免旧 Shell 的迟到 disappear 把新 Shell 已注册的 stack 删掉。
    const coordinator = NavigationTransactionCoordinator.getInstance()
    coordinator.unregisterTabStack(TAB_HOME, this.homeStack)
    coordinator.unregisterTabStack(TAB_STARMAP, this.starMapStack)
    coordinator.unregisterTabStack(TAB_STATS, this.statsStack)
  }

  // 通过 Core ScreenPolicy.showPrimaryNavigation 决定底栏显隐。
  // Core 规则：Writing/Settings=false，Home/ProjectWorkspace/StarMap/Stats/Sync=true。
  // Issue #767 评论5832129578 第3项：role 变化时先禁用切 tab（保守策略），
  // 等 policy 返回后再决定是否开放；异步返回后校验 role 是否已变化，防止旧页面结果覆盖新页面状态。
  private async updateShowPrimaryNav(screenRole: ScreenRole): Promise<void> {
    this.currentShowPrimaryNav = false

    const requestedRole = screenRole
    const policy = await ScreenPolicyBridge.getInstance().resolveScreenPolicy(screenRole)
    // 异步返回后校验 role 是否已变化，防止旧页面结果覆盖新页面状态
    if (this.currentScreenRole !== requestedRole) {
      return
    }
    if (policy) {
      this.currentShowPrimaryNav = policy.showPrimaryNavigation
    } else {
      this.currentShowPrimaryNav = true
    }
  }

  // Issue #767 评论5832129578 第1项：syncActiveTab 是同步 activeStack 和 Coordinator 的唯一入口。
  // 无论是用户手势切 tab（Tabs.onChange → onTabChange → currentTab 赋值 → @Watch → syncActiveTab）
  // 还是程序修改 currentTab（如 applyContinuation 中 this.currentTab = TAB_HOME → @Watch → syncActiveTab），
  // 都走这个方法，确保 activeStack 和 Coordinator 状态一致。
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
    hilog.info(0x0000, 'PrimaryTabShell', 'syncActiveTab: index=%{public}d, stack updated', index)
  }

  // Issue #767 评论5832129578 第1项：@Watch 回调，currentTab 被父组件程序修改时触发同步。
  private onCurrentTabChanged(): void {
    this.syncActiveTab(this.currentTab)
  }

  // Issue #767：Tabs onChange 切换后只更新 currentTab，由 @Watch 统一触发 syncActiveTab。
  // 一级切换只切 tab index，不再 clear() + pushPath() 重建根页面。
  private onTabChange(index: number): void {
    this.currentTab = index
  }

  // Issue #767：获取指定 tab 的 NavPathStack。
  // 供 AppNavigation 的接续恢复逻辑使用。
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
    Tabs({ barPosition: BarPosition.End, index: this.currentTab }) {
      // 作品 tab（TAB_HOME=0）
      TabContent() {
        Navigation(this.homeStack) {
          // 内容由 homeStack 驱动
        }
        .navDestination(this.PageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
      }
      .tabBar(new BottomTabBarStyle($r('sys.media.ohos_ic_public_folder'), '作品'))

      // 星图 tab（TAB_STARMAP=1）
      TabContent() {
        Navigation(this.starMapStack) {
          // 内容由 starMapStack 驱动
        }
        .navDestination(this.PageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
      }
      .tabBar(new BottomTabBarStyle($r('app.media.ic_star'), '星图'))

      // 统计 tab（TAB_STATS=2）
      TabContent() {
        Navigation(this.statsStack) {
          // 内容由 statsStack 驱动
        }
        .navDestination(this.PageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
      }
      .tabBar(new BottomTabBarStyle($r('sys.media.ohos_ic_public_more'), '统计'))
    }
    .barHeight(this.currentShowPrimaryNav ? NORMAL_BAR_HEIGHT : HIDDEN_BAR_HEIGHT)
    // Issue #767 评论5832129578 第2项：正文/设置页禁止横向滑动切 tab，
    // 防止绕过 requestLeave/safePop 的离开事务。
    .scrollable(this.currentShowPrimaryNav)
    .onContentWillChange((currentIndex: number, comingIndex: number): boolean => {
      // Issue #767 评论5832129578 第2项：底栏隐藏时（Writing/Settings）禁止横向滑动切 tab。
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
