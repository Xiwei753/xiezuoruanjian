// SyncMessagePayload — 同步消息结构化载荷的共享契约。
// 替代旧的 "sync_complete:${n}" / "error:${code}" 机械字符串，
// 原样传递 outcome.messageKey + errorCode + messageArgs + errorCategory + rawError + overallStatus。
//
// phase 使用 'success' | 'failed'，与 SyncOperationController.ets 的 SyncPhase 对齐
// （SyncPhase 是 'idle' | 'in_progress' | 'success' | 'failed'，本接口只用到成功/失败两个终态）。
// 构造 SyncOutcome 时，'failed' 可直接赋给 SyncPhase，不再出现 'error' 这个无效值。

export interface SyncMessagePayload {
  phase: 'success' | 'failed'
  messageKey?: string
  messageArgs?: Record<string, string>
  errorCode?: string
  errorCategory?: string
  rawError?: string
  totalDownloaded?: number
  overallStatus?: string
}
