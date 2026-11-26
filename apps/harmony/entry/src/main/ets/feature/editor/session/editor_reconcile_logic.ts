// editor_reconcile_logic.ts — 会话冲突恢复的纯逻辑层（Issue #1063 复审评论）。
//
// 为什么单独抽出来：reconcile 既不能要求「模拟结果逐字等于原生全文」（那样 Core 与原生
// 各自修改不同位置时永远失败），也不能凭 Core 当前 CAS 或「镜像==confirmed」就把未经确认
// 的原生正文当权威。这里把三方合并与冲突会话状态机抽成不依赖 ArkUI 的纯逻辑：
//   - 共同基线 = confirmedText（最后一次已确认的 Core 正文）；
//   - Core 当前正文与原生实际正文分别是另外两侧；
//   - 只有一侧变化时正常收敛；两侧都变化时先验证各自的 delta 能复现输入文本，再要求
//     编辑区间不重叠，重叠（含同点不同插入）才进入真实冲突；
//   - 冲突时调用方保留全部 pendingEdits 与原生正文，并把两份正文交给既有 Core 冲突材料
//     机制持久保留（本文件只提供稳定的 CAS 不匹配版本哨兵，不新增持久化流程）。
//
// 生产代码：feature/editor/input/NativeRichEditorBridge.ets。
// 运行测试：node --experimental-strip-types __tests__/editor_reconcile_logic.test.mjs

export type EditorSessionHealth = 'healthy' | 'reconciling' | 'conflict'

// Issue #1063 复审评论第 2 条：会话内冲突重试结果。
// recovered=false 时会话仍保持 conflict（或保持 reconciling 等真实结算），
// errorCode 给出 NOT_IN_CONFLICT / RECONCILE_DEFERRED / IME_SETTLEMENT_CONFLICT /
// IME_SETTLEMENT_SESSION_CHANGED 等明确原因。
export interface ConflictRecoveryResult {
  readonly recovered: boolean
  readonly errorCode: string
  readonly detail: string
}

/**
 * 屏障/保存入口前的冲突恢复判定：只有真正恢复，或本来就不在冲突中（NOT_IN_CONFLICT，
 * 例如会话仍在 reconciling，等待交给后面的屏障与超时）才允许继续。
 *
 * 其余结果（RECONCILE_DEFERRED、会话切换、不可合并冲突）都按失败收尾：
 * 调用方走既有保存失败状态，原生正文与 pendingEdits 一条不丢。
 */
export function isConflictRecoverySatisfied(result: ConflictRecoveryResult | null): boolean {
  if (result === null) {
    return true
  }
  return result.recovered === true || result.errorCode === 'NOT_IN_CONFLICT'
}

// ── 三方正文合并（纯 UTF-16 坐标） ──

/**
 * 单段最小差量：`base[start, end)` → `replacement`。
 *
 * 用公共前后缀求出唯一一段变化区间；坐标是 UTF-16 code unit，落到代理对中间时由调用方
 * 用原文子串拼接，不会产生非法 UTF-8。多段编辑会被合并成一段更大的区间，这只影响合并
 * 成功率（更保守），不会把内容改错。
 */
export interface TextDelta {
  start: number
  end: number
  replacement: string
}

export function computeTextDelta(base: string, text: string): TextDelta {
  const maxPrefix: number = Math.min(base.length, text.length)
  let start: number = 0
  while (start < maxPrefix && base.charCodeAt(start) === text.charCodeAt(start)) {
    start += 1
  }
  const maxSuffix: number = maxPrefix - start
  let suffix: number = 0
  while (suffix < maxSuffix &&
      base.charCodeAt(base.length - 1 - suffix) === text.charCodeAt(text.length - 1 - suffix)) {
    suffix += 1
  }
  return {
    start: start,
    end: base.length - suffix,
    replacement: text.substring(start, text.length - suffix)
  }
}

/** 把单段差量应用到原文；用于验证 delta 确实复现目标文本。 */
export function applyTextDelta(base: string, delta: TextDelta): string {
  return base.substring(0, delta.start) + delta.replacement + base.substring(delta.end)
}

export type ThreeWayMergeKind =
  'converged'   // Core 与原生已逐字一致
  | 'core-only' // 只有 Core 前进，原生仍是已确认基线
  | 'native-only' // 只有原生有未确认差量，Core 仍是已确认基线
  | 'merged'    // 两侧都有变化且互不重叠，merged 是合并结果
  | 'conflict'  // 两侧改动重叠，必须保留双方正文

