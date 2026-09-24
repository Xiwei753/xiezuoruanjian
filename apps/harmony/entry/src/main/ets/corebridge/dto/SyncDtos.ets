// ─── Sync DTOs ───
// Issue #753：字段与 Core api/types/sync.rs 的 DTO 一字不差对齐。
// Core DTO 是唯一数据结构，Harmony FFI 只负责搬运。

export interface ProviderConfig {
  type: string  // "github"
  remoteUrl: string
  branch: string
  username: string
  transport: string
}

export interface SyncConfig {
  enabled: boolean
  activeProvider: string
  providerConfig?: ProviderConfig
  autoSync: boolean
  syncIntervalSeconds: number
  hasNetworkPermission: boolean
  hasNetworkStatePermission: boolean
}

export interface SyncState {
  status: string
  remoteUrl?: string
  backendType?: string
  transport?: string
  lastSyncedCommit?: string
  lastSyncTime?: number
  lastError?: string
  conflicts?: SyncConflict[]
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
  commitHash?: string
  error?: string
  errorCategory?: string
  firstSyncMode: string
  userMessage?: string
}

export interface SyncDiagnosticsResult {
  success: boolean
  backendType: string
  androidHasInternetPermission: boolean
  androidHasAccessNetworkStatePermission: boolean
  androidNetworkState: string
  tcpProbeOk: boolean
  tcpProbeStatus: string
  httpConnectProbeOk: boolean
  httpConnectProbeStatus: string
  libgit2ProbeOk: boolean
  libgit2ProbeStatus: string
  networkOk: boolean
  authOk: boolean
  repoOk: boolean
  branchOk: boolean
  networkStatus: string
  authStatus: string
  repoStatus: string
  branchStatus: string
  remoteUrlSanitized: string
  transport: string
  errorCategory: string
  userMessage?: string
  rawError?: string
  chosenNetworkMode?: string
  networkProbeSummary?: string
}

// ── 全量同步 DTO（Issue #630 统一入口，与 Rust serde JSON 对齐） ──
// 全量同步 = App target + 所有 Project target 一次同步，一个全局 config + 一份全局凭据。
// overallStatus 线格式：success/no_changes/latest_wins_applied/branch_missing_recovered/
// conflict/partial_conflict/recoverable_error/fatal_error/dirty_repo_blocked/error/syncing。

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
  error?: string
  errorCategory?: string
  messageKey?: string
}

export interface FullSyncTargetResult {
  targetKind: string
  projectId?: string
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
  projectId?: string
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
