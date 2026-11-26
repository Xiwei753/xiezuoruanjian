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
//   - 每个 waiter 有独立 deadline/timer；超时 reject，绝不 resolve
//     （退出屏障 IME_SETTLEMENT_TIMEOUT，普通保存 SAVE_BARRIER_TIMEOUT）。
//   - waiter 绑定 sessionId + Core sessionId + 屏障 revision + 队列序号；
//     会话结束/取消立即移除回调和 timer，Core 会话不一致也不能结算。
//   - 退出屏障（ime-settlement）只有在「提交的事务已进入 Core 且原生全文、镜像、Core 文本
//     在同一 revision 完全一致」或「preview 明确为空且正文未变」时才满足；preview 读取失败
//     （unknown）继续等待，最终由各自的 timer 超时失败。
//   - 普通保存屏障（committed-revision）允许只冻结已提交 Core revision，预编辑保留在
//     原生未提交层；但它也必须能区分「原生状态未知」（loading/read-failed 不得成功），
//     并且 Core snapshot 不可用绝不成功。
//
// 生产代码：feature/editor/input/NativeRichEditorBridge.ets。
// 运行测试：node --experimental-strip-types __tests__/ime_settlement_barrier.test.mjs

export const IME_SETTLEMENT_TIMEOUT: string = 'IME_SETTLEMENT_TIMEOUT'
export const IME_SETTLEMENT_SESSION_CHANGED: string = 'IME_SETTLEMENT_SESSION_CHANGED'
export const IME_SETTLEMENT_CANCELLED: string = 'IME_SETTLEMENT_CANCELLED'
export const IME_SETTLEMENT_CONFLICT: string = 'IME_SETTLEMENT_CONFLICT'
// 普通保存屏障（committed-revision）的独立超时：它也不能永远挂着。
// 超时只让本次保存明确失败，会话/正文/pendingEdits 原样保留，用户下一次保存可重试。
export const SAVE_BARRIER_TIMEOUT: string = 'SAVE_BARRIER_TIMEOUT'
export const SAVE_BARRIER_TIMEOUT_MS: number = 3000
// Core 会话存在但已读不到快照：没有可对齐的基线，等待者必须拿到明确失败而不是静默挂起。
export const CORE_SNAPSHOT_UNAVAILABLE: string = 'CORE_SNAPSHOT_UNAVAILABLE'

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
  // 桥接本地会话令牌（beginSession 递增，用于会话替换时立即 reject）。
  readonly sessionId: number
  // 注册时的 Core 会话 id（-1 表示注册时没有 Core 会话）；屏障只能由同一 Core 会话结算。
  readonly coreSessionId: number
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
  // -1 表示这个 waiter 没有 timer（当前只有显式结算路径；正常注册都会挂独立 timer）。
  timerId: number
}

// 原生编辑器的可用状态。unknown（读取失败）绝不能当成「没有原生编辑器可对齐」。
export type NativeEditorState = 'unbound' | 'loading' | 'ready' | 'read-failed'

// 屏障满足条件所需的三方事实（由 NativeRichEditorBridge 在 checkIdle 时读取）。
export interface ImeAlignmentFacts {
  readonly coreAvailable: boolean
  // 原生编辑器是否已绑定、已加载正文并可读出全文（unbound/loading/ready/read-failed）。
  readonly nativeEditorState: NativeEditorState
  // 原生实际全文 === 本地镜像 === Core snapshot.text。
  readonly aligned: boolean
  readonly coreRevision: number
  readonly appliedRevision: number
  readonly coreText: string
  readonly confirmedText: string
  // 这些事实来自哪个 Core 会话（-1 表示未知）；与 waiter 注册时的 coreSessionId 必须一致。
  readonly coreSessionId: number
}

export interface ImeStateFacts {
  readonly imeComposing: boolean
  readonly previewState: PreviewTextState
}

