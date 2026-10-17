// ─── Sync DTOs ───
// Issue #753：字段与 Core api/types/sync.rs 的 DTO 一字不差对齐。
// Core DTO 是唯一数据结构，Harmony FFI 只负责搬运。
// 线格式约定：
// - camelCase 结构体：SyncConfigDto / SyncStateDto / SyncConflictDto / SyncResultDto /
//   SyncPlanDto / SyncDiagnosticsResultDto，以及全量同步聚合 DTO。
// - `ProviderConfigDto` 是 internally tagged enum（`#[serde(tag = "type")]`），
//   变体名 snake_case、变体字段保持 Rust 字段名（remote_url），Core 侧无 rename_all_fields。
// - Rust `Option<T>` 未标 skip_serializing_if 的字段一定以 `null` 出现，
//   ArkTS 必须写成 `T | null`，不能写成可选属性。

export interface ProviderConfig {
  type: string  // "github"
  remote_url: string
  branch: string
  username: string
  transport: string
}

export interface SyncConfig {
  enabled: boolean
  activeProvider: string
  providerConfig: ProviderConfig | null
  autoSync: boolean
  syncIntervalSeconds: number
  hasNetworkPermission: boolean
  hasNetworkStatePermission: boolean
}

export interface SyncState {
  status: string
  lastSyncTime: number | null
  lastError: string | null
  conflicts: SyncConflict[] | null
}

export interface SyncConflict {
  localPath: string
  remotePath: string
  localHash: string
  remoteHash: string
  baseHash: string
  createdAt: number
  description: string
  kind: string
  remoteSnapshotPath: string | null
}

export interface SyncResult {
  status: string
  uploadedFiles: string[]
  downloadedFiles: string[]
  localDeletes: string[]
  remoteDeletes: string[]
  overwrittenFiles: string[]
  ignoredFiles: string[]
  conflicts: SyncConflict[]
  error: string | null
  errorCategory: string | null
  messageKey: string | null
  searchIndexRebuildError: string | null
}

export interface SyncDiagnosticsResult {
  success: boolean
  providerType: string
  hasNetworkPermission: boolean
  hasNetworkStatePermission: boolean
  networkState: string
  networkOk: boolean
  authOk: boolean
  remoteOk: boolean
  networkStatus: string
  authStatus: string
  errorCategory: string
  rawError: string | null
  providerDetails: string | null
}

// ── 全量同步 DTO（Issue #630 统一入口，与 Rust serde JSON 对齐） ──
// 全量同步 = App target + 所有 Project target 一次同步，一个全局 config + 一份全局凭据。
// overallStatus 线格式：idle/syncing/success/configured_not_tested/conflict/
// partial_conflict/recoverable_error/fatal_error/no_changes/latest_wins_applied。

export interface FullSyncResult {
  overallStatus: string
  targets: FullSyncTargetResult[]
  totalUploaded: number
  totalDownloaded: number
  totalLocalDeletes: number
  totalRemoteDeletes: number
  totalOverwritten: number
  totalIgnored: number
  totalConflicts: number
  error: string | null
  errorCategory: string | null
  messageKey: string | null
}

export interface FullSyncTargetResult {
  targetKind: string
  projectId: string | null
  remotePrefix: string
  result: SyncResult
}

export interface FullSyncDryRunResult {
  targets: FullSyncTargetPlan[]
  totalToUpload: number
  totalToDownload: number
  totalToDeleteLocal: number
  totalToDeleteRemote: number
  totalIgnored: number
  totalConflicts: number
}

export interface FullSyncTargetPlan {
  targetKind: string
  projectId: string | null
  remotePrefix: string
  plan: SyncPlan
}

export interface SyncPlan {
  filesToUpload: string[]
  filesToDownload: string[]
  filesToDeleteLocal: string[]
  filesToDeleteRemote: string[]
  ignoredFiles: string[]
  conflicts: string[]
}

export interface FullSyncDiagnosticsResult {
  diagnostics: SyncDiagnosticsResult
}

// ── Secrets DTO（Issue #780） ──
// ProviderSecrets 是 internally-tagged enum（`#[serde(tag = "type")]`），
// 当前唯一变体 "github"，字段保持 Rust 字段名（token）。
// SyncSecrets.providerSecrets 对应 Core 侧 `Option<ProviderSecretsDto>`，nullable。

export interface ProviderSecrets {
  type: string // "github"
  token: string
}

export interface SyncSecrets {
  providerSecrets: ProviderSecrets | null
}

// ── 全量同步持久状态 / 冲突总览与预览（Issue #925 / #926） ──
// 字段与 Core api/types/sync.rs 的 DTO 一一对应，FFI 只负责搬运：
// - FullSyncStateDto 没有 rename_all，线格式保持 Rust snake_case 字段名；
// - AllSyncConflictEntryDto / SyncConflictPreviewDto 为 camelCase。
// Rust `Option<T>` 未标 skip_serializing_if 时恒发该键（T | null），不要写成可选属性。

// Core `FullSyncStateDto`：full_state.local.json 中"最近一次全量同步事务"的总体结果。
// overall_status 线格式与 FullSyncResult.overallStatus 同一映射（idle/success/...）；
// last* 为 Unix 秒；failed_targets 是失败 target 标识（"app" / "project:<id>" / "global" / "preflight"）。
export interface FullSyncState {
  overall_status: string
  last_attempt_time: number | null
  last_success_time: number | null
  failed_targets: string[]
}

// Core `AllSyncConflictEntryDto`：全局未解决冲突摘要（list_all_sync_conflicts）。
// targetKind 为 "app" / "project"；App target 的 projectId / projectTitle 为空字符串。
export interface AllSyncConflictEntry {
  projectId: string
  projectTitle: string
  path: string
  kind: string
  createdAt: number
  targetKind: string
}

// Core `SyncConflictPreviewDto`：单条冲突的本地/远端内容预览。
// remoteDeleted 为 true 时 remoteContent 为 null（不伪造空字符串为远端正文）。
export interface SyncConflictPreview {
  path: string
  kind: string
  createdAt: number
  localContent: string
  remoteContent: string | null
  remoteDeleted: boolean
}
