// AppNavigation — 应用统一导航宿主。
// Issue #767：改为装载 PrimaryTabShell（真正的横向 Tabs + BottomTabBarStyle）。
// 删除手写 BottomTabBar builder 和 switchToTab 的 clear() + pushPath() 实现。
// 每个 tab 各自持有自己的 NavPathStack 和 Navigation，一级切换只切 tab index。
//
// 修复 navPathStack render 期间修改问题：
// 不再在 aboutToAppear() 里 pushPath(Home)，NavPathStack 在 PrimaryTabShell 字段初始化时
// 通过工厂函数 createStackWithRoot() 创建并带根路由。
//
// 导航事务逻辑（意图队列、离开守卫、安全 pop/replace/rebuild）在 NavigationTransactionCoordinator。
// 接续恢复逻辑保留在 AppNavigation（通过 coordinator 获取活动栈操作）。

import {
  Routes,
  SettingsRouteParams,
  StarMapFocusKind,
  StarMapRouteParams,
  StatsRouteParams,
  SearchRouteParams,
  WorkspaceRouteParams,
  WritingRouteParams
} from './Routes'
import hilog from '@ohos.hilog'
import { SearchContext, SearchOriginScreen } from '../../corebridge/dto/SearchDtos'
import { ContinuationPayload, continuationTabIndex } from '../../platform/continuity/ContinuationPayload'
import { NavigationTransactionCoordinator } from './NavigationTransactionCoordinator'
import { PrimaryTabShell } from './PrimaryTabShell'
import { SearchOverlayController } from '../../feature/search/presentation/SearchOverlayController'
import { WideLayoutMode, getWideLayoutState } from './wide/WideLayoutMode'

// 一级导航 Tab 索引常量
const TAB_HOME: number = 0    // 作品
const TAB_STARMAP: number = 1 // 星图
const TAB_STATS: number = 2   // 统计

@Component
export struct AppNavigation {
  // 当前接续恢复的编辑器状态（selection/cursor/viewport/contentHash），供页面层主动读取。
  @State continuationPayload: ContinuationPayload | null = null
  // Issue #767：一级导航当前选中 Tab（0=作品, 1=星图, 2=统计），与 PrimaryTabShell 通过 @Link 共享。
  @State currentTab: number = TAB_HOME

  aboutToAppear(): void {
    hilog.info(0x0000, 'AppNavigation', 'aboutToAppear: registering with coordinator')
    // 注册到 coordinator，使 ContinuationEntrypoint 触发的接续恢复能落到本组件。
    // Issue #767 评论5830638688 第1项：不再创建 placeholderStack 污染 activeTabStack。
    // 只注册 restoreListener 回调；pending payload 的消费延迟到 PrimaryTabShell 注册 homeStack
    // 后由 tryConsumePendingRestore() 触发。
    const coordinator = NavigationTransactionCoordinator.getInstance()
    coordinator.registerRestoreListener((payload: ContinuationPayload) => {
      this.applyContinuation(payload)
    })
    // Issue #982：宽屏全局搜索浮层由根导航宿主注册窗口级 UIContext；
    // 浮层关闭只 dismiss 对话框，底层路由栈与页面状态保持不变。
    SearchOverlayController.getInstance().attachUIContext(this.getUIContext())
  }

  aboutToDisappear(): void {
    NavigationTransactionCoordinator.getInstance().unregister()
    SearchOverlayController.getInstance().detachUIContext(this.getUIContext())
  }

