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
  'manual' | 'auto_timer' | 'startup' | 'foreground_handoff' | 'background_handoff' | 'handoff_retry' |
  'durable_commit'

// 交接类触发要绕过周期 timer 的间隔到期判断（enabled && autoSync、单一同步事务
// 与保存屏障仍然生效）；auto_timer / manual 维持原有语义。
// Issue #1065：durable_commit 是"业务事务已 durable 落盘"的上传唤醒，它由调度器
// 按用户间隔合并后再发起，因此不在这里绕过间隔。
export function isHandoffTrigger(trigger: SyncTriggerReason): boolean {
  return trigger === 'startup' || trigger === 'foreground_handoff' ||
    trigger === 'background_handoff' || trigger === 'handoff_retry'
}

// Issue #1065：force_sync 只表示"跳过 Core 的调度防抖窗口"，不表示"重建索引"，
// 也不绕过本地/远端对象版本比对。用户手动入口与交接类触发跳过防抖；周期 timer
// 的到期判断已经由调度器完成，仍走 Core 防抖。
export function bypassSyncDebounce(trigger: SyncTriggerReason): boolean {
  return trigger === 'manual' || isHandoffTrigger(trigger)
}

// Issue #1065：业务事务 durable 提交后的上传唤醒事件。发布方只声明"有新的已落盘
// 事实"，由 HarmonyAutoSyncScheduler 按用户间隔合并成一次同步，不按保存次数同步。
export const SYNC_DURABLE_COMMIT_EVENT_ID: number = 1002

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
