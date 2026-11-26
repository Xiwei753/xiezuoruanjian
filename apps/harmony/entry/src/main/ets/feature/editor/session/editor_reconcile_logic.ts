// editor_reconcile_logic.ts — 会话冲突恢复的纯逻辑层（Issue #1063 复审评论）。
//
// 这一层只保留两件事：
//   - 单段最小文本差量（computeTextDelta / applyTextDelta）：并发映射与撤销确认的
//     基础工具，坐标是 UTF-16 code unit；
//   - 冲突会话状态机 ReconcileSessionState：conflict 是确定结果，自动错误路径不能把它
//     降级成 reconciling；只有显式重试入口能重新尝试。
//
// 三方合并与 pendingEdits 逐笔重基已迁到 editor_merge_plan.ts：旧实现用一段公共前后缀
// diff 替代编辑事务确认，既没有核对 beforeText/oldText，也会把多笔离散编辑压成一段并
// 允许切到代理对内部（复审评论第 3 条）。新的映射必须保留每笔编辑事实与顺序。
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

// ── 单段最小文本差量（纯 UTF-16 坐标） ──

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
