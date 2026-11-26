// editor_reconcile_logic.ts — 会话冲突恢复的纯逻辑层（Issue #1063 复审评论第 2 条）。
//
// 为什么单独抽出来：复审指出 reconcile 的 conflict 仍是只能 beginSession 解锁的终态，
// 而 beginSession 会清空 pendingEdits，真实冲突就成了「丢事件或换会话」二选一。
// 本文件把「从 Core 当前 snapshot 与原生实际正文做可验证 rebase」和「冲突会话状态机」
// 抽成不依赖 ArkUI 的纯逻辑：
//   - 冲突时保留全部 pendingEdits 与每条 edit 的 beforeText 基线；
//   - retryReconcile() 只是把状态从 conflict 切到 reconciling，再重新跑本文件的模拟；
//   - 只有模拟成功、且结果与原生全文逐字一致时，调用方才用 consumedEdits 丢弃已确认事件；
//   - 模拟失败返回 consumedEdits=0，调用方必须保持 conflict 与全部 pendingEdits 不变。
//
// 生产代码：feature/editor/input/NativeRichEditorBridge.ets。
// 运行测试：node --experimental-strip-types __tests__/editor_reconcile_logic.test.mjs

import { computeMinimalTextDiff } from '../session/editor_patch_logic.ts'

export type EditorSessionHealth = 'healthy' | 'reconciling' | 'conflict'

// 冻结 pending edit 做 rebase 所需的最小事实（结构与 NativeRichEditorBridge.PendingEdit 兼容）。
export interface FrozenEditFact {
  localSequence: number
  beforeText: string
  startUtf16: number
  endUtf16: number
  insertedText: string
  oldText: string
}

export interface EditRangeMap {
  startUtf16: number
  endUtf16: number
}

export interface FrozenRebaseAttempt {
  // true 表示模拟成功；调用方还必须核对 text === 原生全文才能提交 Core。
  recovered: boolean
  // 模拟应用后的正文；失败时为进入本次尝试时的 Core 正文。
  text: string
  // 非空是明确失败原因（重叠/oldText 不一致），直接进入/保持 conflict。
  reason: string
  // 成功时调用方可以丢弃的已确认 edit 数；失败必须是 0（保留全部事件）。
  consumedEdits: number
}

// 把一条冻结 edit 的原始 UTF-16 范围映射到给定基线正文。
// 基线未变时直接返回原范围；基线变化时必须能证明 edit 范围整体落在 diff 的一侧。
export function mapEditRangeToBaseline(edit: FrozenEditFact, baselineText: string): EditRangeMap | null {
  if (baselineText === edit.beforeText) {
    return { startUtf16: edit.startUtf16, endUtf16: edit.endUtf16 }
  }
  const diff = computeMinimalTextDiff(edit.beforeText, baselineText)
  const diffLenDelta = diff.replacement.length - (diff.end - diff.start)
  if (diff.end <= edit.startUtf16) {
    return { startUtf16: edit.startUtf16 + diffLenDelta, endUtf16: edit.endUtf16 + diffLenDelta }
  }
  if (diff.start >= edit.endUtf16) {
    return { startUtf16: edit.startUtf16, endUtf16: edit.endUtf16 }
  }
  return null
}

/**
 * 按 edit 原始顺序把冻结 pendingEdits 映射到 Core 当前正文并模拟应用。
 *
 * 成功返回模拟正文与 consumedEdits=frozen.length；失败返回 consumedEdits=0。
 * 调用方只能在 simulated.text === 原生实际全文且 Core 事务成功后才丢弃前 consumedEdits 条。
 */
export function attemptFrozenRebase(frozen: FrozenEditFact[], coreText: string): FrozenRebaseAttempt {
  let simulated: string = coreText
  for (let i = 0; i < frozen.length; i++) {
    const edit: FrozenEditFact = frozen[i]
    const mapped: EditRangeMap | null = mapEditRangeToBaseline(edit, simulated)
    if (mapped === null) {
      return {
        recovered: false,
        text: coreText,
        reason: `cannot map frozen edit #${edit.localSequence} at offset ${edit.startUtf16}: overlapping diff`,
        consumedEdits: 0
      }
    }
    const slice: string = simulated.substring(mapped.startUtf16, mapped.endUtf16)
    if (slice !== edit.oldText) {
      return {
        recovered: false,
        text: coreText,
        reason: `cannot map frozen edit #${edit.localSequence}: oldText mismatch`
          + ` expectedLen=${edit.oldText.length} actualLen=${slice.length}`,
        consumedEdits: 0
      }
    }
    simulated = simulated.substring(0, mapped.startUtf16) + edit.insertedText
      + simulated.substring(mapped.endUtf16)
  }
  return { recovered: true, text: simulated, reason: '', consumedEdits: frozen.length }
}

/**
 * 冲突会话状态机。
 *
 * 与旧实现的区别：conflict 不再是只能 beginSession 解锁的终态。
 * - beginManualRetry() 允许从 conflict 回到 reconciling，pendingEdits/原生正文都由调用方保留；
 * - 重试失败调用 enterConflict()，给出新的原因，事件一条不丢；
 * - 成功调用 confirmHealthy()，清 conflict 与诊断原因；
 * - 自动错误路径不能把 conflict 降级成 reconciling（enterReconciling 返回 false），
 *   避免旧闩锁换成终态枚举后又出现自动「假恢复」。
 */
export class ReconcileSessionState {
  private health: EditorSessionHealth = 'healthy'
  private conflictDetail: string | null = null
  private reconcileReason: string = ''

  getHealth(): EditorSessionHealth {
    return this.health
  }

  getConflictDetail(): string | null {
    return this.conflictDetail
  }

  getReconcileReason(): string {
    return this.reconcileReason
  }

  isConflict(): boolean {
    return this.health === 'conflict'
  }

  // 自动错误路径：conflict 是确定结果，不允许被普通失败再次降级。
  enterReconciling(reason: string): boolean {
    if (this.health === 'conflict') {
      return false
    }
    this.health = 'reconciling'
    this.reconcileReason = reason
    this.conflictDetail = null
    return true
  }

  enterConflict(reason: string): void {
    this.health = 'conflict'
    this.conflictDetail = reason
    this.reconcileReason = ''
  }

  // 会话内显式重试入口：只有 conflict 才需要。保留 conflictDetail 直到本次重试给出结论。
  beginManualRetry(reason: string): boolean {
    if (this.health !== 'conflict') {
      return false
    }
    this.health = 'reconciling'
    this.reconcileReason = reason
    return true
  }

  confirmHealthy(): void {
    this.health = 'healthy'
    this.conflictDetail = null
    this.reconcileReason = ''
  }

  reset(): void {
    this.health = 'healthy'
    this.conflictDetail = null
    this.reconcileReason = ''
  }
}
