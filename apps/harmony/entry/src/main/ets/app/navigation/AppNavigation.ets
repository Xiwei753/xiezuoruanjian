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

// Issue #629 评论14 第2项：导航意图类型。
// 区分 Pop / ClearAndRebuild / ReplacePath，相同 intent + 相同目标可复用同一 task；
// 不同 intent 必须串行排到前一个之后执行，不能丢。
type NavigationIntentKind = 'Pop' | 'ClearAndRebuild' | 'ReplacePath'

interface NavigationIntent {
  readonly kind: NavigationIntentKind
  // ReplacePath 时的目标路由名（Pop/ClearAndRebuild 不需要）
  readonly targetPath?: string
}

// AppNavigationHost — AppNavigation 的可外部访问入口。
// EntryAbility（非 UI 上下文）无法直接持有 @Component struct 实例，
// 通过此单例间接触发接续恢复：AppNavigation 在 aboutToAppear 注册 navPathStack 与恢复回调，
// EntryAbility.restoreContinuationFromWant 调 restoreFromContinuation(payload) 手动重建 NavPathStack。
// 评论明确要求 SUPPORT_CONTINUE_PAGE_STACK_KEY=false 禁用自动页面栈迁移，
// 由本宿主在目标设备手动重建。
//
// Issue #629 评论14 第2项：修复导航意图合并逻辑 + 唯一 guard token。
// - 不再用裸 navigationTask 合并所有 mutation。
// - 建 NavigationIntent，区分 Pop / ClearAndRebuild / ReplacePath。
// - 相同 intent + 相同目标可复用同一 task；不同 intent 串行排队。
// - registerLeaveGuard 返回唯一 lease/token（单调递增 number），
//   unregisterLeaveGuard(token) 只注销这一个实例。
// - 新增 safeReplacePath(path)，宽屏 Writing 左侧切章节统一走它。
export class AppNavigationHost {
  private static instance: AppNavigationHost
  // AppNavigation 注册的 navPathStack；struct 未挂载时为 null。
  private navPathStack: NavPathStack | null = null
  // AppNavigation 注册的恢复回调：能访问 struct @State 进行 selection/cursor/viewport 恢复。
  private restoreListener: ((payload: ContinuationPayload) => void) | null = null
  // AppNavigation 尚未 aboutToAppear 时暂存 payload，待注册后消费。
  // 场景：onWindowStageRestore 早于首帧，EntryAbility 先把 payload 交来。
  private pendingRestore: ContinuationPayload | null = null
  // Issue #629 评论14 第2项：离开守卫注册表（token → guard）。
  // 多个 Writing destination 生命周期重叠时，每个实例有唯一 token，
  // unregisterLeaveGuard(token) 只注销自己注册的 guard。
  private leaveGuards: Map<number, () => Promise<boolean>> = new Map()
  private nextGuardToken: number = 1
  // Issue #629 评论14 第2项：导航事务串行队列。
  // 每个 task 记录自己的 intent；不同 intent 串行排队，相同 intent + 相同目标复用。
  private pendingTasks: Array<{ intent: NavigationIntent; task: Promise<boolean>; resolve: (v: boolean) => void; execute: () => void }> = []
  private isProcessingQueue: boolean = false

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

  // Issue #629 评论14 第2项：注册离开守卫，返回唯一 token。
  // 多个 Writing destination 生命周期重叠时，每个实例有唯一 token。
  // guard 返回 boolean：true 表示可以离开（保存成功），false 表示不能离开（保存失败）。
  registerLeaveGuard(guard: () => Promise<boolean>): number {
    const token: number = this.nextGuardToken++
    this.leaveGuards.set(token, guard)
    return token
  }

  // Issue #629 评论14 第2项：注销离开守卫（按 token 精确注销）。
  // 不再用固定字符串常量，避免旧实例误注销新实例的 guard。
  unregisterLeaveGuard(token: number): void {
    this.leaveGuards.delete(token)
  }

  // Issue #629 评论14 第2项：串行导航事务队列。
  // 不同 intent 串行排队；相同 intent + 相同目标复用同一 task。
  // 队列消费：前一个 task 完成后才执行下一个。
  private enqueueNavigation(
    intent: NavigationIntent,
    execute: () => void
  ): Promise<boolean> {
    // 检查是否有相同 intent + 相同目标的 task 正在排队（复用）
    for (const entry of this.pendingTasks) {
      if (entry.intent.kind === intent.kind && entry.intent.targetPath === intent.targetPath) {
        return entry.task
      }
    }

    // 创建新 task
    let resolveTask: (v: boolean) => void
    const task: Promise<boolean> = new Promise<boolean>((resolve) => {
      resolveTask = resolve
    })

    this.pendingTasks.push({ intent, task, resolve: resolveTask!, execute })
    this.processQueue()
    return task
  }

  private async processQueue(): Promise<void> {
    if (this.isProcessingQueue) {
      return
    }
    this.isProcessingQueue = true

    while (this.pendingTasks.length > 0) {
      const entry = this.pendingTasks.shift()!
      try {
        // 执行所有注册的 leave guard（取最严格的：只要有一个 guard 返回 false 就阻止）
        let canLeave: boolean = true
        for (const [token, guard] of this.leaveGuards) {
          const ok: boolean = await guard()
          if (!ok) {
            canLeave = false
            break
          }
        }

        if (canLeave) {
          entry.execute()
          entry.resolve(true)
        } else {
          entry.resolve(false)
        }
      } catch (_e) {
        entry.resolve(false)
      }
    }

    this.isProcessingQueue = false
  }

  // Issue #629 评论14 第2项：安全的 pop。
  // 串行排队，guard 成功才 navPathStack.pop()；失败留在本页。
  safePop(): Promise<boolean> {
    const intent: NavigationIntent = { kind: 'Pop' }
    return this.enqueueNavigation(intent, (): void => {
      if (this.navPathStack !== null) {
        this.navPathStack.pop()
      }
    })
  }

  // Issue #629 评论14 第2项：安全的 clear + rebuild。
  // 串行排队，guard 成功才执行 rebuild（含 navPathStack.clear() + pushPath 重建）；
  // 失败保持当前页面，不清栈。
  safeClearAndRebuild(rebuild: () => void): Promise<boolean> {
    const intent: NavigationIntent = { kind: 'ClearAndRebuild' }
    return this.enqueueNavigation(intent, rebuild)
  }

  // Issue #629 评论14 第2项：安全的 replace path。
  // 宽屏 Writing 左侧切章节统一走它：guard 成功才 replace 当前路由为新 Writing route；
  // 失败保持当前页面。
  safeReplacePath(replace: () => void): Promise<boolean> {
    const intent: NavigationIntent = { kind: 'ReplacePath' }
    return this.enqueueNavigation(intent, replace)
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
  // Issue #629 评论14 第2项：改用 safeClearAndRebuild，先调 leaveGuard（保存+关闭事务），
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
