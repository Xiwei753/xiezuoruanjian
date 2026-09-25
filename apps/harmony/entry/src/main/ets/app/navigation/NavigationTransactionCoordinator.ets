// NavigationTransactionCoordinator — 导航事务协调器。
// 从 AppNavigation.ets 分离出来的导航事务逻辑：导航意图队列、离开守卫、安全 pop/replace/rebuild。
// AppNavigation struct 通过 getInstance() 注册 navPathStack 和恢复回调，
// EntryAbility / WritingScreen 通过 getInstance() 触发接续恢复和安全导航操作。
//
// Issue #767：支持多 tab stack。
// - 单一 navPathStack 改为 tabStacks Map（显式登记每个 tab 的 stack）
// - 增加 registerTabStack / getTabStack / setActiveTab 方法
// - pendingRestore 延迟到 homeStack 可用时才消费（tryConsumePendingRestore）
// - safePop / safeReplacePath 仍操作 activeTabStack（兼容）
// - getNavPathStack() 返回当前活动栈（兼容）

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
// - tabStacks Map 显式登记每个 tab 的 NavPathStack，替代单一 activeTabStack。
// - setActiveTab() 设置当前活动 tab index，getTabStack() 按 index 取栈。
// - pendingRestore 延迟到 homeStack 可用时才消费（tryConsumePendingRestore）。
// - safePop / safeReplacePath 仍操作 activeTabStack（兼容）。
// - getNavPathStack() 返回当前活动栈（兼容）。
export class NavigationTransactionCoordinator {
  private static instance: NavigationTransactionCoordinator
  // Issue #767：当前活动 tab 的 NavPathStack（兼容字段，safePop/safeReplacePath 使用）。
  private activeTabStack: NavPathStack | null = null
  // Issue #767：当前活动 tab index。
  private activeTabIndex: number = 0
  // Issue #767：显式登记的 tab stack Map，key 为 tab index。
  private tabStacks: Map<number, NavPathStack> = new Map()
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

  // Issue #767：设置当前活动 tab 的 NavPathStack（兼容方法）。
  // Tabs onChange 切换后调用此方法更新活动栈，
  // 使 safePop / safeReplacePath / getNavPathStack 操作正确的栈。
  setActiveStack(stack: NavPathStack): void {
    this.activeTabStack = stack
  }

  // Issue #767：登记指定 tab index 的 NavPathStack。
  // PrimaryTabShell 在 aboutToAppear 中为三个 tab 分别调用。
  registerTabStack(tabIndex: number, stack: NavPathStack): void {
    this.tabStacks.set(tabIndex, stack)
  }

  // Issue #767：获取指定 tab index 的 NavPathStack。
  // 返回 null 表示该 tab 的 stack 尚未注册（PrimaryTabShell 还没挂载）。
  getTabStack(tabIndex: number): NavPathStack | null {
    const stack: NavPathStack | undefined = this.tabStacks.get(tabIndex)
    if (stack === undefined) {
      return null
    }
    return stack
  }

  // Issue #767：设置当前活动 tab index。
  // 同时更新 activeTabStack 为对应 tab 的 stack（如果已注册）。
  setActiveTab(tabIndex: number): void {
    this.activeTabIndex = tabIndex
    const stack: NavPathStack | null = this.getTabStack(tabIndex)
    if (stack !== null) {
      this.activeTabStack = stack
    }
  }

  // Issue #767 评论5830638688 第1项：注册恢复回调（纯 listener，不再收 placeholderStack）。
  // 不再接受 navPathStack 参数——之前传 placeholderStack 会污染 activeTabStack，
  // 导致 safePop/safeReplacePath 在 PrimaryTabShell 注册真实栈之前操作错误栈。
  // pending payload 的消费延迟到 PrimaryTabShell 注册 homeStack 后调用 tryConsumePendingRestore()。
  registerRestoreListener(restoreListener: (payload: ContinuationPayload) => void): void {
    this.restoreListener = restoreListener
  }

  // Issue #767：尝试消费待处理的接续恢复。
  // 当 PrimaryTabShell 注册了 homeStack 后调用此方法。
  // 如果有 pendingRestore 且 homeStack 可用（getTabStack(TAB_HOME) 返回非 null），才消费。
  tryConsumePendingRestore(): void {
    if (this.pendingRestore !== null && this.restoreListener !== null) {
      // 确认 homeStack 已注册才消费，避免恢复内容写到错误的栈。
      const homeStack: NavPathStack | null = this.getTabStack(0)
      if (homeStack !== null) {
        const pending: ContinuationPayload = this.pendingRestore
        this.pendingRestore = null
        this.restoreListener(pending)
      }
    }
  }

  // Issue #767 评论5831949869 第1项：按实例相等注销指定 tab 的 NavPathStack。
  // 必须按"实例相等"再删，避免旧 Shell 的迟到 disappear 把新 Shell 已注册的 stack 删掉。
  unregisterTabStack(tabIndex: number, stack: NavPathStack): void {
    const registered = this.tabStacks.get(tabIndex)
    if (registered === stack) {
      this.tabStacks.delete(tabIndex)
    }
    if (this.activeTabStack === stack) {
      this.activeTabStack = null
    }
  }

  // AppNavigation.aboutToDisappear 注销，避免悬空回调。
  unregister(): void {
    this.activeTabStack = null
    this.restoreListener = null
  }

  // EntryAbility 调此方法触发接续恢复。
  // Issue #767 评论5830638688 第1项：同时检查 restoreListener 和 homeStack 是否可用。
  // 之前只检查 restoreListener：如果 continuation 正好在"restoreListener 已注册、homeStack 还没注册"
  // 的窗口到达，payload 直接进 applyContinuation()，getTabStack(TAB_HOME) 返回 null，
  // callback 直接 return，payload 没重新放回 pendingRestore，接续数据就丢了。
  // 现在改为：只要 restoreListener 或 homeStack 任一不可用，就暂存到 pendingRestore，
  // 等 PrimaryTabShell 注册 homeStack 后由 tryConsumePendingRestore() 消费。
  restoreFromContinuation(payload: ContinuationPayload): void {
    const homeStack: NavPathStack | null = this.getTabStack(0)
    if (this.restoreListener === null || homeStack === null) {
      this.pendingRestore = payload
      return
    }
    this.restoreListener(payload)
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
