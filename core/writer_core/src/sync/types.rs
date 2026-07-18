use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SyncErrorCategory {
    #[default]
    None,
    TokenMissing,
    TokenInvalid,
    TokenPermissionDenied,
    AuthError,
    RepoNotFoundOrNoPermission,
    GithubUnauthorized,
    GithubForbidden,
    EmptyUrl,
    MissingPermission,
    NetworkProbeFailed,
    GithubNetworkFailed,
    DnsFailed,
    TlsFailed,
    BranchMissing,
    RemoteBranchMissing,
    NotFound,
    FileNotFound,
    NonFastForward,
    Conflict,
    CheckoutConflict,
    LocalBlockingFile,
    UnrelatedHistories,
    LocalIoError,
    ApiRateLimited,
    ApiError,
    DirtyRepo,
    Other,
}


impl SyncErrorCategory {
    pub fn to_ui_status(&self) -> &'static str {
        match self {
            SyncErrorCategory::None => "error",
            SyncErrorCategory::TokenMissing => "token_missing",
            SyncErrorCategory::TokenInvalid => "token_invalid",
            SyncErrorCategory::TokenPermissionDenied => "token_permission_denied",
            SyncErrorCategory::AuthError
            | SyncErrorCategory::GithubUnauthorized
            | SyncErrorCategory::GithubForbidden => "auth_failed",
            SyncErrorCategory::RepoNotFoundOrNoPermission => "repo_not_found_or_no_permission",
            SyncErrorCategory::EmptyUrl => "not_configured",
            SyncErrorCategory::MissingPermission => "permission_missing",
            SyncErrorCategory::NetworkProbeFailed
            | SyncErrorCategory::GithubNetworkFailed
            | SyncErrorCategory::DnsFailed
            | SyncErrorCategory::TlsFailed => "network_failed",
            SyncErrorCategory::BranchMissing | SyncErrorCategory::RemoteBranchMissing => {
                "branch_missing"
            }
            SyncErrorCategory::NotFound | SyncErrorCategory::FileNotFound => "auth_failed",
            SyncErrorCategory::NonFastForward => "non_fast_forward",
            SyncErrorCategory::Conflict
            | SyncErrorCategory::CheckoutConflict
            | SyncErrorCategory::LocalBlockingFile => "conflict",
            SyncErrorCategory::UnrelatedHistories => "unrelated_histories",
            SyncErrorCategory::LocalIoError => "error",
            SyncErrorCategory::ApiRateLimited => "error",
            SyncErrorCategory::ApiError => "error",
            SyncErrorCategory::DirtyRepo => "dirty_repo",
            SyncErrorCategory::Other => "error",
        }
    }

    pub fn to_message_key(&self) -> &'static str {
        match self {
            SyncErrorCategory::None => "sync.result.generic_error",
            SyncErrorCategory::TokenMissing => "sync.result.token_missing",
            SyncErrorCategory::TokenInvalid => "sync.result.token_invalid",
            SyncErrorCategory::TokenPermissionDenied => "sync.result.token_permission_denied",
            SyncErrorCategory::AuthError => "sync.result.auth_failed",
            SyncErrorCategory::RepoNotFoundOrNoPermission => {
                "sync.result.repo_not_found_or_no_permission"
            }
            SyncErrorCategory::GithubUnauthorized | SyncErrorCategory::GithubForbidden => {
                "sync.result.auth_failed"
            }
            SyncErrorCategory::EmptyUrl => "sync.result.configured_not_tested",
            SyncErrorCategory::MissingPermission => "sync.result.permission_missing",
            SyncErrorCategory::NetworkProbeFailed
            | SyncErrorCategory::GithubNetworkFailed
            | SyncErrorCategory::DnsFailed
            | SyncErrorCategory::TlsFailed => "sync.result.network_failed",
            SyncErrorCategory::BranchMissing | SyncErrorCategory::RemoteBranchMissing => {
                "sync.result.branch_recovered_summary"
            }
            SyncErrorCategory::NotFound | SyncErrorCategory::FileNotFound => {
                "sync.result.auth_failed"
            }
            SyncErrorCategory::NonFastForward => "sync.result.non_fast_forward",
            SyncErrorCategory::Conflict
            | SyncErrorCategory::CheckoutConflict
            | SyncErrorCategory::LocalBlockingFile => "sync.result.conflict_summary",
            SyncErrorCategory::UnrelatedHistories => "sync.result.unrelated_histories",
            SyncErrorCategory::LocalIoError => "sync.result.generic_error",
            SyncErrorCategory::ApiRateLimited => "sync.result.generic_error",
            SyncErrorCategory::ApiError => "sync.result.generic_error",
            SyncErrorCategory::DirtyRepo => "sync.result.dirty_repo_blocked",
            SyncErrorCategory::Other => "sync.result.generic_error",
        }
    }

    pub fn from_code(code: &str, _fallback_msg: &str) -> Self {
        match code {
            "none" | "" => SyncErrorCategory::Other,
            "token_missing" => SyncErrorCategory::TokenMissing,
            "token_invalid" => SyncErrorCategory::TokenInvalid,
            "token_permission_denied" => SyncErrorCategory::TokenPermissionDenied,
            "auth_error" => SyncErrorCategory::AuthError,
            "repo_not_found_or_no_permission" => SyncErrorCategory::RepoNotFoundOrNoPermission,
            "github_unauthorized" => SyncErrorCategory::GithubUnauthorized,
            "github_forbidden" => SyncErrorCategory::GithubForbidden,
            "empty_url" => SyncErrorCategory::EmptyUrl,
            "missing_permission" => SyncErrorCategory::MissingPermission,
            "network_probe_failed" => SyncErrorCategory::NetworkProbeFailed,
            "github_network_failed" => SyncErrorCategory::GithubNetworkFailed,
            "dns_failed" => SyncErrorCategory::DnsFailed,
            "tls_failed" => SyncErrorCategory::TlsFailed,
            "branch_missing" => SyncErrorCategory::BranchMissing,
            "remote_branch_missing" => SyncErrorCategory::RemoteBranchMissing,
            "not_found" => SyncErrorCategory::NotFound,
            "file_not_found" => SyncErrorCategory::FileNotFound,
            "non_fast_forward" => SyncErrorCategory::NonFastForward,
            "conflict" => SyncErrorCategory::Conflict,
            "checkout_conflict" => SyncErrorCategory::CheckoutConflict,
            "local_blocking_file" => SyncErrorCategory::LocalBlockingFile,
            "unrelated_histories" => SyncErrorCategory::UnrelatedHistories,
            "local_io_error" => SyncErrorCategory::LocalIoError,
            "api_rate_limited" => SyncErrorCategory::ApiRateLimited,
            "api_error" => SyncErrorCategory::ApiError,
            "network_error" => SyncErrorCategory::GithubNetworkFailed,
            "dirty_repo" => SyncErrorCategory::DirtyRepo,
            _ => SyncErrorCategory::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BackendType {
    Git,
    #[default]
    GithubApi,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncTransport {
    HttpsToken,
    SshDeployKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FirstSyncMode {
    #[default]
    NotAttempted,
    CloneIntoEmptyWorkspace,
    InitExistingWorkspace,
    AlreadyGitRepo,
    BlockedNonEmptyRemote,
    UnrelatedHistories,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub enabled: bool,
    #[serde(default)]
    pub backend_type: BackendType,
    pub remote_url: String,
    pub transport: SyncTransport,
    #[serde(default = "default_branch")]
    pub branch: String,
    pub auto_sync: bool,
    pub sync_interval_seconds: u32,

    /// GitHub username for HTTPS credential callback.
    /// Defaults to "x-access-token" when empty.
    #[serde(default)]
    pub username: String,
    /// Android-only: whether INTERNET permission is granted.
    /// Linux always sets this to true.
    #[serde(default = "default_true")]
    pub android_has_internet_permission: bool,
    /// Android-only: whether ACCESS_NETWORK_STATE permission is granted.
    /// Linux always sets this to true.
    #[serde(default = "default_true")]
    pub android_has_access_network_state_permission: bool,
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_branch() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncSecrets {
    pub token: Option<String>,
    pub ssh_private_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Idle,
    Syncing,
    Success,
    ConfiguredNotTested,
    Conflict,
    PartialConflict,
    RecoverableError(String),
    FatalError(String),
    DirtyRepoBlocked,
    BranchMissingRecovered,
    Error(String),
    NoChanges,
    LatestWinsApplied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingConflictDetail {
    pub key: String,
    pub local_value: serde_json::Value,
    pub remote_value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncKind {
    Upload,
    Ignore,
    ConflictCandidate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFileEntry {
    pub relative_path: String,
    pub absolute_path: String,
    pub file_hash: String,
    pub modified_time: i64,
    pub sync_kind: SyncKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub local_path: String,
    pub remote_path: String,
    pub local_hash: String,
    pub remote_hash: String,
    pub base_hash: String,
    pub created_at: i64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDiagnosticsResult {
    pub success: bool,
    /// Backend type: git/github_api
    pub backend_type: String,
    /// Android permission check results
    pub android_has_internet_permission: bool,
    pub android_has_access_network_state_permission: bool,
    pub android_network_state: String,
    pub network_ok: bool,
    pub auth_ok: bool,
    pub repo_ok: bool,
    pub branch_ok: bool,
    pub network_status: String,
    pub auth_status: String,
    pub repo_status: String,
    pub branch_status: String,
    /// Sanitized remote URL (no credentials)
    pub remote_url_sanitized: String,
    /// Transport type: https/ssh/unknown
    pub transport: String,

    /// Error category for sync failures
    pub error_category: String,
    pub raw_error: Option<String>,
}

impl Default for SyncDiagnosticsResult {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncDiagnosticsResult {
    pub fn new() -> Self {
        Self {
            success: false,
            backend_type: "git".to_string(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
            android_network_state: "unchecked".to_string(),
            network_ok: false,
            auth_ok: false,
            repo_ok: false,
            branch_ok: false,
            network_status: "unchecked".to_string(),
            auth_status: "unchecked".to_string(),
            repo_status: "unchecked".to_string(),
            branch_status: "unchecked".to_string(),
            remote_url_sanitized: "".to_string(),
            transport: "unknown".to_string(),
            error_category: "none".to_string(),
            raw_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflictSummary {
    pub status: String,
    pub local_dirty: bool,
    pub remote_changed: bool,
    pub conflicted_files: Vec<String>,
    pub blocked_reason: String,
    pub safe_next_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub status: SyncStatus,
    pub uploaded_files: Vec<String>,
    pub downloaded_files: Vec<String>,
    pub ignored_files: Vec<String>,
    pub conflicts: Vec<SyncConflict>,
    pub commit_hash: Option<String>,
    pub error: Option<String>,
    pub error_category: Option<String>,
    pub message_key: Option<String>,
    pub conflict_summary: Option<SyncConflictSummary>,
    pub first_sync_mode: FirstSyncMode,
    pub settings_conflicts: Option<Vec<SettingConflictDetail>>,
    #[serde(default)]
    pub local_deletes: Vec<String>,
    #[serde(default)]
    pub remote_deletes: Vec<String>,
    #[serde(default)]
    pub overwritten_files: Vec<String>,
}

impl SyncResult {
    pub fn success() -> Self {
        Self {
            status: SyncStatus::Success,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts: Vec::new(),
            commit_hash: None,
            error: None,
            error_category: None,
            message_key: None,
            conflict_summary: None,
            first_sync_mode: FirstSyncMode::NotAttempted,
            settings_conflicts: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
        }
    }

    pub fn error(status: SyncStatus, first_sync_mode: FirstSyncMode, error: String, error_category: Option<String>) -> Self {
        let message_key = error_category.as_deref().map(sync_error_category_to_message_key);
        Self {
            status,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts: Vec::new(),
            commit_hash: None,
            error: Some(error),
            error_category: error_category.clone(),
            message_key,
            conflict_summary: None,
            first_sync_mode,
            settings_conflicts: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
        }
    }

    pub fn conflict(conflicts: Vec<SyncConflict>, error: String, error_category: Option<String>) -> Self {
        Self {
            status: SyncStatus::Conflict,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts,
            commit_hash: None,
            error: Some(error),
            error_category: error_category.clone(),
            message_key: error_category.as_deref().map(sync_error_category_to_message_key),
            conflict_summary: None,
            first_sync_mode: FirstSyncMode::NotAttempted,
            settings_conflicts: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
        }
    }
}

fn sync_error_category_to_message_key(category: &str) -> String {
    let cat = SyncErrorCategory::from_code(category, "");
    cat.to_message_key().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub files_to_upload: Vec<String>,
    pub files_to_download: Vec<String>,
    pub files_to_delete_local: Vec<String>,
    pub files_to_delete_remote: Vec<String>,
    pub ignored_files: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
}

impl Default for SyncPlan {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncPlan {
    pub fn new() -> Self {
        Self {
            files_to_upload: Vec::new(),
            files_to_download: Vec::new(),
            files_to_delete_local: Vec::new(),
            files_to_delete_remote: Vec::new(),
            ignored_files: Vec::new(),
            conflicts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFileRecord {
    pub path: String,
    pub content_hash: String,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub deleted_at_ms: Option<i64>,
    pub device_id: String,
    pub op: String, // "upsert" or "delete"
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncManifest {
    pub files: Vec<ManifestFileRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tombstone {
    pub original_path: String,
    pub trash_path: String,
    pub deleted_at: i64,
    pub purge_after: i64,
    pub deleted_by: String,
    pub original_hash: String,
    pub kind: String, // "local_delete" or "remote_delete"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub remote_url: Option<String>,
    pub transport: Option<SyncTransport>,
    pub last_synced_commit: Option<String>,
    pub last_sync_time: Option<i64>,
    pub last_error: Option<String>,
    pub known_files: std::collections::HashMap<String, String>,
    pub conflicts: Vec<SyncConflict>,
    #[serde(default)]
    pub tombstones: Vec<Tombstone>,
    #[serde(default)]
    pub deleted_files: std::collections::HashSet<String>,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub known_files_updated_at: std::collections::HashMap<String, i64>,
    /// Paths that have unresolved sync conflicts. While a path is in this set,
    /// the sync engine must not auto-upload, auto-download, or apply LWW/three-way
    /// resolution to it. The conflict persists until the user explicitly resolves
    /// it via `resolve_conflict_keep_local` / `resolve_conflict_take_remote` /
    /// `resolve_conflict_mark_merged`.
    #[serde(default)]
    pub conflicted_files: std::collections::HashSet<String>,
    /// Paths where the user chose "take remote" but the remote content has not
    /// yet been downloaded. On the next `perform_sync`, the engine must force-
    /// download these paths before any three-way comparison, then clear the set.
    #[serde(default)]
    pub pending_take_remote: std::collections::HashSet<String>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            remote_url: None,
            transport: None,
            last_synced_commit: None,
            last_sync_time: None,
            last_error: None,
            known_files: std::collections::HashMap::new(),
            conflicts: Vec::new(),
            tombstones: Vec::new(),
            deleted_files: std::collections::HashSet::new(),
            device_id: uuid::Uuid::new_v4().to_string(),
            known_files_updated_at: std::collections::HashMap::new(),
            conflicted_files: std::collections::HashSet::new(),
            pending_take_remote: std::collections::HashSet::new(),
        }
    }
}
