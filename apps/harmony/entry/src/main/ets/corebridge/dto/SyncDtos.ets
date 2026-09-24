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
