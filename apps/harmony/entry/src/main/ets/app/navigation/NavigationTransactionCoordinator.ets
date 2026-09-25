// NavigationTransactionCoordinator — 导航事务协调器。
// 从 AppNavigation.ets 分离出来的导航事务逻辑：导航意图队列、离开守卫、安全 pop/replace/rebuild。
// AppNavigation struct 通过 getInstance() 注册 navPathStack 和恢复回调，
// EntryAbility / WritingScreen 通过 getInstance() 触发接续恢复和安全导航操作。
//
// Issue #767：支持多 tab stack。
// - 单一 navPathStack 改为 activeTabStack（当前活动 tab 的 stack）
// - 增加 setActiveStack() 方法，Tabs onChange 切换后更新活动栈
// - safePop / safeReplacePath / continuation restore 都操作当前活动栈
// - 接续恢复有 project/chapter 时先切到作品 tab，再重建作品栈
// - getNavPathStack() 返回当前活动栈

import { ContinuationPayload } from '../../platform/continuity/ContinuationPayload'

// Issue #629 评论14 第2项：导航意图类型。
// 区分 Pop / ClearAndRebuild / ReplacePath，相同 intent + 相同目标可复用同一 task；
// 不同 intent 必须串行排到前一个之后执行，不能丢。
type NavigationIntentKind = 'Pop' | 'ClearAndRebuild' | 'ReplacePath'

interface NavigationIntent {
  readonly kind: NavigationIntentKind
  // 用于去重的稳定 key。Pop='pop'，ReplacePath='replace:<targetKey>'，
  // ClearAndRebuild 由调用方提供（不同 continuation payload 不能因为 kind 相同就吞掉）。
  readonly dedupeKey: string
}

// Issue #629 评论14 第2项：离开守卫 lease（命名 interface，避免对象字面量类型）。
interface GuardLease {
  token: number
  guard: () => Promise<boolean>
}

// Issue #629 评论14 第2项：导航事务队列条目（命名 interface，避免对象字面量类型）。
interface NavigationTaskEntry {
  intent: NavigationIntent
  task: Promise<boolean>
  resolve: (v: boolean) => void
  execute: () => void
}

// Issue #629 评论14 第2项：正在执行的导航事务（命名 interface，避免对象字面量类型）。
interface ActiveNavigationTask {
  intent: NavigationIntent
  task: Promise<boolean>
}

// NavigationTransactionCoordinator — AppNavigation 的可外部访问入口。
// EntryAbility（非 UI 上下文）无法直接持有 @Component struct 实例，
// 通过此单例间接触发接续恢复：AppNavigation 在 aboutToAppear 注册 navPathStack 与恢复回调，
// EntryAbility.restoreContinuationFromWant 调 restoreFromContinuation(payload) 手动重建 NavPathStack。
// 评论明确要求 SUPPORT_CONTINUE_PAGE_STACK_KEY=false 禁用自动页面栈迁移，
// 由本协调器在目标设备手动重建。
//
// Issue #629 评论14 第2项：修复导航意图合并逻辑 + 唯一 guard token。
// - 不再用裸 navigationTask 合并所有 mutation。
// - 建 NavigationIntent，区分 Pop / ClearAndRebuild / ReplacePath。
// - 相同 intent + 相同目标可复用同一 task；不同 intent 串行排队。
// - registerLeaveGuard 返回唯一 lease/token（单调递增 number），
//   unregisterLeaveGuard(token) 只注销这一个实例。
// - 新增 safeReplacePath(path)，宽屏 Writing 左侧切章节统一走它。
//
// Issue #767：支持多 tab stack。
// - activeTabStack 替代单一 navPathStack，表示当前活动 tab 的 NavPathStack。
// - setActiveStack() 由 Tabs onChange 调用，切换活动栈。
// - safePop / safeReplacePath 操作 activeTabStack。
// - getNavPathStack() 返回 activeTabStack。
export class NavigationTransactionCoordinator {
  private static instance: NavigationTransactionCoordinator
  // Issue #767：当前活动 tab 的 NavPathStack。
  // Tabs onChange 切换后通过 setActiveStack() 更新。
  // safePop / safeReplacePath / getNavPathStack 都操作此栈。
  private activeTabStack: NavPathStack | null = null
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
  // Issue #629 评论14 第2项：当前活跃守卫 lease。
  // 新 Writing 注册时替换 active lease；unregisterLeaveGuard(token) 只有 token 等于当前 lease 才清掉。
  // 这样旧实例迟到的 disappear 不会删新 guard，也不会在一次 pop 时把底下页面的 session 一起 close。
  private activeGuardLease: GuardLease | null = null
  // Issue #629 评论14 第2项：导航事务串行队列。
  // 去重同时检查 pendingTasks + activeTask（正在执行的任务不能从"可去重集合"里消失）。
  private pendingTasks: Array<NavigationTaskEntry> = []
  // Issue #629 评论14 第2项：正在执行的任务。
  // processQueue 中 shift() 后执行期间，该 entry 仍在 activeTask 里，
  // 新的相同 dedupeKey 的请求可以直接复用 activeTask 的 Promise。
  private activeTask: ActiveNavigationTask | null = null
  private isProcessingQueue: boolean = false

  static getInstance(): NavigationTransactionCoordinator {
    if (!NavigationTransactionCoordinator.instance) {
      NavigationTransactionCoordinator.instance = new NavigationTransactionCoordinator()
    }
    return NavigationTransactionCoordinator.instance
  }

  // Issue #767：设置当前活动 tab 的 NavPathStack。
  // Tabs onChange 切换后调用此方法更新活动栈，
  // 使 safePop / safeReplacePath / getNavPathStack 操作正确的栈。
  setActiveStack(stack: NavPathStack): void {
    this.activeTabStack = stack
  }

