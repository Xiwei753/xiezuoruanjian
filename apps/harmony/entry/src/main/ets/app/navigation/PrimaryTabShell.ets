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
import { StarMapListScreen } from '../../feature/starmap/ui/StarMapListScreen'
import { SettingsScreen } from '../../feature/settings/ui/SettingsScreen'
import { SettingsOverlayHost } from '../../feature/settings/ui/SettingsOverlayHost'
import { StatsScreen } from '../../feature/stats/ui/StatsScreen'
import { NavigationTransactionCoordinator } from './NavigationTransactionCoordinator'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../ui/adaptive/AdaptiveContext'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../ui/theme/DesignTokens'
import { ScreenRole, WidthClass } from '../../corebridge/dto/PlatformDtos'
import { ScreenPolicyBridge } from '../../ui/adaptive/ScreenPolicyBridge'
import { NavigationChromeStore } from './NavigationChromeStore'
import { PrimaryTabGestureLock } from './PrimaryTabGestureLock'
import { PlatformApiResolver } from '../../platform/version/PlatformApiResolver'
import { HdsPrimaryTabsApi23 } from './impl/api23/HdsPrimaryTabsApi23'

// 一级导航 Tab 索引常量
const TAB_HOME: number = 0    // 作品
const TAB_STARMAP: number = 1 // 星图
const TAB_STATS: number = 2   // 统计

// 正常底栏高度（vp）
const NORMAL_BAR_HEIGHT: number = 56
// 隐藏底栏时的高度（vp）
const HIDDEN_BAR_HEIGHT: number = 0

// Issue #825：宽屏左侧一级导航项。和底部 Tabs 用同一组 tab 索引，
// 只决定“摆在哪里”，不新增第四套路由栈，也不新增 Core 枚举。
interface WideRailEntry {
  index: number
  label: string
}

const WIDE_RAIL_ENTRIES: WideRailEntry[] = [
  { index: TAB_HOME, label: '作品' },
  { index: TAB_STARMAP, label: '星图' },
  { index: TAB_STATS, label: '统计' }
]

