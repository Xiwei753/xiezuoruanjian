// ime_settlement_barrier.ts — IME 结算屏障的纯逻辑层（Issue #1063 复审评论第 2 条）。
//
// 为什么单独抽出来：屏障的等待条件与超时生命周期不依赖 ArkUI / NAPI，但恰恰是
// 「退出屏障假成功」的根源：
//   - 旧实现 stopEditing() 后立刻 settleImeComposition(true)，force=true 跳过 previewText
//     检查，把镜像里的候选文本当正文提交；
//   - 旧实现只有一个共享 imeSettlementTimedOut 布尔，普通 whenIdle 会把它复位，
//     多个 waiter 共用一条超时路径，超时后把退出 waiter 当成功 resolve；
//   - previewText 读取失败被当成 0（已结算）。
//
// 现在：
//   - 每个 waiter 有独立 deadline/timer；超时 reject IME_SETTLEMENT_TIMEOUT，绝不 resolve。
//   - waiter 绑定 sessionId + 屏障 revision + 队列序号；会话结束/取消立即移除回调和 timer。
//   - 退出屏障（ime-settlement）只有在「提交的事务已进入 Core 且原生全文、镜像、Core 文本
//     在同一 revision 完全一致」或「preview 明确为空且正文未变」时才满足；preview 读取失败
//     （unknown）继续等待，最终由各自的 timer 超时失败。
//   - 普通保存屏障（committed-revision）允许只冻结已提交 Core revision，预编辑保留在
//     原生未提交层；它不检查 preview，也不会触碰退出屏障的超时。
//
// 生产代码：feature/editor/input/NativeRichEditorBridge.ets。
// 运行测试：node --experimental-strip-types __tests__/ime_settlement_barrier.test.mjs

export const IME_SETTLEMENT_TIMEOUT: string = 'IME_SETTLEMENT_TIMEOUT'
export const IME_SETTLEMENT_SESSION_CHANGED: string = 'IME_SETTLEMENT_SESSION_CHANGED'
export const IME_SETTLEMENT_CANCELLED: string = 'IME_SETTLEMENT_CANCELLED'
export const IME_SETTLEMENT_CONFLICT: string = 'IME_SETTLEMENT_CONFLICT'

// 会话健康状态机（healthy/reconciling/conflict）与可验证 rebase 在
// feature/editor/session/editor_reconcile_logic.ts；本文件只管屏障 waiter 的结算生命周期。

// previewText 的读取结果。unknown 绝不能当成 empty/已结算。
export type PreviewTextState = 'empty' | 'present' | 'unknown'

// committed-revision：普通保存；ime-settlement：同步/关闭/切章/手动结束输入。
export type ImeBarrierMode = 'committed-revision' | 'ime-settlement'

// 带明确错误码的屏障错误；调用方按 errorCode 区分超时/会话切换/冲突/取消。
export class ImeSettlementError extends Error {
  readonly errorCode: string

  constructor(errorCode: string, detail: string) {
    super(`${errorCode}: ${detail}`)
    this.name = 'ImeSettlementError'
    this.errorCode = errorCode
  }
}

// 一个屏障 waiter 的公开身份。requiredSequence 只在 IME commit 回调到达时推进，
// requireImeSignal 表示「还没有看到能证明 preview 结束的 commit/cancel 信号」。
export interface ImeBarrierWaiter {
  readonly id: number
  readonly sessionId: number
  readonly barrierRevision: number
  readonly mode: ImeBarrierMode
  requiredSequence: number
  requireImeSignal: boolean
}

export interface ImeBarrierRegistration {
  readonly id: number
  readonly promise: Promise<void>
}

interface ImeBarrierSlot {
  waiter: ImeBarrierWaiter
  resolve: () => void
  reject: (error: Error) => void
  // -1 表示这个 waiter 没有 timer（普通保存屏障不设超时）。
  timerId: number
}

// 屏障满足条件所需的三方事实（由 NativeRichEditorBridge 在 checkIdle 时读取）。
export interface ImeAlignmentFacts {
  readonly coreAvailable: boolean
  // 原生编辑器是否已绑定且已加载正文；false 时没有可结算的原生预编辑。
  readonly nativeEditorAvailable: boolean
  // 原生实际全文 === 本地镜像 === Core snapshot.text。
  readonly aligned: boolean
  readonly coreRevision: number
  readonly appliedRevision: number
  readonly coreText: string
  readonly confirmedText: string
}

