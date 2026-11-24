// SyncRunTypes — Controller、展示快照与结果呈现共用的纯数据类型。

export type SyncPhase = 'idle' | 'in_progress' | 'success' | 'failed'

// 同步触发原因（Issue #1058）：设备切换（退后台保存完成 / 真实后台→前台）与首次 Core
// 就绪是独立于周期 timer 的同步原因，不能只靠「timer 到期」表达。
// - manual：顶栏 / 设置页 / 冲突面板等用户入口；
// - auto_timer：前台周期唤醒 timer 到期（仍按用户设置的间隔判断）；
// - startup：Native Core 首次初始化完成；
// - foreground_handoff：真实后台→前台切换；
// - background_handoff：退后台保存完成后的交接同步；
// - handoff_retry：网络恢复 / 前台恢复时重试未完成的交接。
export type SyncTriggerReason =
  'manual' | 'auto_timer' | 'startup' | 'foreground_handoff' | 'background_handoff' | 'handoff_retry'

// 交接类触发要绕过周期 timer 的间隔到期判断（enabled && autoSync、单一同步事务
// 与保存屏障仍然生效）；auto_timer / manual 维持原有语义。
export function isHandoffTrigger(trigger: SyncTriggerReason): boolean {
  return trigger === 'startup' || trigger === 'foreground_handoff' ||
    trigger === 'background_handoff' || trigger === 'handoff_retry'
}

export interface SyncOutcome {
  phase: SyncPhase
  message: string
  messageKey?: string
  messageArgs?: Record<string, string>
  operationId?: string
  failureStage?: string
  errorCategory?: string
  errorCode?: string
  rawError?: string
  totalDownloaded?: number
  totalUploaded?: number
  totalConflicts?: number
  overallStatus?: string
}