  // Issue #767：注册恢复回调。
  // navPathStack 参数保留兼容性，内部只使用 activeTabStack。
  // 若 EntryAbility 已先交付 payload（pendingRestore），立即消费。
  register(navPathStack: NavPathStack, restoreListener: (payload: ContinuationPayload) => void): void {
    this.activeTabStack = navPathStack
    this.restoreListener = restoreListener
    if (this.pendingRestore !== null) {
      const pending: ContinuationPayload = this.pendingRestore
      this.pendingRestore = null
      restoreListener(pending)
    }
  }

  // AppNavigation.aboutToDisappear 注销，避免悬空回调。
  unregister(): void {
    this.activeTabStack = null
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

  // Issue #767：返回当前活动 tab 的 NavPathStack。
  // safePop / safeReplacePath 内部使用此方法获取操作目标。
  getNavPathStack(): NavPathStack | null {
    return this.activeTabStack
  }

  // Issue #629 评论14 第2项：注册离开守卫，返回唯一 token。
  // 多个 Writing destination 生命周期重叠时，每个实例有唯一 token。
  // guard 返回 boolean：true 表示可以离开（保存成功），false 表示不能离开（保存失败）。
  registerLeaveGuard(guard: () => Promise<boolean>): number {
    const token: number = this.nextGuardToken++
    // Issue #629 评论14 第2项：新 Writing 注册时替换 active lease。
    // coordinator 只持有当前 active guard；旧实例迟到的 disappear 不会删新 guard。
    const lease: GuardLease = { token, guard }
    this.activeGuardLease = lease
    return token
  }

  // Issue #629 评论14 第2项：注销离开守卫（按 token 精确注销）。
  // 只有 token 等于当前 active lease 才清掉，避免旧实例误注销新实例的 guard。
  unregisterLeaveGuard(token: number): void {
    if (this.activeGuardLease !== null && this.activeGuardLease.token === token) {
      this.activeGuardLease = null
    }
  }

  // Issue #629 评论14 第2项：串行导航事务队列。
  // 不同 intent 串行排队；相同 intent + 相同目标复用同一 task。
  // 队列消费：前一个 task 完成后才执行下一个。
  private enqueueNavigation(
    intent: NavigationIntent,
    execute: () => void
  ): Promise<boolean> {
    // Issue #629 评论14 第2项：去重同时检查 pendingTasks + activeTask。
    // 正在执行的任务不能从"可去重集合"里消失，否则 shift() 后新请求查不到它会再入队导致 double pop。
    for (const entry of this.pendingTasks) {
      if (entry.intent.dedupeKey === intent.dedupeKey) {
        return entry.task
      }
    }
    if (this.activeTask !== null && this.activeTask.intent.dedupeKey === intent.dedupeKey) {
      return this.activeTask.task
    }

    // 创建新 task
    let resolveTask: (v: boolean) => void
    const task: Promise<boolean> = new Promise<boolean>((resolve) => {
      resolveTask = resolve
    })

    const taskEntry: NavigationTaskEntry = { intent, task, resolve: resolveTask!, execute }
    this.pendingTasks.push(taskEntry)
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
      // Issue #629 评论14 第2项：shift 后立即设置 activeTask，
      // 使得新入队的相同 dedupeKey 请求可以复用正在执行的 Promise。
      const active: ActiveNavigationTask = { intent: entry.intent, task: entry.task }
      this.activeTask = active
      try {
        // Issue #629 评论14 第2项：只执行当前 active guard（不是遍历所有历史 guard）。
        let canLeave: boolean = true
        const lease = this.activeGuardLease
        if (lease !== null) {
          canLeave = await lease.guard()
        }

        if (canLeave) {
          entry.execute()
          entry.resolve(true)
        } else {
          entry.resolve(false)
        }
      } catch (_e) {
        entry.resolve(false)
      } finally {
        // 执行完成后清除 activeTask
        if (this.activeTask !== null && this.activeTask.task === entry.task) {
          this.activeTask = null
        }
      }
    }

    this.isProcessingQueue = false
  }

  // Issue #629 评论14 第2项：安全的 pop。
  // 串行排队，guard 成功才 activeTabStack.pop()；失败留在本页。
  // Issue #767：操作当前活动 tab 的 stack。
  safePop(): Promise<boolean> {
    const intent: NavigationIntent = { kind: 'Pop', dedupeKey: 'pop' }
    return this.enqueueNavigation(intent, (): void => {
      if (this.activeTabStack !== null) {
        this.activeTabStack.pop()
      }
    })
  }

  // Issue #629 评论14 第2项：安全的 clear + rebuild。
  // 串行排队，guard 成功才执行 rebuild（含 navPathStack.clear() + pushPath 重建）；
  // 失败保持当前页面，不清栈。
  safeClearAndRebuild(dedupeKey: string, rebuild: () => void): Promise<boolean> {
    const intent: NavigationIntent = { kind: 'ClearAndRebuild', dedupeKey }
    return this.enqueueNavigation(intent, rebuild)
  }

  // Issue #629 评论14 第2项：安全的 replace path。
  // 宽屏 Writing 左侧切章节统一走它：guard 成功才 replace 当前路由为新 Writing route；
  // 失败保持当前页面。
  // Issue #767：操作当前活动 tab 的 stack。
  safeReplacePath(targetKey: string, replace: () => void): Promise<boolean> {
    const intent: NavigationIntent = { kind: 'ReplacePath', dedupeKey: `replace:${targetKey}` }
    return this.enqueueNavigation(intent, replace)
  }
}