export interface ImeStateFacts {
  readonly imeComposing: boolean
  readonly previewState: PreviewTextState
}

/**
 * waiter 是否已经可以结算。
 *
 * - committed-revision：只要求已提交 revision 稳定。仍有 IME 预编辑时，
 *   Core revision/text 与 confirmed 基线一致即可放行（预编辑留在原生未提交层）；
 *   没有预编辑时要求 native/mirror/core 三方一致。
 * - ime-settlement：必须已经有真实 commit/cancel 信号（requireImeSignal=false）、
 *   preview 明确为空、不在 composing，并且 Core 的 revision 与原生/Core 文本一致；
 *   三者同 revision 才放行。任何 unknown 都返回 false，等各自 timer 超时。
 */
export function isBarrierWaiterSatisfied(waiter: ImeBarrierWaiter, alignment: ImeAlignmentFacts,
  state: ImeStateFacts, appliedSequence: number): boolean {
  if (alignment.appliedRevision < waiter.barrierRevision) {
    return false
  }
  if (appliedSequence < waiter.requiredSequence) {
    return false
  }
  if (waiter.mode === 'committed-revision') {
    if (!state.imeComposing) {
      if (!alignment.coreAvailable || !alignment.nativeEditorAvailable) {
        return true
      }
      return alignment.aligned
    }
    return alignment.coreAvailable &&
      alignment.coreRevision === alignment.appliedRevision &&
      alignment.coreText === alignment.confirmedText
  }
  // ime-settlement：没有真实结算信号、仍在预编辑或 preview 读取失败都继续等待。
  if (waiter.requireImeSignal) {
    return false
  }
  if (state.imeComposing) {
    return false
  }
  if (state.previewState !== 'empty') {
    return false
  }
  if (!alignment.coreAvailable) {
    return false
  }
  if (!alignment.nativeEditorAvailable) {
    // 没有绑定的原生编辑器就没有未提交预编辑；此时只需队列已排空并由 timer 兜底。
    return true
  }
  return alignment.aligned && alignment.coreRevision === alignment.appliedRevision
}

/**
 * 每个 waiter 独立的屏障账本。
 *
 * 注意：本类只负责 waiter 生命周期（timer/结算/会话终止），是否满足由调用方用
 * isBarrierWaiterSatisfied 判定后调用 settle。这样 Node 测试不需要伪造 ArkUI。
 */
export class ImeSettlementBarrier {
  private readonly timeoutMs: number
  private slots: ImeBarrierSlot[] = []
  private nextId: number = 1

  constructor(timeoutMs: number) {
    this.timeoutMs = timeoutMs
  }

  /**
   * 注册一个 waiter。
   * - ime-settlement：立即挂独立 timer，超时 reject IME_SETTLEMENT_TIMEOUT。
   * - committed-revision：不挂 timer（普通保存不得因预编辑而伪造成败）。
   */
  register(sessionId: number, barrierRevision: number, requiredSequence: number,
    mode: ImeBarrierMode, requireImeSignal: boolean): ImeBarrierRegistration {
    let resolveWaiter: () => void = (): void => {}
    let rejectWaiter: (error: Error) => void = (): void => {}
    const promise: Promise<void> = new Promise<void>(
      (resolve: () => void, reject: (error: Error) => void): void => {
        resolveWaiter = resolve
        rejectWaiter = reject
      })
    const waiter: ImeBarrierWaiter = {
      id: this.nextId,
      sessionId: sessionId,
      barrierRevision: barrierRevision,
      mode: mode,
      requiredSequence: requiredSequence,
      requireImeSignal: requireImeSignal
    }
    this.nextId += 1
    const slot: ImeBarrierSlot = {
      waiter: waiter,
      resolve: resolveWaiter,
      reject: rejectWaiter,
      timerId: -1
    }
    if (mode === 'ime-settlement') {
      slot.timerId = setTimeout((): void => {
        this.rejectSlot(slot, new ImeSettlementError(IME_SETTLEMENT_TIMEOUT,
          `waiter ${waiter.id} session=${waiter.sessionId}`
            + ` barrierRevision=${waiter.barrierRevision} sequence=${waiter.requiredSequence}`
            + ` previewSignal=${waiter.requireImeSignal}`))
      }, this.timeoutMs)
    }
    this.slots.push(slot)
    return { id: waiter.id, promise: promise }
  }

