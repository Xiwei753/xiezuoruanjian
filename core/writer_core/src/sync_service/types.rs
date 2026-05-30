use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BackendType {
    Git,
    #[default]
    GithubApi,
    WebDav,
    S3,
    LocalFolder,
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
    #[serde(default)]
    pub proxy_enabled: bool,
    #[serde(default = "default_proxy_type")]
    pub proxy_type: String,
    #[serde(default = "default_proxy_host")]
    pub proxy_host: String,
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
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

pub(crate) fn default_proxy_type() -> String {
    "auto".to_string()
}

pub(crate) fn default_proxy_host() -> String {
    "127.0.0.1".to_string()
}

pub(crate) fn default_proxy_port() -> u16 {
    7890
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
    ConfiguredUntested,
    Conflict,
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
pub struct NetworkProbeResult {
    pub mode: String,
    pub success: bool,
    pub status: String,
    pub message: String,
    pub raw_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDiagnosticsResult {
    pub success: bool,
    /// Backend type: git/github_api/webdav/s3/local_folder
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
    /// App-level proxy status: "未启用"/"已启用"
    pub app_proxy_status: String,
    /// Error category for proxy_enabled=false failures
    pub error_category: String,
    pub user_message: String,
    pub raw_error: Option<String>,
    pub chosen_network_mode: Option<String>,
    pub network_probe_summary: Vec<NetworkProbeResult>,
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
            app_proxy_status: "未启用".to_string(),
            error_category: "none".to_string(),
            user_message: "".to_string(),
            raw_error: None,
            chosen_network_mode: None,
            network_probe_summary: Vec::new(),
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
    pub conflict_summary: Option<SyncConflictSummary>,
    pub first_sync_mode: FirstSyncMode,
    pub user_message: Option<String>,
    pub chosen_network_mode: Option<String>,
    pub network_probe_summary: Vec<NetworkProbeResult>,
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
            conflict_summary: None,
            first_sync_mode: FirstSyncMode::NotAttempted,
            user_message: None,
            chosen_network_mode: None,
            network_probe_summary: Vec::new(),
            settings_conflicts: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
        }
    }

    pub fn error(
        status: SyncStatus,
        first_sync_mode: FirstSyncMode,
        user_message: Option<String>,
        error: String,
    ) -> Self {
        Self {
            status,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts: Vec::new(),
            commit_hash: None,
            error: Some(error),
            error_category: None,
            conflict_summary: None,
            first_sync_mode,
            user_message,
            chosen_network_mode: None,
            network_probe_summary: Vec::new(),
            settings_conflicts: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
        }
    }

    pub fn conflict(
        conflicts: Vec<SyncConflict>,
        error: String,
        user_message: Option<String>,
    ) -> Self {
        Self {
            status: SyncStatus::Conflict,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts,
            commit_hash: None,
            error: Some(error),
            error_category: None,
            conflict_summary: None,
            first_sync_mode: FirstSyncMode::NotAttempted,
            user_message,
            chosen_network_mode: None,
            network_probe_summary: Vec::new(),
            settings_conflicts: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
        }
    }
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
    pub last_successful_network_mode: Option<String>,
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
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            remote_url: None,
            transport: None,
            last_synced_commit: None,
            last_sync_time: None,
            last_error: None,
            last_successful_network_mode: None,
            known_files: std::collections::HashMap::new(),
            conflicts: Vec::new(),
            tombstones: Vec::new(),
            deleted_files: std::collections::HashSet::new(),
            device_id: uuid::Uuid::new_v4().to_string(),
            known_files_updated_at: std::collections::HashMap::new(),
        }
    }
}
