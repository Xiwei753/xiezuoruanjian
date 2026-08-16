// AppNavigation — 应用统一导航宿主。
// 用 HarmonyOS Navigation 组件 + NavDestination，按 Routes 装载对应 feature/*/ui Screen。
// 不再用 router 跳转。

import { Routes, WorkspaceRouteParams, WritingRouteParams } from './Routes'
import { HomeScreen } from '../../feature/project/ui/HomeScreen'
import { WorkspaceScreen } from '../../feature/project/ui/WorkspaceScreen'
import { WritingScreen } from '../../feature/editor/ui/WritingScreen'
import { StarMapScreen } from '../../feature/starmap/ui/StarMapScreen'
import { SettingsScreen } from '../../feature/settings/ui/SettingsScreen'
import { StatsScreen } from '../../feature/stats/ui/StatsScreen'
import { ContinuationPayload } from '../../platform/continuity/ContinuationPayload'

// AppNavigationHost — AppNavigation 的可外部访问入口。
// EntryAbility（非 UI 上下文）无法直接持有 @Component struct 实例，
// 通过此单例间接触发接续恢复：AppNavigation 在 aboutToAppear 注册 navPathStack 与恢复回调，
// EntryAbility.restoreContinuationFromWant 调 restoreFromContinuation(payload) 手动重建 NavPathStack。
// 评论明确要求 SUPPORT_CONTINUE_PAGE_STACK_KEY=false 禁用自动页面栈迁移，
// 由本宿主在目标设备手动重建。
export class AppNavigationHost {
  private static instance: AppNavigationHost
  // AppNavigation 注册的 navPathStack；struct 未挂载时为 null。
  private navPathStack: NavPathStack | null = null
  // AppNavigation 注册的恢复回调：能访问 struct @State 进行 selection/cursor/viewport 恢复。
  private restoreListener: ((payload: ContinuationPayload) => void) | null = null
  // AppNavigation 尚未 aboutToAppear 时暂存 payload，待注册后消费。
  // 场景：onWindowStageRestore 早于首帧，EntryAbility 先把 payload 交来。
  private pendingRestore: ContinuationPayload | null = null

  static getInstance(): AppNavigationHost {
    if (!AppNavigationHost.instance) {
      AppNavigationHost.instance = new AppNavigationHost()
    }
    return AppNavigationHost.instance
  }

  // AppNavigation.aboutToAppear 注册 navPathStack 与恢复回调。
  // 若 EntryAbility 已先交付 payload（pendingRestore），立即消费。
  register(navPathStack: NavPathStack, restoreListener: (payload: ContinuationPayload) => void): void {
    this.navPathStack = navPathStack
    this.restoreListener = restoreListener
    if (this.pendingRestore !== null) {
      const pending: ContinuationPayload = this.pendingRestore
      this.pendingRestore = null
      restoreListener(pending)
    }
  }

  // AppNavigation.aboutToDisappear 注销，避免悬空回调。
  unregister(): void {
    this.navPathStack = null
    this.restoreListener = null
  }

  // EntryAbility 调此方法触发接续恢复。
  // 若 AppNavigation 已挂载，直接调回调；否则暂存，aboutToAppear 时消费。
  restoreFromContinuation(payload: ContinuationPayload): void {
    if (this.restoreListener !== null) {
      this.restoreListener(payload)
      return
    }
    this.pendingRestore = payload
  }

  // 供恢复回调内部使用：直接操作 navPathStack 重建路由栈。
  getNavPathStack(): NavPathStack | null {
    return this.navPathStack
  }
}

@Component
export struct AppNavigation {
  @Provide('navPathStack') navPathStack: NavPathStack = new NavPathStack()
  // 当前接续恢复的编辑器状态（selection/cursor/viewport/revision），供页面层主动读取。
  @State continuationPayload: ContinuationPayload | null = null

  aboutToAppear(): void {
    // 默认显示 Home。
    this.navPathStack.pushPath({ name: Routes.Home })
    // 注册到 host，使 EntryAbility 能触发接续恢复。
    AppNavigationHost.getInstance().register(this.navPathStack, (payload: ContinuationPayload) => {
      this.applyContinuation(payload)
    })
  }

  aboutToDisappear(): void {
    AppNavigationHost.getInstance().unregister()
  }

  // 根据接续 payload 重建 NavPathStack 并恢复 selection/cursor/viewport。
  // NavPathStack 不能自动迁移（SUPPORT_CONTINUE_PAGE_STACK_KEY=false），这里手动重建。
  private applyContinuation(payload: ContinuationPayload): void {
    // 清栈到根后按 payload 重建。clear() 清空全部路由，再 push Home 作为根。
    this.navPathStack.clear()
    this.navPathStack.pushPath({ name: Routes.Home })

    const hasProject: boolean = payload.projectId !== undefined && payload.projectId !== ''
    const hasChapter: boolean = payload.chapterId !== undefined && payload.chapterId !== ''
    const hasVolume: boolean = payload.volumeId !== undefined && payload.volumeId !== ''

    if (hasProject) {
      const wsParam: WorkspaceRouteParams = {
        projectId: payload.projectId as string,
        projectName: '' // payload 不带 projectName；WorkspaceScreen 会从 core 加载。
      }
      this.navPathStack.pushPath({ name: Routes.Workspace, param: wsParam })
    }

    if (hasChapter && hasProject && hasVolume) {
      // 携带 selection/cursor/viewport/revision 到 WritingScreen 入口。
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
      if (payload.revision !== undefined) {
        wParam.revision = payload.revision
      }
      this.navPathStack.pushPath({ name: Routes.Writing, param: wParam })
    }

    // 暂存 payload，供页面层主动读取编辑器状态（selection/cursor/viewport）。
    this.continuationPayload = payload
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
    Navigation(this.navPathStack) {
      // 内容由 navPathStack 驱动。
    }
    .navDestination(this.PageMap)
    .mode(NavigationMode.Stack)
    .hideTitleBar(true)
  }
}
