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
// Writing/Settings 需要隐藏一级底栏时，通过平台动作表 resolveShowPrimaryNavigation 控制 Tabs 的 barHeight 切到 0。
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
import { SettingsOverlayController } from '../../feature/settings/presentation/SettingsOverlayController'
import { StatsScreen } from '../../feature/stats/ui/StatsScreen'
import { NavigationTransactionCoordinator } from './NavigationTransactionCoordinator'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../ui/adaptive/AdaptiveContext'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../ui/theme/HarmonyThemeAdapter'
import { ActionRole, ActionSlot, ScreenRole, resolveScreenPolicy, resolveShowPrimaryNavigation } from './ScreenSemantics'
import { NavigationChromeStore } from './NavigationChromeStore'
import { PrimaryTabGestureLock } from './PrimaryTabGestureLock'
import { PlatformApiResolver } from '../../platform/version/PlatformApiResolver'
import { HdsPrimaryTabsApi23 } from './impl/api23/HdsPrimaryTabsApi23'
import { WideAppShell } from './wide/WideAppShell'
import { WideLayoutMode, resolveWideLayoutMode } from './wide/WideLayoutMode'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../../platform/diagnostics/HarmonyDiagnosticEvents'
import { SyncRunPresentationState, SyncRunSnapshot } from '../../feature/settings/presentation/SyncRunPresentationState'
import { SyncOutcomePresenter } from '../../feature/settings/presentation/SyncOutcomePresenter'
import { SyncStatusStrip, isSyncOutcomeConflict } from '../../ui/components/SyncStatusStrip'
import { NavigationHeaderActionDispatcher } from './NavigationHeaderActionDispatcher'

// 一级导航 Tab 索引常量
const TAB_HOME: number = 0    // 作品
const TAB_STARMAP: number = 1 // 星图
const TAB_STATS: number = 2   // 统计

// 正常底栏高度（vp）
const NORMAL_BAR_HEIGHT: number = 56
// 隐藏底栏时的高度（vp）
const HIDDEN_BAR_HEIGHT: number = 0

