// AppNavigation — 应用统一导航宿主。
// 用 HarmonyOS Navigation 组件 + NavDestination，按 Routes 装载对应 feature/*/ui Screen。
// 不再用 router 跳转。
// 导航事务逻辑（意图队列、离开守卫、安全 pop/replace/rebuild）已分离到 NavigationTransactionCoordinator。

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

@Component
export struct AppNavigation {
  @Provide('navPathStack') navPathStack: NavPathStack = new NavPathStack()
  // 当前接续恢复的编辑器状态（selection/cursor/viewport/contentHash），供页面层主动读取。
  @State continuationPayload: ContinuationPayload | null = null

  aboutToAppear(): void {
    // 默认显示 Home。
    this.navPathStack.pushPath({ name: Routes.Home })
    // Issue #752：首帧路标日志，确认 Home 已压入 navPathStack。
    hilog.info(0x0000, 'AppNavigation', 'aboutToAppear: pushed Home, navPathStack size=%{public}d', this.navPathStack.size())
    // 注册到 coordinator，使 EntryAbility 能触发接续恢复。
    NavigationTransactionCoordinator.getInstance().register(this.navPathStack, (payload: ContinuationPayload) => {
      this.applyContinuation(payload)
    })
  }

  aboutToDisappear(): void {
    NavigationTransactionCoordinator.getInstance().unregister()
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