  // Issue #988：按 payload.screen/tabIndex 重建源页面所属的真实 tab 和页面层级。
  // 不再固定回 Home，也不假定接续一定发生在写作页。
  private async applyContinuation(payload: ContinuationPayload): Promise<void> {
    const targetTab: number = continuationTabIndex(payload)
    const dedupeKey =
      `continuation:${payload.snapshotSequence ?? 0}:${payload.screen ?? 'writing'}:${payload.projectId ?? ''}:${payload.chapterId ?? ''}`
    await NavigationTransactionCoordinator.getInstance().safeClearAndRebuild(dedupeKey, (): void => {
      const coordinator = NavigationTransactionCoordinator.getInstance()
      const targetStack: NavPathStack | null = coordinator.getTabStack(targetTab)
      if (targetStack === null) {
        hilog.error(0x0000, 'AppNavigation', 'applyContinuation: target stack not registered')
        return
      }

      this.currentTab = targetTab
      targetStack.clear()
      const screen: string = payload.screen ?? this.legacyContinuationScreen(payload, targetTab)
      const wideMode: boolean = getWideLayoutState().current() === WideLayoutMode.Wide
      const hasProject: boolean = payload.projectId !== undefined && payload.projectId !== ''
      const hasChapter: boolean = payload.chapterId !== undefined && payload.chapterId !== ''

      // 先用目标 tab 的真实根页面建立层级，再按 screen 压入源页面。
      // settings/search 即使发生在星图/统计 tab，也必须在对应 tab 上重建，而不是打开星图/统计根。
      if (targetTab === TAB_STARMAP) {
        targetStack.pushPath({ name: Routes.StarMapList })
      } else if (targetTab === TAB_STATS) {
        targetStack.pushPath({ name: Routes.Stats })
      } else {
        targetStack.pushPath({ name: Routes.Home })
      }

      if (screen === 'starmap' && targetTab === TAB_STARMAP &&
        payload.starmapId !== undefined && payload.starmapId !== '') {
        const starParam: StarMapRouteParams = {
          starmapId: payload.starmapId,
          starmapTitle: payload.starmapTitle ?? ''
        }
        if (payload.parentStarmapId !== undefined) {
          starParam.parentStarmapId = payload.parentStarmapId
        }
        if (payload.parentStarmapTitle !== undefined) {
          starParam.parentStarmapTitle = payload.parentStarmapTitle
        }
        if (payload.focusKind !== undefined) {
          starParam.focusKind = payload.focusKind as StarMapFocusKind
        }
        if (payload.focusId !== undefined) {
          starParam.focusId = payload.focusId
        }
        starParam.parentPath = payload.parentPath
        starParam.focusScenePath = payload.focusScenePath
        starParam.selectedNodeId = payload.selectedNodeId
        starParam.selectedKind = payload.selectedKind
        starParam.cameraScale = payload.cameraScale
        starParam.cameraOffsetX = payload.cameraOffsetX
        starParam.cameraOffsetY = payload.cameraOffsetY
        targetStack.pushPath({ name: Routes.StarMap, param: starParam })
      } else if (screen === 'stats' && targetTab === TAB_STATS) {
        const statsParam: StatsRouteParams = {}
        if (payload.statsPeriod !== undefined) {
          statsParam.period = payload.statsPeriod
        }
        if (payload.statsExpandedPanel !== undefined) {
          statsParam.expandedPanel = payload.statsExpandedPanel
        }
        if (payload.statsHeatmapYear !== undefined) {
          statsParam.heatmapYear = payload.statsHeatmapYear
        }
        targetStack.clear()
        targetStack.pushPath({ name: Routes.Stats, param: statsParam })
      } else if (screen === 'settings') {
        const settingsParam: SettingsRouteParams = {}
        if (payload.settingsSection !== undefined) {
          settingsParam.section = payload.settingsSection
        }
        if (payload.settingKey !== undefined) {
          settingsParam.settingKey = payload.settingKey
        }
        targetStack.pushPath({ name: Routes.Settings, param: settingsParam })
      } else if (screen === 'search') {
        const searchParam: SearchRouteParams = { context: this.buildSearchContext(payload) }
        if (payload.searchQuery !== undefined) {
          searchParam.initialQuery = payload.searchQuery
        }
        targetStack.pushPath({ name: Routes.Search, param: searchParam })
      } else if (screen === 'workspace' && hasProject && targetTab === TAB_HOME) {
        const wsParam: WorkspaceRouteParams = {
          projectId: payload.projectId as string,
          projectName: ''
        }
        targetStack.pushPath({ name: Routes.Workspace, param: wsParam })
      } else if ((screen === 'writing' || (payload.screen === undefined && hasChapter)) &&
        hasProject && targetTab === TAB_HOME) {
        if (!wideMode) {
          const wsParam: WorkspaceRouteParams = {
            projectId: payload.projectId as string,
            projectName: ''
          }
          targetStack.pushPath({ name: Routes.Workspace, param: wsParam })
        }
        if (wideMode || (hasChapter && payload.volumeId !== undefined && payload.volumeId !== '')) {
          const wParam: WritingRouteParams = {
            chapterId: hasChapter ? payload.chapterId as string : '',
            chapterTitle: '',
            projectId: payload.projectId as string,
            volumeId: payload.volumeId ?? ''
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
          if (payload.snapshotId !== undefined) {
            wParam.snapshotId = payload.snapshotId
          }
          if (payload.snapshotHash !== undefined) {
            wParam.snapshotHash = payload.snapshotHash
          }
          if (payload.sourceDeviceId !== undefined) {
            wParam.sourceDeviceId = payload.sourceDeviceId
          }
          if (payload.sourceNetworkId !== undefined) {
            wParam.sourceNetworkId = payload.sourceNetworkId
          }
          targetStack.pushPath({ name: Routes.Writing, param: wParam })
        }
      }

      this.continuationPayload = payload
    })
  }

  private legacyContinuationScreen(payload: ContinuationPayload, targetTab: number): string {
    if (targetTab === TAB_STARMAP) {
      return 'starmapList'
    }
    if (targetTab === TAB_STATS) {
      return 'stats'
    }
    return payload.chapterId !== undefined ? 'writing' : 'home'
  }

  private buildSearchContext(payload: ContinuationPayload): SearchContext {
    const origin: SearchOriginScreen =
      (payload.searchOriginScreen as SearchOriginScreen | undefined) ?? 'home'
    const context: SearchContext = { originScreen: origin }
    if (payload.searchProjectId !== undefined) {
      context.currentProjectId = payload.searchProjectId
    }
    if (payload.searchVolumeId !== undefined) {
      context.currentVolumeId = payload.searchVolumeId
    }
    if (payload.searchChapterId !== undefined) {
      context.currentChapterId = payload.searchChapterId
    }
    if (payload.searchStarmapId !== undefined) {
      context.currentStarmapId = payload.searchStarmapId
    }
    if (payload.searchSettingsSection !== undefined) {
      context.currentSettingsSection = payload.searchSettingsSection
    }
    return context
  }

  build() {
    Column() {
      // Issue #972：应用只有一个导航宿主 PrimaryTabShell；
      // 它内部按根窗口宽度只挂载 WideAppShell（宽屏）或 CompactPrimaryShell（窄屏）之一。
      // currentTab 通过 @Link 共享，接续恢复时可从 AppNavigation 切到 Home tab。
      PrimaryTabShell({ currentTab: $currentTab })
    }
    .width('100%')
    .height('100%')
  }
}