  // 当前 waiter 快照（调用方只读；不要修改 waiter 字段）。
  waiters(): ImeBarrierWaiter[] {
    const result: ImeBarrierWaiter[] = []
    for (const slot of this.slots) {
      result.push(slot.waiter)
    }
    return result
  }

  pendingCount(): number {
    return this.slots.length
  }

  hasImeSettlementWaiter(sessionId: number): boolean {
    for (const slot of this.slots) {
      if (slot.waiter.sessionId === sessionId && slot.waiter.mode === 'ime-settlement') {
        return true
      }
    }
    return false
  }

  hasWaiter(waiterId: number): boolean {
    return this.findSlot(waiterId) !== null
  }

  // 真实 IME commit 回调：把这些退出 waiter 的 requiredSequence 推进到提交事务的序号，
  // 并清除「必须等信号」标记。只有 Core 应用并三方对齐后才会真正 settle。
  bindImeCommitSequence(sessionId: number, sequence: number): void {
    for (const slot of this.slots) {
      if (slot.waiter.sessionId !== sessionId || slot.waiter.mode !== 'ime-settlement') {
        continue
      }
      if (sequence > slot.waiter.requiredSequence) {
        slot.waiter.requiredSequence = sequence
      }
      slot.waiter.requireImeSignal = false
    }
  }

  // 真实 IME cancel 回调（preview 明确取消且正文未变）：不产生新事务，只清除信号标记。
  clearImeSignal(sessionId: number): void {
    for (const slot of this.slots) {
      if (slot.waiter.sessionId === sessionId && slot.waiter.mode === 'ime-settlement') {
        slot.waiter.requireImeSignal = false
      }
    }
  }

  // 结算单个 waiter：resolve 并立即移除 timer 与回调。
  settle(waiterId: number): boolean {
    const slot = this.findSlot(waiterId)
    if (slot === null) {
      return false
    }
    this.removeSlot(slot)
    slot.resolve()
    return true
  }

  // 会话结束/取消：只 reject 指定会话的 waiter，并清掉它们的 timer。
  rejectSession(sessionId: number, errorCode: string, detail: string): number {
    const error = new ImeSettlementError(errorCode, detail)
    const rejected: ImeBarrierSlot[] = []
    const remaining: ImeBarrierSlot[] = []
    for (const slot of this.slots) {
      if (slot.waiter.sessionId === sessionId) {
        rejected.push(slot)
      } else {
        remaining.push(slot)
      }
    }
    this.slots = remaining
    for (const slot of rejected) {
      this.clearTimer(slot)
      slot.reject(error)
    }
    return rejected.length
  }

  // 页面销毁/切章：所有 waiter 拿到确定失败；timer 与回调全部移除。
  rejectAll(errorCode: string, detail: string): number {
    const error = new ImeSettlementError(errorCode, detail)
    const rejected = this.slots
    this.slots = []
    for (const slot of rejected) {
      this.clearTimer(slot)
      slot.reject(error)
    }
    return rejected.length
  }

  private findSlot(waiterId: number): ImeBarrierSlot | null {
    for (const slot of this.slots) {
      if (slot.waiter.id === waiterId) {
        return slot
      }
    }
    return null
  }

  private rejectSlot(slot: ImeBarrierSlot, error: Error): void {
    if (this.findSlot(slot.waiter.id) === null) {
      return
    }
    this.removeSlot(slot)
    slot.reject(error)
  }

  private removeSlot(slot: ImeBarrierSlot): void {
    const index = this.slots.indexOf(slot)
    if (index >= 0) {
      this.slots.splice(index, 1)
    }
    this.clearTimer(slot)
  }

  private clearTimer(slot: ImeBarrierSlot): void {
    if (slot.timerId !== -1) {
      clearTimeout(slot.timerId)
      slot.timerId = -1
    }
  }
}
