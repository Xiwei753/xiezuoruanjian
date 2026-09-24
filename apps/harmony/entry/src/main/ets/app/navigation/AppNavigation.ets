// AppNavigation — 应用统一导航宿主。
// 用 HarmonyOS Navigation 组件 + NavDestination，按 Routes 装载对应 feature/*/ui Screen。
// 不再用 router 跳转。
// 导航事务逻辑（意图队列、离开守卫、安全 pop/replace/rebuild）已分离到 NavigationTransactionCoordinator。
// 窄屏一级壳层（作品/星图/统计底部 Tab Bar）在 compact/single-pane 模式下显示。
//
// Issue #760 评论 5822350374 问题2：
// - 底栏根据当前 route 对应的 ScreenRole showPrimaryNavigation 控制显隐
//   （Home/StarMap/Stats=true，Writing/Settings=false）
// - Tab 切换统一走 NavigationTransactionCoordinator.safeClearAndRebuild()

import { Routes, WorkspaceRouteParams, WritingRouteParams } from './Routes'
import hilog from '@ohos.hilog'
import { HomeScreen } from '../../feature/project/ui/HomeScreen'
import { WorkspaceScreen } from '../../feature/project/ui/WorkspaceScreen'
import { WritingScreen } from '../../feature/editor/ui/WritingScreen'
import { StarMapScreen } from '../../feature/starmap/ui/StarMapScreen'
import { SettingsScreen } from '../../feature/settings/ui/SettingsScreen'
import { StatsScreen } from '../../feature/stats/ui/StatsScreen'
import { ContinuationPayload } from '../../platform/continuity/ContinuationPayload'
import { NavigationTransactionCoordinator } from './NavigationTransactionCoordinator'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../ui/adaptive/AdaptiveContext'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../ui/theme/HarmonyThemeAdapter'

// 一级导航 Tab 索引常量
const TAB_HOME: number = 0    // 作品
const TAB_STARMAP: number = 1 // 星图
const TAB_STATS: number = 2   // 统计

// Issue #760 评论 5822350374 问题2：根级页面（showPrimaryNavigation=true）集合。
// 只有这些 route 在窄屏时显示底部一级导航栏；Writing/Settings 等二级页面不显示。
const ROOT_LEVEL_ROUTES: Set<string> = new Set([Routes.Home, Routes.StarMap, Routes.Stats])

