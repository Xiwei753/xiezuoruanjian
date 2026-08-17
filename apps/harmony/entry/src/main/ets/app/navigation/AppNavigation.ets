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
//
// Issue #629 评论11 第3项：LeaveGuard + safePop/safeClearAndRebuild 幂等导航事务。
// - registerLeaveGuard(owner, guard)：当前 destination 注册异步保存+关闭事务（guard 返回 boolean）。
// - unregisterLeaveGuard(owner)：只注销自己注册的 guard（owner token 匹配）。
// - safePop()/safeClearAndRebuild(rebuild)：把 guard + NavPathStack mutation 收成一个串行事务。
//   两个并发 safePop 合并成一次（navigationTask 复用同一 Promise，幂等）。
// - applyContinuation 改用 safeClearAndRebuild，避免直接 clear() 卸载活跃 WritingScreen 绕过保存。
export class AppNavigationHost {
  private static instance: AppNavigationHost
  // AppNavigation 注册的 navPathStack；struct 未挂载时为 null。
  private navPathStack: NavPathStack | null = null
  // AppNavigation 注册的恢复回调：能访问 struct @State 进行 selection/cursor/viewport 恢复。
  private restoreListener: ((payload: ContinuationPayload) => void) | null = null
  // AppNavigation 尚未 aboutToAppear 时暂存 payload，待注册后消费。
  // 场景：onWindowStageRestore 早于首帧，EntryAbility 先把 payload 交来。
  private pendingRestore: ContinuationPayload | null = null
  // Issue #629 评论11 第3项：离开守卫。当前 destination 注册的异步保存+关闭事务。
  // guard 返回 boolean：true 表示可以离开（保存成功），false 表示不能离开（保存失败，留在本页）。
  private leaveGuard: (() => Promise<boolean>) | null = null
  // leaveGuard 的 owner token：unregisterLeaveGuard 只注销自己注册的 guard（避免误删别人的）。
  private leaveGuardOwner: string | null = null
  // Issue #629 评论11 第3项：导航事务串行队列。两个并发操作合并，guard 一次、mutation 一次。
  // navigationTask !== null 时复用同一 task（幂等），返回它的结果。
  private navigationTask: Promise<boolean> | null = null

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

  // Issue #629 评论11 第3项：注册离开守卫。
  // 当前 destination（如 WritingScreen）在 aboutToAppear 注册异步保存+关闭事务。
  // guard 返回 boolean：true 表示可以离开（保存成功），false 表示不能离开（保存失败）。
  // owner 是 token，unregisterLeaveGuard 只注销自己注册的 guard。
  registerLeaveGuard(owner: string, guard: () => Promise<boolean>): void {
    this.leaveGuard = guard
    this.leaveGuardOwner = owner
  }

  // Issue #629 评论11 第3项：注销离开守卫。
  // 只注销自己注册的 guard（owner token 匹配），避免误删别人注册的 guard。
  unregisterLeaveGuard(owner: string): void {
    if (this.leaveGuardOwner === owner) {
      this.leaveGuard = null
      this.leaveGuardOwner = null
    }
  }

  // Issue #629 评论11 第3项：串行导航事务。
  // 把 guard + NavPathStack mutation 收成一个事务：
  // - 若已有 navigationTask，复用同一 task（幂等，两个并发 safePop 合并成一次），返回它的结果。
  // - 否则创建新 task：await leaveGuard()（无 guard 视为 true）→ 成功才执行 mutation → 返回 true；
  //   guard 失败返回 false（不执行 mutation，留在当前页面）。
  // - task 完成后清 navigationTask（允许后续新导航）。
  //
  // 关键：本方法不是 async 函数，直接返回 Promise 引用。
  // async 函数会包一层新 Promise，破坏 navigationTask 引用复用（并发 safePop 无法返回同一 Promise）。
  // 用 .then() 清 navigationTask（避免 const task TDZ：async IIFE 同步完成时 finally 引用 task 时 task 尚未赋值）。
  private runGuardedNavigation(mutation: () => void): Promise<boolean> {
    if (this.navigationTask !== null) {
      return this.navigationTask
    }
    const task: Promise<boolean> = (async (): Promise<boolean> => {
      const guard: (() => Promise<boolean>) | null = this.leaveGuard
      if (guard !== null) {
        const ok: boolean = await guard()
        if (!ok) {
          return false
        }
      }
      mutation()
      return true
    })()
    this.navigationTask = task
    // task 完成后清 navigationTask，允许后续新导航。
    // 用 === task 防止已被并发新任务覆盖。
    task.then(
      (_ok: boolean): void => {
        if (this.navigationTask === task) {
          this.navigationTask = null
        }
      },
      (_e: Object): void => {
        if (this.navigationTask === task) {
          this.navigationTask = null
        }
      }
    )
    return task
  }

  // Issue #629 评论11 第3项：安全的 pop。
  // 先调 leaveGuard（保存+关闭事务），guard 成功才 navPathStack.pop()；失败留在本页。
  // 两个并发 safePop 合并成一次（navigationTask 复用同一 Promise，幂等）。
  // 不是 async 函数：直接返回 Promise 引用，保证幂等复用。
  safePop(): Promise<boolean> {
    return this.runGuardedNavigation((): void => {
      if (this.navPathStack !== null) {
        this.navPathStack.pop()
      }
    })
  }

  // Issue #629 评论11 第3项：安全的 clear + rebuild。
  // 先调 leaveGuard，guard 成功才执行 rebuild（含 navPathStack.clear() + pushPath 重建）；
  // 失败保持当前页面，不清栈。两个并发 safeClearAndRebuild 合并成一次。
  // 不是 async 函数：直接返回 Promise 引用，保证幂等复用。
  safeClearAndRebuild(rebuild: () => void): Promise<boolean> {
    return this.runGuardedNavigation(rebuild)
  }
}

@Component
export struct AppNavigation {
  @Provide('navPathStack') navPathStack: NavPathStack = new NavPathStack()
  // 当前接续恢复的编辑器状态（selection/cursor/viewport/contentHash），供页面层主动读取。
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
  // Issue #629 评论11 第3项：改用 safeClearAndRebuild，先调 leaveGuard（保存+关闭事务），
  // guard 成功才 clear + rebuild；失败保持当前页面，不清栈（不卸载活跃 WritingScreen 绕过保存）。
  private async applyContinuation(payload: ContinuationPayload): Promise<void> {
    await AppNavigationHost.getInstance().safeClearAndRebuild((): void => {
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
