// ─── Envelope DTOs ───
// Issue #753：envelope 的唯一来源是 Core `ffi/mod.rs` 的 `ok_json` / `err_json`：
//   成功 → {"success":true,"data":<Core DTO>}
//   失败 → {"success":false,"errorCode":<string>,"userMessage":<string>}
// 平台端不得再发明 Core 从不发出的字段（原先的 messageKey / messageArgs /
// warnings / changedPaths / changedEntities / rawError 全部删除——它们运行时
// 恒为 undefined，UI 读出来的就是空提示）。
// `data` 只在成功时出现，且必须是各域 decoder 校验过的强类型；
// decoder 不通过时 Bridge 返回 SCHEMA_MISMATCH，绝不让 undefined 流到 UI。

import { ProjectSummary } from './ProjectDtos'

export interface ResultEnvelope<T> {
  success: boolean
  data?: T
  errorCode?: string
  userMessage?: string
}

// ─── Error Codes ───
// 前四个是 ArkTS Bridge 自己构造的（JSON 解析失败 / decoder 契约不符 /
// native 调用抛异常 / 未实现），其余必须与 Core ffi/*.rs 里 err_json 的错误码一致。

export type ErrorCode =
  // ArkTS bridge 层
  | 'PARSE_ERROR'
  | 'SCHEMA_MISMATCH'
  | 'NATIVE_ERROR'
  | 'NOT_IMPLEMENTED'
  // Core ffi
  | 'APP_STATE_ERROR'
  | 'BUILTIN_THEMES_ERROR'
  | 'CHAPTER_ALREADY_EXISTS'
  | 'CHAPTER_NOT_FOUND'
  | 'DEVICE_INFO_ERROR'
  | 'EDITOR_SESSION_CLOSE_FAILED'
  | 'EDITOR_SESSION_ERROR'
  | 'EMPTY_OVERWRITE_BLOCKED'
  | 'INVALID_ARG'
  | 'INVALID_ARGUMENT'
  | 'IO_READ_ERROR'
  | 'IO_WRITE_ERROR'
  | 'PALETTE_DELETE_ERROR'
  | 'PALETTE_LIST_ERROR'
  | 'PALETTE_LOAD_ERROR'
  | 'PROJECT_ALREADY_EXISTS'
  | 'PROJECT_NOT_FOUND'
  | 'SEARCH_ERROR'
  | 'SETTINGS_INVALID'
  | 'SETTINGS_NOT_FOUND'
  | 'STARMAP_ALREADY_EXISTS'
  | 'STARMAP_ERROR'
  | 'STARMAP_NOT_FOUND'
  | 'SYNC_NETWORK_ERROR'
  | 'SYNC_STATE_ERROR'
  | 'UNKNOWN_ERROR'
  | 'VOLUME_ALREADY_EXISTS'
  | 'VOLUME_NOT_FOUND'

// ─── App State DTOs ───

// RecentEditDto
export interface RecentEdit {
  projectId: string
  volumeId: string
  chapterId: string
  timestamp: string
}

// AppStateSummaryDto（writer_core_get_app_state）
export interface AppSummary {
  projects: ProjectSummary[]
  // #732 评论第5节：首页契约 singular — 最近一次编辑（Core 恒发出该键，可为 null）。
  recentEdit: RecentEdit | null
}
