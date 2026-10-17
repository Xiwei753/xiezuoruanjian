// ActiveWritingSessionRegistry — 活跃写作会话登记处（Issue #923）。
//
// 为什么需要它：
// 鸿蒙的同步入口（顶栏 / 设置 / 自动同步）直接调 SyncOperationController → NativeSyncBridge →
// Core.perform_full_sync，不先结算正在编辑的正文。Core 全量同步读磁盘，当前写作内容可能仍在
// TextEditSession 内存里；同步下载后 WritingScreen 未订阅变化，活跃会话可能继续持有同步前正文
// 并在下一次保存中覆盖下载版本。
//
// 边界：
// - 本类只负责「登记活跃写作会话」+「同步事务互斥」，不持有正文、不做 Core I/O。
// - 编辑正文真值仍在 EditorSessionCoordinator（Core snapshot）+ WritingWorkflowCoordinator 会话事务。
// - 同步事务的唯一入口是 withSyncBarrier：同步前让活跃会话落盘、传完再按 Core 落盘身份回填会话。
// - 无写作会话时直接允许同步（不阻塞后台同步）。

// 同步前保存屏障失败时，withSyncBarrier 抛出本错误，同步事务必须立即停止（不得进入 Core 同步）。
export const SYNC_PREFLIGHT_SAVE_FAILED: string = 'SYNC_PREFLIGHT_SAVE_FAILED'

export class SyncPreflightError extends Error {
  readonly errorCode: string

  constructor(errorCode: string = SYNC_PREFLIGHT_SAVE_FAILED) {
    super(errorCode)
    this.name = 'SyncPreflightError'
    this.errorCode = errorCode
  }
}

// 一个活跃写作会话对同步事务暴露的能力。实现方是 WritingWorkflowCoordinator：
// - beginSyncBarrier：结算 IME composition / 输入队列 / 精确快照落盘，并在传输期间封住输入，
//   防止继续往将被替换的 live 章节写旧内存内容；保存失败返回 ok:false（不进入同步）。
// - reconcileAfterSync：按 Core 落盘 contentHash 复核当前章节；变化时把同一编辑会话与新内容
//   重新对齐；发现未提交本地变化返回 false（明确冲突，不静默覆盖）。
// - endSyncBarrier：释放传输期间的输入封锁（有释放配对）。
export interface ActiveWritingSession {
  readonly chapterId: string
  beginSyncBarrier(): Promise<SyncBarrierBeginResult>
  reconcileAfterSync(): Promise<boolean>
  endSyncBarrier(): void
}

// beginSyncBarrier 结果：ok=true 才允许执行 Core 全量同步；contentHash 是放行时的落盘身份。
export interface SyncBarrierBeginResult {
  readonly ok: boolean
  readonly contentHash?: string
}

// withSyncBarrier 交给同步事务的回填入口。
// 注意：字段用箭头函数属性声明而不是方法签名——ArkTS 不允许用对象字面量实现接口的方法签名
// （arkts-no-untyped-obj-literals），属性形式的函数类型才能被字面量初始化。
export interface SyncBarrierScope {
  // 本次同步是否准备了活跃写作会话（false = 无写作会话，直接允许同步）。
  readonly activeSession: boolean
  // 同步完成后由同步事务调用：复核并回填活跃写作会话。
  // true = 会话已与 Core 落盘对齐（或无需对齐）；false = 明确冲突，调用方必须上报用户。
  readonly reconcileAfterSync: () => Promise<boolean>
}

export class ActiveWritingSessionRegistry {
  private static instance: ActiveWritingSessionRegistry | null = null

  // 已注册会话（栈序：最后一个是最新打开的写作页）。
  private sessions: ActiveWritingSession[] = []
  // 同步事务互斥：整笔同步（保存屏障 → 传输 → 回填 → 释放）期间只有一个持有者。
  private lifecycleHeld: boolean = false
  private lifecycleWaiters: Array<() => void> = []

  static getInstance(): ActiveWritingSessionRegistry {
    if (ActiveWritingSessionRegistry.instance === null) {
      ActiveWritingSessionRegistry.instance = new ActiveWritingSessionRegistry()
    }
    return ActiveWritingSessionRegistry.instance
  }

  // 页面 aboutToAppear 注册，返回注销函数；aboutToDisappear 只注销自身 token。
  // 用 token 精确移除本会话（不按数量清空），避免切章时旧页面销毁把新页面注册移除。
  register(session: ActiveWritingSession): () => void {
    this.sessions.push(session)
    return (): void => {
      const index = this.sessions.indexOf(session)
      if (index >= 0) {
        this.sessions.splice(index, 1)
      }
    }
  }

  hasActiveSession(): boolean {
    return this.currentSession() !== null
  }

  private currentSession(): ActiveWritingSession | null {
    return this.sessions.length > 0 ? this.sessions[this.sessions.length - 1] : null
  }

  /**
   * 同步事务唯一入口：所有手动 / 自动同步都必须从这里进入。
   * - 建立整笔同步的互斥（前一笔完全释放后才开始下一笔）。
   * - 有活跃写作会话时先做保存屏障；保存失败抛 SyncPreflightError，不进入 Core 同步。
   * - 无论 action 成功、失败还是抛错，都在 finally 里释放会话屏障与互斥。
   *
   * syncAction 内应通过 scope.reconcileAfterSync() 在拿到 Core 终态后回填活跃会话。
   */
  async withSyncBarrier<T>(syncAction: (scope: SyncBarrierScope) => Promise<T>): Promise<T> {
    await this.acquireLifecycle()
    const session: ActiveWritingSession | null = this.currentSession()
    try {
      if (session !== null) {
        const prepared: SyncBarrierBeginResult = await session.beginSyncBarrier()
        if (!prepared.ok) {
          throw new SyncPreflightError(SYNC_PREFLIGHT_SAVE_FAILED)
        }
      }
      const scope: SyncBarrierScope = {
        activeSession: session !== null,
        reconcileAfterSync: (): Promise<boolean> => {
          if (session === null) {
            return Promise.resolve(true)
          }
          return session.reconcileAfterSync()
        }
      }
      return await syncAction(scope)
    } finally {
      if (session !== null) {
        session.endSyncBarrier()
      }
      this.releaseLifecycle()
    }
  }

  private acquireLifecycle(): Promise<void> {
    if (!this.lifecycleHeld) {
      this.lifecycleHeld = true
      return Promise.resolve()
    }
    return new Promise<void>((resolve: () => void): void => {
      this.lifecycleWaiters.push(resolve)
    })
  }

  private releaseLifecycle(): void {
    const next: (() => void) | undefined = this.lifecycleWaiters.shift()
    if (next !== undefined) {
      // 所有权直接移交给下一个等待者，保持 held。
      next()
      return
    }
    this.lifecycleHeld = false
  }
}