export interface ThreeWayMergeResult {
  ok: boolean
  kind: ThreeWayMergeKind
  // ok=true 时必须写入 Core 的目标正文；ok=false 时保持 Core 当前正文（诊断用）。
  merged: string
  reason: string
}

/**
 * 以 confirmedText 为共同基线做三方合并。
 *
 * 判定顺序：
 *   1. Core === native → converged；
 *   2. native === base → core-only（调用方在确认原生实际全文仍等于 base 后才能回填）；
 *   3. Core === base → native-only（调用方把 native 提交为新的 Core 正文）；
 *   4. 两侧都有变化：先验证 base+nativeDelta===native、base+coreDelta===core，
 *      区间不重叠 → 把 nativeDelta 映射到 Core 坐标后
 *      得到 merged；其余（重叠/同点不同插入）→ conflict。
 *
 * 不要求模拟结果逐字等于 native 之外的任何中间态；只要最终合并结果可证明包含双方编辑。
 */
export function planThreeWayReconcile(base: string, core: string, native: string): ThreeWayMergeResult {
  if (core === native) {
    return { ok: true, kind: 'converged', merged: core, reason: '' }
  }
  if (native === base) {
    return { ok: true, kind: 'core-only', merged: core, reason: '' }
  }
  if (core === base) {
    return { ok: true, kind: 'native-only', merged: native, reason: '' }
  }

  const coreDelta: TextDelta = computeTextDelta(base, core)
  const nativeDelta: TextDelta = computeTextDelta(base, native)
  // 防御性：delta 必须逐字复现输入，否则说明 diff 实现有误，按冲突处理。
  if (applyTextDelta(base, coreDelta) !== core || applyTextDelta(base, nativeDelta) !== native) {
    return {
      ok: false,
      kind: 'conflict',
      merged: core,
      reason: 'delta verification failed'
    }
  }

  const sameRange: boolean = coreDelta.start === nativeDelta.start && coreDelta.end === nativeDelta.end
  if (sameRange) {
    // 同一区间同一替换必然 core === native（已在函数开头收敛）；到这里的只能是不同改法。
    return {
      ok: false,
      kind: 'conflict',
      merged: core,
      reason: `same range [${coreDelta.start}, ${coreDelta.end}) edited differently`
    }
  }

  // 映射到 Core 坐标：Core 的改动区间整体在原生改动之前时，原生区间整体右移。
  if (coreDelta.end <= nativeDelta.start) {
    const shift: number = coreDelta.replacement.length - (coreDelta.end - coreDelta.start)
    const mappedStart: number = nativeDelta.start + shift
    const mappedEnd: number = nativeDelta.end + shift
    if (mappedStart < 0 || mappedEnd > core.length) {
      return {
        ok: false,
        kind: 'conflict',
        merged: core,
        reason: `mapped native delta out of bounds [${mappedStart}, ${mappedEnd}) coreLen=${core.length}`
      }
    }
    return {
      ok: true,
      kind: 'merged',
      merged: applyTextDelta(core, {
        start: mappedStart,
        end: mappedEnd,
        replacement: nativeDelta.replacement
      }),
      reason: ''
    }
  }
  if (nativeDelta.end <= coreDelta.start) {
    return {
      ok: true,
      kind: 'merged',
      merged: applyTextDelta(core, nativeDelta),
      reason: ''
    }
  }
  return {
    ok: false,
    kind: 'conflict',
    merged: core,
    reason: `overlapping edits core=[${coreDelta.start}, ${coreDelta.end})`
      + ` native=[${nativeDelta.start}, ${nativeDelta.end})`
  }
}

/**
 * 触发 Core 既有章节提交冲突材料的版本哨兵。
 *
 * `ChapterCommitRequest.expected` 的 CAS 比较要求 object_generation 与 content_sha256
 * 非空；这里故意使用空 sha256 与 revision 0，使任意真实版本都不可能匹配，Core 会把
 * 磁盘当前正文（local.md）、原生未确认正文（incoming.md）与 baseContent（base.md）
 * 写入既有 conflict_dir 后返回 CHAPTER_VERSION_CONFLICT，不会写正文。
 *
 * 调用方必须只在会话进入真实冲突时使用，并核对返回错误码是 CHAPTER_VERSION_CONFLICT；
 * 这是 #1062 持久提交所有权下的既有接口，不新增另一套持久化流程。
 */
export interface EditorConflictVersionSentinel {
  objectGeneration: string
  revision: number
  contentSha256: string
}

export function editorConflictVersionSentinel(): EditorConflictVersionSentinel {
  return {
    objectGeneration: 'editor-conflict-material',
    revision: 0,
    contentSha256: ''
  }
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
