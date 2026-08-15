// 本文件定义鸿蒙端 DTO，字段与 Core api/types/ 真实 DTO 对齐。
// Core struct 字段使用 serde(rename_all = "camelCase")，JSON 输出为 camelCase。
// Core enum 变体使用 PascalCase（如 "Back"、"TopLeading"、"SinglePane"），与 Rust 枚举名一致。
// 如需新增字段，必须先在 Core api/types/ 中定义，不得自行发明。

import { Project } from './ProjectDtos'

// ─── Envelope DTOs ───

export interface ResultEnvelope<T> {
  success: boolean
  data?: T
  errorCode?: string
  messageKey?: string
  messageArgs?: Record<string, string>
  userMessage?: string
  rawError?: string
  warnings: string[]
  changedPaths: string[]
  changedEntities: ChangedEntity[]
}

export interface ChangedEntity {
  entityType: string
  entityId?: string
}

// ─── Error Codes ───
// 与 Core WriterError 错误码对齐

export type ErrorCode =
  | 'IO_ERROR'
  | 'JSON_ERROR'
  | 'PROJECT_NOT_FOUND'
  | 'VOLUME_NOT_FOUND'
  | 'CHAPTER_NOT_FOUND'
  | 'STARMAP_NOT_FOUND'
  | 'EMPTY_OVERWRITE_BLOCKED'
  | 'NOT_IMPLEMENTED'
  | 'REFUSE_DELETE_ROOT'
  | 'INVALID_DELETE_TARGET'
  | 'SYNC_CONFLICT'
  | 'SYNC_FAILED'
  | 'OTHER'

// ─── App State DTOs ───

export interface RecentEdit {
  projectId: string
  volumeId: string
  chapterId: string
  timestamp: string
}

export interface AppSummary {
  projects: Project[]
  recentEdits: RecentEdit[]
}