const SYNC_STATUS_SUCCESS_VISIBLE_MS: number = 3200
const SYNC_STATUS_MAX_WIDTH_VP: number = 560

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
  // 外壳形状只看这一个原始宽度值；用基础类型单独存一份，避免对象状态在首帧/重建时不同步。
  @State shellWidthVp: number = getAdaptiveState().widthVp
  // 主题配置（底部 Tab 配色）
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  // 当前页面对应的 ScreenRole
  @State currentScreenRole: ScreenRole = ScreenRole.ProjectList
  // 是否显示底部一级导航栏，由平台动作表 resolveShowPrimaryNavigation 决定
  @State currentShowPrimaryNav: boolean = true
  // Issue #774 评论5847638284 第2项：交互锁——解析页面动作期间禁止切 tab，
  // 但不触发 @Watch 导致底栏无意义 hide→show。与视觉显示分开。
  @State currentPrimaryNavInteractive: boolean = true
  // 按住主操作悬浮按钮期间的手势锁。与上面的 policy 交互锁语义不同（那个是「能否离开当前页」，
  // 这个纯粹是「手指按在按钮上时别让 Tabs 抢走滑动」），两者取与后传给 Tabs，互不覆盖。
  @State primaryTabGestureLocked: boolean = false

  // Issue #940：唯一应用壳层宿主负责呈现跨路由同步状态与手动入口 Toast。
  // Issue #952：只呈现用户手动发起的同步；自动同步照常运行并记录，但不弹状态条 / Toast。
  @State syncRunSnapshot: SyncRunSnapshot = SyncRunPresentationState.getInstance().snapshot()
  @State syncStatusVisible: boolean =
    this.syncRunSnapshot.source === 'manual' && this.syncRunSnapshot.phase !== 'idle'

  // Issue #768：API23+ 分流标志。true 时使用 HdsPrimaryTabsApi23，false 时使用普通 Tabs。
  @State useHdsTabs: boolean = false

  // Issue #827 评论 1 第 6 点：宽屏顶部固定动作条的动作槽，来源是平台动作表。
  @State wideHeaderActions: ActionSlot[] = []

  // Issue #767：@Provide 活动栈，供 @Consume("navPathStack") 的页面使用。
  // 初始值引用 homeStack（默认 tab 是作品，homeStack 在此之前已声明初始化）。
  // Tabs onChange 切换时更新为对应 tab 的 stack。
  @Provide('navPathStack') activeStack: NavPathStack = this.homeStack

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private navChromeObserver: ((role: ScreenRole) => void) | null = null
  private gestureLockObserver: ((locked: boolean) => void) | null = null
  private syncRunUnsubscribe: (() => void) | null = null
  private syncStatusHideTimerId: number = -1
  // Issue #968：记录最近一次已上报的导航布局判定，避免 build 每帧写诊断。
  private lastReportedLayoutMode: string = ''

  aboutToAppear(): void {
    hilog.info(0x0000, 'PrimaryTabShell', 'aboutToAppear: stacks initialized with root routes')

    // Issue #768：检测 API Level，决定是否使用 HdsTabs。
    this.useHdsTabs = PlatformApiResolver.getInstance().isApiAtLeast(23)

    // Issue #825 复核第2项：宽屏设置浮层改成窗口层级 custom dialog，
    // openCustomDialog 必须在 UIContext 上调用，而页头动作可能来自任何 NavDestination，
    // 所以这里在壳层注册一次窗口级 UIContext，不在每个页面各传一遍。
    SettingsOverlayController.getInstance().attachUIContext(this.getUIContext())
    // Issue #952：顶栏同步入口在与后台自动同步重叠时需要自行弹提示与结果 Toast，
    // 同样由壳层注册窗口级 UIContext（与 SettingsOverlayController 同一模式）。
    NavigationHeaderActionDispatcher.getInstance().attachUIContext(this.getUIContext())

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
      self.shellWidthVp = state.widthVp
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)
    this.adaptiveState = AdaptiveContext.getInstance().getCurrentState()
    this.shellWidthVp = this.adaptiveState.widthVp

    // 订阅 NavigationChromeStore，根据 ScreenRole 变化更新底栏显隐
    this.navChromeObserver = (role: ScreenRole) => {
      self.currentScreenRole = role
      self.updateShowPrimaryNav(role)
      // Issue #827 评论 1 第 6 点：宽屏顶部固定动作条跟着当前 ScreenRole 换动作槽。
      self.loadWideHeaderActions(role)
    }
    NavigationChromeStore.getInstance().subscribe(this.navChromeObserver!)
    // 初始 role 也要解析一次，否则宽屏顶部动作条在首帧是空的。
    this.loadWideHeaderActions(this.currentScreenRole)

    // 订阅主操作按钮的手势锁：按住 FAB 时把一级 Tabs 的横向滑动关掉。
    this.gestureLockObserver = (locked: boolean) => {
      self.primaryTabGestureLocked = locked
    }
    PrimaryTabGestureLock.getInstance().subscribe(this.gestureLockObserver)

    const syncState: SyncRunPresentationState = SyncRunPresentationState.getInstance()
    this.syncRunSnapshot = syncState.snapshot()
    // Issue #952：恢复快照时只恢复手动发起的可见提示；自动同步结果留在状态里被动可查。
    this.syncStatusVisible =
      this.syncRunSnapshot.source === 'manual' && this.syncRunSnapshot.phase !== 'idle'
    if (this.syncRunSnapshot.source === 'manual' && this.isPureSuccessfulSyncSnapshot(this.syncRunSnapshot)) {
      this.scheduleSyncStatusSuccessHide(this.syncRunSnapshot.finishedAtMs)
    }
    this.syncRunUnsubscribe = syncState.subscribe((snapshot: SyncRunSnapshot): void => {
      const previous: SyncRunSnapshot = self.syncRunSnapshot
      self.syncRunSnapshot = snapshot
      self.onSyncRunSnapshotChanged(previous, snapshot)
    })
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
    if (this.syncRunUnsubscribe !== null) {
      this.syncRunUnsubscribe()
      this.syncRunUnsubscribe = null
    }
    this.clearSyncStatusHideTimer()
    // Issue #768 评论5835171266 第1项：PrimaryTabShell 始终注销三个栈，不再因 useHdsTabs 跳过。
    // Issue #767 评论5831949869 第1项：注销自己持有的三个栈。
    // 必须按实例相等再删，避免旧 Shell 的迟到 disappear 把新 Shell 已注册的 stack 删掉。
    const coordinator = NavigationTransactionCoordinator.getInstance()
    coordinator.unregisterTabStack(TAB_HOME, this.homeStack)
    coordinator.unregisterTabStack(TAB_STARMAP, this.starMapStack)
    coordinator.unregisterTabStack(TAB_STATS, this.statsStack)
    // Issue #825 复核第3项：壳层销毁时如果还持有当前窗口的设置浮层，先关掉再摘 UIContext，
    // 否则 controller 会攥着一个已经失效的 UIContext，下个窗口 open() 直接失败。
    SettingsOverlayController.getInstance().detachUIContext(this.getUIContext())
    // Issue #952：同步入口的 UIContext 同样按窗口生命周期摘除。
    NavigationHeaderActionDispatcher.getInstance().detachUIContext(this.getUIContext())
  }

  private onSyncRunSnapshotChanged(previous: SyncRunSnapshot, next: SyncRunSnapshot): void {
    // Issue #952：自动同步全程静默——不显示状态条、不弹 Toast。
    // 过程与终态仍由 SyncRunPresentationState 与 Core 保存，设置页同步分类被动可查。
    if (next.source === 'auto') {
      this.clearSyncStatusHideTimer()
      this.syncStatusVisible = false
      return
    }

    if (next.phase === 'in_progress') {
      this.clearSyncStatusHideTimer()
      this.syncStatusVisible = true
    } else if (next.phase === 'success') {
      this.syncStatusVisible = true
      if (this.isPureSuccessfulSyncSnapshot(next)) {
        this.scheduleSyncStatusSuccessHide(next.finishedAtMs)
      } else {
        this.clearSyncStatusHideTimer()
      }
    } else if (next.phase === 'failed') {
      this.clearSyncStatusHideTimer()
      this.syncStatusVisible = true
    } else {
      this.clearSyncStatusHideTimer()
      this.syncStatusVisible = false
    }

    if (next.phase === 'in_progress' && previous.phase !== 'in_progress') {
      this.showSyncToast('同步开始…')
    }
    if (previous.phase === 'in_progress' && next.finishedAtMs !== null && next.outcome !== null) {
      const context: Context = getContext(this)
      this.showSyncToast(SyncOutcomePresenter.present(context, next.outcome))
    }
  }

  private scheduleSyncStatusSuccessHide(finishedAtMs: number | null): void {
    this.clearSyncStatusHideTimer()
    const expectedFinishedAtMs: number | null = finishedAtMs
    this.syncStatusHideTimerId = setTimeout((): void => {
      this.syncStatusHideTimerId = -1
      if (this.isPureSuccessfulSyncSnapshot(this.syncRunSnapshot) &&
        this.syncRunSnapshot.finishedAtMs === expectedFinishedAtMs) {
        this.syncStatusVisible = false
      }
    }, SYNC_STATUS_SUCCESS_VISIBLE_MS)
  }

  private clearSyncStatusHideTimer(): void {
    if (this.syncStatusHideTimerId !== -1) {
      clearTimeout(this.syncStatusHideTimerId)
      this.syncStatusHideTimerId = -1
    }
  }

  private dismissSyncStatus(): void {
    if (this.syncRunSnapshot.phase === 'failed' || isSyncOutcomeConflict(this.syncRunSnapshot.outcome)) {
      this.syncStatusVisible = false
    }
  }

  private isPureSuccessfulSyncSnapshot(snapshot: SyncRunSnapshot): boolean {
    if (snapshot.phase !== 'success' || isSyncOutcomeConflict(snapshot.outcome)) {
      return false
    }
    const outcome = snapshot.outcome
    if (outcome === null) {
      return true
    }
    const status: string = outcome.overallStatus ?? ''
    if (status.length === 0) {
      return outcome.errorCode === undefined && outcome.errorCategory === undefined
    }
    return status === 'success' || status === 'no_changes' || status === 'latest_wins_applied'
  }

  private shouldShowSyncStatus(): boolean {
    // Issue #952：状态条只响应用户手动发起的同步，作为渲染的唯一判定，
    // 避免异步状态改变时旧条复活。
    return this.syncStatusVisible &&
      this.syncRunSnapshot.source === 'manual' &&
      this.syncRunSnapshot.phase !== 'idle'
  }

  @Builder
  SyncStatusBanner() {
    if (this.shouldShowSyncStatus()) {
      Row() {
        SyncStatusStrip({
          snapshot: this.syncRunSnapshot,
          onOpenDetails: (): void => { this.openSyncSettings() },
          onDismiss: (): void => { this.dismissSyncStatus() }
        })
          .width('100%')
          .constraintSize({ maxWidth: SYNC_STATUS_MAX_WIDTH_VP })
      }
      .width('100%')
      .padding({ top: Math.max(this.adaptiveState.safeTopVp, 8), left: 12, right: 12, bottom: 8 })
      .alignItems(VerticalAlign.Center)
    }
  }

  private showSyncToast(message: string): void {
    try {
      // Toast 绑定当前壳层 UIContext，避免从脱离页面的异步回调猜窗口。
      this.getUIContext().getPromptAction().showToast({ message: message })
    } catch (_) {
      // 页面切换/销毁边界不影响 Core 同步结果和状态条。
    }
  }

  private openSyncSettings(): void {
    NavigationHeaderActionDispatcher.getInstance().dispatchAction(ActionRole.Settings)
  }

  // 按平台动作表决定底栏显隐。
  // 规则：Writing/Settings/StarMapDetail=false，Home/ProjectList/ProjectWorkspace/StarMap/Stats/Sync=true。
  // Issue #920 D：动作表已收回平台 UI 层（ScreenSemantics），同步解析，无异步竞态。
  private updateShowPrimaryNav(screenRole: ScreenRole): void {
    const showPrimaryNav: boolean = resolveShowPrimaryNavigation(screenRole)
    this.currentShowPrimaryNav = showPrimaryNav
    this.currentPrimaryNavInteractive = showPrimaryNav
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
  // Issue #906：onChange 只管真实状态提交，不在这里记用户意图——程序化恢复（applyContinuation 等）
  // 也会触发 onChange，记成 User 会把恢复伪装成点击。用户意图由 onTabBarClick 记 nav.tab_requested。
  private onTabChange(index: number): void {
    this.currentTab = index
  }

  // Issue #906：只记用户 Tab 意图（nav.tab_requested，User），不提交状态。
  // 真正的切换由 onChange 写 currentTab → @Watch → syncActiveTab 完成，
  // 避免在点击回调里强行改 index 绕过 onContentWillChange 交互锁。
  private recordTabIntent(index: number): void {
    const fields: DiagnosticField[] = [{ key: 'targetTab', value: String(index) }]
    HarmonyDiagnosticEvents.record('Info', 'User', 'nav.tab_requested', 'PrimaryTabShell', fields)
  }

  // Issue #906：明确的用户切换入口（宽屏左栏点击、作品页「打开星图」）。
  // 先记用户意图，再提交 currentTab；@Watch 只同步实际状态，nav.tab_changed 归为 App。
  private recordUserTabRequest(index: number): void {
    this.recordTabIntent(index)
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
          // Issue #906：作品页「打开星图」是明确的用户入口，走统一的用户 Tab 请求记录。
          this.recordUserTabRequest(TAB_STARMAP)
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

  // Issue #972：宽屏外壳只由根窗口实际宽度按 840vp 断点解析一次。
  // 宽屏只实例化 WideAppShell，窄屏只实例化 CompactPrimaryShell，两者不会同时挂载。
  private get wideLayoutMode(): WideLayoutMode {
    return resolveWideLayoutMode(this.shellWidthVp)
  }

  private get usesWideNavigation(): boolean {
    return this.wideLayoutMode === WideLayoutMode.Wide
  }

  // Issue #968：诊断只记录实际挂载的外壳根容器，而不是根据布尔值预计会挂哪一个。
  // 宽屏 Row 和窄屏 HdsTabs / Tabs 的 onAppear 各自调用这里，日志与同一份 widthVp 对照。
  private recordMountedShell(shell: string): void {
    const signature: string = `${this.adaptiveState.widthVp}x${this.adaptiveState.heightVp}:${this.adaptiveState.prefersWideLayout}:${shell}`
    if (signature === this.lastReportedLayoutMode) {
      return
    }
    this.lastReportedLayoutMode = signature
    const fields: DiagnosticField[] = [
      { key: 'widthVp', value: String(this.adaptiveState.widthVp) },
      { key: 'heightVp', value: String(this.adaptiveState.heightVp) },
      { key: 'prefersWideLayout', value: String(this.adaptiveState.prefersWideLayout) },
      { key: 'mountedShell', value: shell },
      { key: 'currentTab', value: String(this.currentTab) }
    ]
    HarmonyDiagnosticEvents.record('Info', 'App', 'layout.primary_shell', 'PrimaryTabShell', fields)
  }

  // 宽屏顶部固定动作区（同步 / 搜索 / 设置）的动作来源是平台动作表，
  // 不在壳层硬编码三个按钮，也不新增第四套枚举。
  private loadWideHeaderActions(role: ScreenRole): void {
    this.wideHeaderActions = resolveScreenPolicy(role).actionSlots
  }

  // 宽屏页面画布：仍然只承载现有的三个 NavPathStack。
  // 一级导航位置变了，栈、路由、事务协调器都不变；当前 tab 只实例化自己的 Navigation。
  @Builder
  WidePageCanvas() {
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

  // Issue #972：宽屏唯一外壳入口。左栏 / 顶部全局动作条 / 页面画布由 WideAppShell 真实布局承载，
  // 不再在 PrimaryTabShell 里画旧 rail 或退出 HDS 标题栏的 HeaderActionCapsule。
  @Builder
  WideShell() {
    WideAppShell({
      currentTab: this.currentTab,
      layoutMode: this.wideLayoutMode,
      showPrimaryNavigation: this.currentShowPrimaryNav,
      headerActions: this.wideHeaderActions,
      safeTopVp: this.adaptiveState.safeTopVp,
      showSyncStatus: this.currentShowPrimaryNav && this.shouldShowSyncStatus(),
      syncSnapshot: this.syncRunSnapshot,
      onTabRequest: (index: number): void => {
        this.recordUserTabRequest(index)
      },
      onOpenSyncDetails: (): void => {
        this.openSyncSettings()
      },
      onDismissSyncStatus: (): void => {
        this.dismissSyncStatus()
      },
      onMounted: (): void => {
        this.recordMountedShell('WideAppShell')
      }
    }) {
      this.WidePageCanvas()
    }
  }

  // Issue #932：窄屏外壳——API23+ 用 HdsPrimaryTabsApi23（沉浸光感底栏），低版本用普通 Tabs。
  // 两者合成唯一的窄屏入口，不再和宽屏外壳并列成第三套中屏 chrome。
  @Builder
  CompactPrimaryShell() {
    if (this.useHdsTabs) {
      HdsPrimaryTabsApi23({
        currentTab: $currentTab,
        homeStack: this.homeStack,
        starMapStack: this.starMapStack,
        statsStack: this.statsStack,
        safeBottomVp: this.adaptiveState.safeBottomVp,
        currentShowPrimaryNav: this.currentShowPrimaryNav,
        currentPrimaryNavInteractive: this.currentPrimaryNavInteractive && !this.primaryTabGestureLocked,
        // Issue #968：实际挂载的 HdsTabs 根容器回报窄屏外壳。
        onMounted: (): void => {
          this.recordMountedShell('CompactPrimaryShell/HdsTabs')
        },
        // Issue #906：HdsTabs 的用户点击入口，转发到只记用户意图的方法（不改 currentTab）。
        onUserTabClick: (index: number): void => {
          this.recordTabIntent(index)
        }
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
      .onTabBarClick((index: number) => {
        // Issue #906：onTabBarClick 是确认的用户点击入口，只记用户意图（nav.tab_requested）。
        this.recordTabIntent(index)
      })
      .onChange((index: number) => {
        // Issue #906：onChange 只管真实状态提交（用户点击与程序化恢复都会走到这里）。
        this.onTabChange(index)
      })
      .onAppear(() => {
        this.recordMountedShell('CompactPrimaryShell/Tabs')
      })
      .width('100%')
      .height('100%')
    }
  }

  build() {
    // Issue #972：全应用只有两种外壳入口，判定统一来自根窗口宽度（840vp）解析出的 WideLayoutMode。
    // 宽屏只挂 WideAppShell（新左栏 + 右上动作条 + 页面画布），窄屏只挂 CompactPrimaryShell。
    // 设置浮层由窗口级 custom dialog 承载（见 aboutToAppear 的 attachUIContext），与本 Stack 无关。
    if (this.usesWideNavigation) {
      this.WideShell()
    } else {
      Column() {
        this.SyncStatusBanner()
        Stack() {
          this.CompactPrimaryShell()
        }
        .layoutWeight(1)
        .width('100%')
      }
      .width('100%')
      .height('100%')
    }
  }
}