// 宽屏左侧一级导航的固定宽度（vp）
const WIDE_RAIL_WIDTH: number = 200

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
  @State starMapStack: NavPathStack = createStackWithRoot(Routes.StarMapList)
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
  // Issue #774 评论5847638284 第2项：交互锁——异步查 policy 期间禁止切 tab，
  // 但不触发 @Watch 导致底栏无意义 hide→show。与视觉显示分开。
  @State currentPrimaryNavInteractive: boolean = true
  // 按住主操作悬浮按钮期间的手势锁。与上面的 policy 交互锁语义不同（那个是「能否离开当前页」，
  // 这个纯粹是「手指按在按钮上时别让 Tabs 抢走滑动」），两者取与后传给 Tabs，互不覆盖。
  @State primaryTabGestureLocked: boolean = false

  // Issue #768：API23+ 分流标志。true 时使用 HdsPrimaryTabsApi23，false 时使用普通 Tabs。
  @State useHdsTabs: boolean = false

  // Issue #767：@Provide 活动栈，供 @Consume("navPathStack") 的页面使用。
  // 初始值引用 homeStack（默认 tab 是作品，homeStack 在此之前已声明初始化）。
  // Tabs onChange 切换时更新为对应 tab 的 stack。
  @Provide('navPathStack') activeStack: NavPathStack = this.homeStack

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private navChromeObserver: ((role: ScreenRole) => void) | null = null
  private gestureLockObserver: ((locked: boolean) => void) | null = null

  aboutToAppear(): void {
    hilog.info(0x0000, 'PrimaryTabShell', 'aboutToAppear: stacks initialized with root routes')

    // Issue #768：检测 API Level，决定是否使用 HdsTabs。
    this.useHdsTabs = PlatformApiResolver.getInstance().isApiAtLeast(23)

    // Issue #768 评论5835171266 第1项：PrimaryTabShell 始终持有三个 NavPathStack 并注册到 coordinator，
    // 不再因 useHdsTabs 跳过——HdsPrimaryTabsApi23 不再自己创建 stack 或注册。
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

    // 订阅主操作按钮的手势锁：按住 FAB 时把一级 Tabs 的横向滑动关掉。
    this.gestureLockObserver = (locked: boolean) => {
      self.primaryTabGestureLocked = locked
    }
    PrimaryTabGestureLock.getInstance().subscribe(this.gestureLockObserver)
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
    if (this.gestureLockObserver) {
      PrimaryTabGestureLock.getInstance().unsubscribe(this.gestureLockObserver)
    }
    // Issue #768 评论5835171266 第1项：PrimaryTabShell 始终注销三个栈，不再因 useHdsTabs 跳过。
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
  // Issue #774 评论5847638284 第2项：只改交互锁，不改 currentShowPrimaryNav，
  // 避免 @Watch 触发 controller.applyHideAnimation/applyShowAnimation 导致正常切换也 hide→show。
  private async updateShowPrimaryNav(screenRole: ScreenRole): Promise<void> {
    this.currentPrimaryNavInteractive = false

    const requestedRole = screenRole
    const policy = await ScreenPolicyBridge.getInstance().resolveScreenPolicy(screenRole)
    // 异步返回后校验 role 是否已变化，防止旧页面结果覆盖新页面状态
    if (this.currentScreenRole !== requestedRole) {
      return
    }
    // Issue #767 评论5832378491：policy 解析失败时保持 fail-closed，
    // 不要 fail-open 把一级导航重新放开，否则 Writing/Settings 用户能横向切 tab 绕过离开事务。
    if (policy === null) {
      hilog.warn(0x0000, 'PrimaryTabShell',
        'resolveScreenPolicy failed for role=%{public}s; keep primary navigation disabled',
        String(requestedRole))
      this.currentShowPrimaryNav = false
      this.currentPrimaryNavInteractive = false
      return
    }
    this.currentShowPrimaryNav = policy.showPrimaryNavigation
    this.currentPrimaryNavInteractive = policy.showPrimaryNavigation
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
    // Issue #768 评论5835958606：切 tab 时设置 activeTab，由 NavigationChromeStore 发布该 tab 的 role。
    NavigationChromeStore.getInstance().setActiveTab(index)
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
    } else if (name === Routes.StarMapList) {
      StarMapListScreen({ navPathStack: ownerStack })
    } else if (name === Routes.StarMap) {
      StarMapScreen()
    } else if (name === Routes.Settings) {
      SettingsScreen({ navPathStack: ownerStack, screenTabIndex: ownerTabIndex })
    } else if (name === Routes.Stats) {
      StatsScreen()
    }
  }

  // ArkUI navDestination 契约：只接受「单参 @Builder」。#784 把 @Builder 包成箭头 lambda
  // 传给 navDestination，框架解析不到 builder（真机日志 "AceNavigation: Builder function is empty"
  // / "load page failed: StarMap"），destination 退化成空节点。
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

  // 宽屏：一级导航固定在左侧，作品 / 星图 / 统计纵向排列，选中态沿用现有系统 token。
  @Builder
  WidePrimaryRail() {
    Column({ space: 4 }) {
      Text('素笺')
        .fontSize(20)
        .fontWeight(FontWeight.Bold)
        .fontColor($r('sys.color.font_primary'))
        .width('100%')
        .textAlign(TextAlign.Start)
        .margin({ left: 12, bottom: 8 })

      Column()
        .width('100%')
        .height(1)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .margin({ bottom: 8 })

      ForEach(WIDE_RAIL_ENTRIES, (entry: WideRailEntry) => {
        this.WideRailItem(entry)
      }, (entry: WideRailEntry) => `${entry.index}`)

      Blank()
    }
    .width(WIDE_RAIL_WIDTH)
    .height('100%')
    .padding({ left: 12, right: 12, top: 16, bottom: 16 })
    .backgroundColor($r('sys.color.background_secondary'))
  }

  // 宽屏左侧一级导航单项：选中时加粗 + 左侧强调条，与底部 Tabs 保持同一套语义。
  @Builder
  WideRailItem(entry: WideRailEntry) {
    Row() {
      Column()
        .width(3)
        .height(20)
        .borderRadius(DesignTokens.radiusXs)
        .backgroundColor($r('sys.color.ohos_id_color_emphasize'))
        .opacity(this.currentTab === entry.index ? 1 : 0)
        .margin({ right: 12 })

      Text(entry.label)
        .fontSize(16)
        .fontWeight(this.currentTab === entry.index ? FontWeight.Bold : FontWeight.Normal)
        .fontColor($r('sys.color.font_primary'))
        .textAlign(TextAlign.Start)
        .layoutWeight(1)
    }
    .width('100%')
    .height(44)
    .padding({ left: 12, right: 12 })
    .alignItems(VerticalAlign.Center)
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor(this.currentTab === entry.index ? $r('sys.color.comp_background_gray') : Color.Transparent)
    .onClick(() => {
      this.currentTab = entry.index
    })
  }

  @Builder
  WideCurrentNavigation() {
    if (this.currentTab === TAB_HOME) {
      Navigation(this.homeStack) {}
        .navDestination(this.HomePageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
        .width('100%')
        .height('100%')
    } else if (this.currentTab === TAB_STARMAP) {
      Navigation(this.starMapStack) {}
        .navDestination(this.StarMapPageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
        .width('100%')
        .height('100%')
    } else {
      Navigation(this.statsStack) {}
        .navDestination(this.StatsPageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
        .width('100%')
        .height('100%')
    }
  }

  // 宽屏壳：一级导航在左，内容在右。写进 Writing 这类
  // showPrimaryNavigation=false 的页面时左栏整体消失，内容占满。
  @Builder
  WidePrimaryShell() {
    Row() {
      if (this.currentShowPrimaryNav) {
        this.WidePrimaryRail()
      }
      Stack() {
        this.WideCurrentNavigation()

        // Issue #825：宽屏设置悬浮在三个 tab 的当前页面之上，
        // 背景就是打开设置前的页面。关闭只隐藏悬浮层，
        // 下面的页面和原 NavPathStack 保持原位置。
        SettingsOverlayHost()
      }
      .layoutWeight(1)
      .height('100%')
    }
    .width('100%')
    .height('100%')
  }

  build() {
    // Issue #768：API23+ 分流到 HdsPrimaryTabsApi23（使用 HdsTabs 悬浮页签）。
    // API < 23 保持普通 Tabs + BottomTabBarStyle 实现。
    if (this.adaptiveState.sizeClass !== WidthClass.Compact) {
      // 平板/桌面宽屏先进入左侧导航骨架；具体视觉、收起态和设置浮层由后续议题填充。
      this.WidePrimaryShell()
    } else if (this.useHdsTabs) {
      HdsPrimaryTabsApi23({
        currentTab: $currentTab,
        homeStack: this.homeStack,
        starMapStack: this.starMapStack,
        statsStack: this.statsStack,
        safeBottomVp: this.adaptiveState.safeBottomVp,
        currentShowPrimaryNav: this.currentShowPrimaryNav,
        currentPrimaryNavInteractive: this.currentPrimaryNavInteractive && !this.primaryTabGestureLocked
      })
    } else {
      Tabs({ barPosition: BarPosition.End, index: this.currentTab }) {
        // 作品 tab（TAB_HOME=0）
        TabContent() {
          Navigation(this.homeStack) {
            // 内容由 homeStack 驱动
          }
          .navDestination(this.HomePageMap)
          .mode(NavigationMode.Stack)
          .hideTitleBar(true)
        }
        .tabBar(new BottomTabBarStyle($r('sys.media.ohos_ic_public_text'), '作品'))

        // 星图 tab（TAB_STARMAP=1）
        TabContent() {
          Navigation(this.starMapStack) {
            // 内容由 starMapStack 驱动
          }
          .navDestination(this.StarMapPageMap)
          .mode(NavigationMode.Stack)
          .hideTitleBar(true)
        }
        .tabBar(new BottomTabBarStyle($r('app.media.ic_star'), '星图'))

        // 统计 tab（TAB_STATS=2）
        TabContent() {
          Navigation(this.statsStack) {
            // 内容由 statsStack 驱动
          }
          .navDestination(this.StatsPageMap)
          .mode(NavigationMode.Stack)
          .hideTitleBar(true)
        }
        .tabBar(new BottomTabBarStyle($r('sys.media.ohos_ic_public_more'), '统计'))
      }
      .barHeight(this.currentShowPrimaryNav ? NORMAL_BAR_HEIGHT : HIDDEN_BAR_HEIGHT)
      // Issue #774 评论5847638284 第2项：scrollable 用交互锁而非视觉显示，
      // 避免 @Watch 触发底栏 hide→show 动画。
      // 再取与手势锁：手指按在主操作悬浮按钮上时，一级页签的横向滑动要停掉，
      // 否则按住 + 号左右拖动会把整页拖走（实机复现过）。
      .scrollable(this.currentPrimaryNavInteractive && !this.primaryTabGestureLocked)
      .onContentWillChange((currentIndex: number, comingIndex: number): boolean => {
        // Issue #774 评论5847638284 第2项：用交互锁判断，而非视觉显示。
        if (!this.currentPrimaryNavInteractive || this.primaryTabGestureLocked) {
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
}