/**
 * waiter 是否已经可以结算。
 *
 * 公共前置（两种模式都适用）：
 *   - Core revision 与队列序号都已推进到 waiter 要求的位置；
 *   - Core snapshot 可用（coreAvailable=false 绝不能成功）；
 *   - 事实来自 waiter 注册时绑定的同一个 Core 会话。
 *
 * - committed-revision：只冻结已提交 revision，不等预编辑结束。
 *   loading/read-failed（原生状态未知）不得成功；unbound 只要求 coreRevision === appliedRevision
 *   （没有原生编辑器可对齐，由 workflow 处理「无活动会话」）；ready 时仍在 composing 要求
 *   Core revision/正文等于 confirmed 基线，否则要求三方对齐。
 * - ime-settlement：必须已经有真实 commit/cancel 信号（requireImeSignal=false）、
 *   preview 明确为空、不在 composing；任何 unknown 都返回 false，等各自 timer 超时。
 *   unbound 仍要满足上面的队列/Core 前置，不在这里无条件放行。
 */
export function isBarrierWaiterSatisfied(waiter: ImeBarrierWaiter, alignment: ImeAlignmentFacts,
  state: ImeStateFacts, appliedSequence: number): boolean {
  if (alignment.appliedRevision < waiter.barrierRevision) {
    return false
  }
  if (appliedSequence < waiter.requiredSequence) {
    return false
  }
  if (!alignment.coreAvailable) {
    return false
  }
  // Core 会话未知（-1）绝不匹配：两个 unknown 不能互相证明是同一会话。
  if (alignment.coreSessionId < 0 || waiter.coreSessionId < 0) {
    return false
  }
  if (alignment.coreSessionId !== waiter.coreSessionId) {
    return false
  }
  if (alignment.nativeEditorState === 'loading' || alignment.nativeEditorState === 'read-failed') {
    // 原生状态未知：既不能证明没有预编辑，也不能证明三方一致。
    return false
  }
  if (waiter.mode === 'committed-revision') {
    if (alignment.nativeEditorState === 'unbound') {
      return alignment.coreRevision === alignment.appliedRevision
    }
    if (state.imeComposing) {
      // 仍有未提交预编辑：只冻结 Core 已提交 revision 与 confirmed 基线。
      return alignment.coreRevision === alignment.appliedRevision &&
        alignment.coreText === alignment.confirmedText
    }
    return alignment.aligned
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
  if (alignment.nativeEditorState === 'unbound') {
    // 没有绑定的原生编辑器就没有未提交预编辑；队列序号/Core 会话已由公共前置证明，
    // 仍由 waiter 自己的 timer 兜底，避免队列不推进时无限等待。
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
  private readonly saveTimeoutMs: number
  private slots: ImeBarrierSlot[] = []
  private nextId: number = 1

  constructor(timeoutMs: number, saveTimeoutMs: number = SAVE_BARRIER_TIMEOUT_MS) {
    this.timeoutMs = timeoutMs
    this.saveTimeoutMs = saveTimeoutMs
  }

  /**
   * 注册一个 waiter。
   * - ime-settlement：立即挂独立 timer，超时 reject IME_SETTLEMENT_TIMEOUT。
   * - committed-revision：同样挂独立 timer（默认 3s），超时 reject SAVE_BARRIER_TIMEOUT；
   *   普通保存不得因为一次读取失败/加载未完成永远挂着，也不能把超时当成功。
   */
  register(sessionId: number, coreSessionId: number, barrierRevision: number,
    requiredSequence: number, mode: ImeBarrierMode, requireImeSignal: boolean): ImeBarrierRegistration {
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
      coreSessionId: coreSessionId,
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
          `waiter ${waiter.id} session=${waiter.sessionId} coreSession=${waiter.coreSessionId}`
            + ` barrierRevision=${waiter.barrierRevision} sequence=${waiter.requiredSequence}`
            + ` previewSignal=${waiter.requireImeSignal}`))
      }, this.timeoutMs)
    } else {
      slot.timerId = setTimeout((): void => {
        this.rejectSlot(slot, new ImeSettlementError(SAVE_BARRIER_TIMEOUT,
          `waiter ${waiter.id} session=${waiter.sessionId} coreSession=${waiter.coreSessionId}`
            + ` barrierRevision=${waiter.barrierRevision} sequence=${waiter.requiredSequence}`))
      }, this.saveTimeoutMs)
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