@Component
export struct AppNavigation {
  @Provide('navPathStack') navPathStack: NavPathStack = new NavPathStack()
  // 当前接续恢复的编辑器状态（selection/cursor/viewport/contentHash），供页面层主动读取。
  @State continuationPayload: ContinuationPayload | null = null
  // 一级导航当前选中 Tab（0=作品, 1=星图, 2=统计）
  @State currentTab: number = TAB_HOME
  // 响应式布局状态（用于判断是否显示底部 Tab Bar）
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  // 主题配置（底部 Tab Bar 配色）
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  // Issue #760 评论 5822350374 问题2：是否显示底部一级导航栏。
  // 根级页面（Home/StarMap/Stats）=true，二级页面（Writing/Settings/Workspace）=false。
  @State showBottomBar: boolean = true

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  aboutToAppear(): void {
    // 默认显示 Home。
    this.navPathStack.pushPath({ name: Routes.Home })
    // Issue #752：首帧路标日志，确认 Home 已压入 navPathStack。
    hilog.info(0x0000, 'AppNavigation', 'aboutToAppear: pushed Home, navPathStack size=%{public}d', this.navPathStack.size())
    // 注册到 coordinator，使 EntryAbility 能触发接续恢复。
    NavigationTransactionCoordinator.getInstance().register(this.navPathStack, (payload: ContinuationPayload) => {
      this.applyContinuation(payload)
    })

    // 注册主题观察者，底部 Tab Bar 配色随主题变化。
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    // 注册响应式布局观察者，窗口尺寸变化时更新 adaptiveState（控制 Tab Bar 显隐）。
    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)
    // 初始化时同步一次状态。
    this.adaptiveState = AdaptiveContext.getInstance().getCurrentState()
  }

  aboutToDisappear(): void {
    NavigationTransactionCoordinator.getInstance().unregister()
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  // 根据接续 payload 重建 NavPathStack 并恢复 selection/cursor/viewport。
  // NavPathStack 不能自动迁移（SUPPORT_CONTINUE_PAGE_STACK_KEY=false），这里手动重建。
  // Issue #629 评论14 第2项：改用 safeClearAndRebuild，先调 leaveGuard（保存+关闭事务），
  // guard 成功才 clear + rebuild；失败保持当前页面，不清栈（不卸载活跃 WritingScreen 绕过保存）。
  private async applyContinuation(payload: ContinuationPayload): Promise<void> {
    const dedupeKey = `continuation:${payload.chapterId ?? ''}:${payload.projectId ?? ''}`
    await NavigationTransactionCoordinator.getInstance().safeClearAndRebuild(dedupeKey, (): void => {
      // 清栈到根后按 payload 重建。clear() 清空全部路由，再 push Home 作为根。
      this.navPathStack.clear()
      this.navPathStack.pushPath({ name: Routes.Home })
      // 接续恢复始终回到 Home tab。
      this.currentTab = TAB_HOME
      // Issue #760 评论 5822350374 问题2：接续恢复后如果导航到了二级页面，隐藏底栏。
      const hasProject: boolean = payload.projectId !== undefined && payload.projectId !== ''
      const hasChapter: boolean = payload.chapterId !== undefined && payload.chapterId !== ''
      const hasVolume: boolean = payload.volumeId !== undefined && payload.volumeId !== ''
      this.showBottomBar = !hasProject // 只有 Home 根页面时显示底栏

      if (hasProject) {
        const wsParam: WorkspaceRouteParams = {
          projectId: payload.projectId as string,
          projectName: '' // payload 不带 projectName；WorkspaceScreen 会从 core 加载。
        }
        this.navPathStack.pushPath({ name: Routes.Workspace, param: wsParam })
      }

      if (hasChapter && hasProject && hasVolume) {
        // 携带 selection/cursor/viewport/contentHash 到 WritingScreen 入口。
        // WritingRouteParams 正式声明这些可选字段，不再用 Record<string,Object> 偷塞。
        const wParam: WritingRouteParams = {
          chapterId: payload.chapterId as string,
          chapterTitle: '', // payload 不带 chapterTitle；WritingScreen 会从 core 加载章节标题。
          projectId: payload.projectId as string,
          volumeId: payload.volumeId as string
        }
        if (payload.selection !== undefined) {
          wParam.selection = payload.selection
        }
        if (payload.cursor !== undefined) {
          wParam.cursor = payload.cursor
        }
        if (payload.viewport !== undefined) {
          wParam.viewport = payload.viewport
        }
        if (payload.contentHash !== undefined) {
          wParam.contentHash = payload.contentHash
        }
        this.navPathStack.pushPath({ name: Routes.Writing, param: wParam })
      }

      // 暂存 payload，供页面层主动读取编辑器状态（selection/cursor/viewport）。
      this.continuationPayload = payload
    })
  }

  // 切换一级导航 Tab：清空 navPathStack 并 push 对应根页面。
  // 这样 StarMap/Stats 就有了一级壳层，不再是 pushPath 进入的二级页面。
  // 从 StarMap/Stats 页面可以 pushPath 进入其他页面（如 Settings），返回时回到对应的 tab 根页面。
  // Issue #760 评论 5822350374 问题2：改用 safeClearAndRebuild，先调 leaveGuard（保存+关闭事务），
  // guard 成功才 clear + rebuild；失败保持当前页面，不清栈。
  private switchToTab(tab: number): void {
    if (this.currentTab === tab) {
      return // 已在当前 Tab，不做导航
    }
    const targetRoute: string = tab === TAB_HOME ? Routes.Home
      : tab === TAB_STARMAP ? Routes.StarMap
      : Routes.Stats
    const dedupeKey = `tabSwitch:${targetRoute}`
    NavigationTransactionCoordinator.getInstance().safeClearAndRebuild(dedupeKey, (): void => {
      this.currentTab = tab
      this.showBottomBar = true // 根级页面始终显示底栏
      this.navPathStack.clear()
      this.navPathStack.pushPath({ name: targetRoute })
    })
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
    Column() {
      // 主内容区 — Navigation（占据剩余空间）
      Navigation(this.navPathStack) {
        // 内容由 navPathStack 驱动。
      }
      .navDestination(this.PageMap)
      .mode(NavigationMode.Stack)
      .hideTitleBar(true)
      .layoutWeight(1)

      // 底部 Tab Bar — 仅窄屏（!isTwoPane）且当前页面 showPrimaryNavigation=true 时显示
      // Issue #760 评论 5822350374 问题2：Writing/Settings 等二级页面不显示底栏
      if (!this.adaptiveState.isTwoPane && this.showBottomBar) {
        this.BottomTabBar()
      }
    }
    .width('100%')
    .height('100%')
  }

  // ── 底部一级壳层：作品/星图/统计 ──
  // 三个 tab 共享同一个底部导航栏，点击 tab 切换根页面（不是 pushPath）。
  // 当前选中 tab 高亮（primary 色），未选中 tab 用 textSecondary 色。
  @Builder
  BottomTabBar() {
    Row() {
      // 作品 tab
      Column() {
        Image($r('app.media.ic_star'))
          .width(22)
          .height(22)
          .fillColor(this.currentTab === TAB_HOME ? this.themeConfig.colors.primary : this.themeConfig.colors.textSecondary)
        Text('作品')
          .fontSize(12)
          .fontColor(this.currentTab === TAB_HOME ? this.themeConfig.colors.primary : this.themeConfig.colors.textSecondary)
          .margin({ top: 4 })
      }
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
      .onClick(() => {
        this.switchToTab(TAB_HOME)
      })

      // 星图 tab
      Column() {
        Image($r('app.media.ic_star'))
          .width(22)
          .height(22)
          .fillColor(this.currentTab === TAB_STARMAP ? this.themeConfig.colors.primary : this.themeConfig.colors.textSecondary)
        Text('星图')
          .fontSize(12)
          .fontColor(this.currentTab === TAB_STARMAP ? this.themeConfig.colors.primary : this.themeConfig.colors.textSecondary)
          .margin({ top: 4 })
      }
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
      .onClick(() => {
        this.switchToTab(TAB_STARMAP)
      })

      // 统计 tab
      Column() {
        Image($r('sys.media.ohos_ic_public_more'))
          .width(22)
          .height(22)
          .fillColor(this.currentTab === TAB_STATS ? this.themeConfig.colors.primary : this.themeConfig.colors.textSecondary)
        Text('统计')
          .fontSize(12)
          .fontColor(this.currentTab === TAB_STATS ? this.themeConfig.colors.primary : this.themeConfig.colors.textSecondary)
          .margin({ top: 4 })
      }
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
      .onClick(() => {
        this.switchToTab(TAB_STATS)
      })
    }
    .width('100%')
    .height(56)
    .backgroundColor(this.themeConfig.colors.surface)
    .shadow({ radius: 4, color: '#1A000000', offsetY: -2 })
  }
}
