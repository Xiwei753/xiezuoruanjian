// SyncRunTypes — Controller、展示快照与结果呈现共用的纯数据类型。

export type SyncPhase = 'idle' | 'in_progress' | 'success' | 'failed'

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
