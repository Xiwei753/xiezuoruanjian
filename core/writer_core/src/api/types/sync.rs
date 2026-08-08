//! # 同步 DTO 层 — Core 内部类型与 JSON 线格式的双向转换
//!
//! 本模块定义跨语言边界（FFI/HTTP）使用的数据传输对象。
//! 线格式约定：
//! - 枚举使用字符串表示（如 `"idle"`, `"syncing"`），而非数字或嵌套 JSON，
//!   以保证 UniFFI/JNI/JSON 各端可无损解析
//! - `_to_wire` 后缀函数将 Rust 枚举转换为线格式字符串
//! - `From<Dto>` / `From<Internal>` 实现双向转换
//!
//! 注意：`SyncSecretsDto` → `SyncSecrets` 转换时 `ssh_private_key` 始终设为 None，
//! 因为 SSH 密钥不通过 JSON DTO 传输，由平台端安全存储直接注入。

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncConfigDto {
    pub enabled: bool,
    pub backend_type: String,
    pub remote_url: String,
    pub transport: String,
    pub branch: String,
    pub auto_sync: bool,
    pub sync_interval_seconds: u32,
    pub username: String,
    pub has_network_permission: bool,
    pub has_network_state_permission: bool,
}

impl From<crate::sync::SyncConfig> for SyncConfigDto {
    fn from(c: crate::sync::SyncConfig) -> Self {
        Self {
            enabled: c.enabled,
            backend_type: match c.backend_type {
                crate::sync::BackendType::Git => "git".to_string(),
                crate::sync::BackendType::GithubApi => "github_api".to_string(),
            },
            remote_url: c.remote_url,
            transport: match c.transport {
                crate::sync::SyncProtocol::HttpsToken => "https_token".to_string(),
                crate::sync::SyncProtocol::SshDeployKey => "ssh_deploy_key".to_string(),
            },
            branch: c.branch,
            auto_sync: c.auto_sync,
            sync_interval_seconds: c.sync_interval_seconds,
            username: c.username,
            has_network_permission: c.has_network_permission,
            has_network_state_permission: c.has_network_state_permission,
        }
    }
}

impl From<SyncConfigDto> for crate::sync::SyncConfig {
    fn from(c: SyncConfigDto) -> Self {
        crate::sync::SyncConfig {
            enabled: c.enabled,
            backend_type: match c.backend_type.as_str() {
                "git" => crate::sync::BackendType::Git,
                "github_api" => crate::sync::BackendType::GithubApi,
                _ => crate::sync::BackendType::GithubApi,
            },
            remote_url: c.remote_url,
            transport: match c.transport.as_str() {
                "https_token" => crate::sync::SyncProtocol::HttpsToken,
                "ssh" | "ssh_deploy_key" => crate::sync::SyncProtocol::SshDeployKey,
                _ => crate::sync::SyncProtocol::HttpsToken,
            },
            branch: c.branch,
            auto_sync: c.auto_sync,
            sync_interval_seconds: c.sync_interval_seconds,
            username: c.username,
            has_network_state_permission: c.has_network_state_permission,
            has_network_permission: c.has_network_permission,
            scope: crate::sync::SyncScope::default(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncSecretsDto {
    pub token: Option<String>,
}

impl From<crate::sync::SyncSecrets> for SyncSecretsDto {
    fn from(s: crate::sync::SyncSecrets) -> Self {
        Self { token: s.token }
    }
}

impl From<SyncSecretsDto> for crate::sync::SyncSecrets {
    fn from(s: SyncSecretsDto) -> Self {
        crate::sync::SyncSecrets {
            token: s.token,
            ssh_private_key: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncStateDto {
    pub status: String,
    pub remote_url: Option<String>,
    pub backend_type: Option<String>,
    pub transport: Option<String>,
    pub last_synced_commit: Option<String>,
    pub last_sync_time: Option<i64>,
    pub last_error: Option<String>,
    pub conflicts: Option<Vec<SyncConflictDto>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncConflictDto {
    pub local_path: String,
    pub remote_path: String,
    pub local_hash: String,
    pub remote_hash: String,
    pub base_hash: String,
    pub created_at: i64,
    pub description: String,
}

impl From<crate::sync::SyncConflict> for SyncConflictDto {
    fn from(c: crate::sync::SyncConflict) -> Self {
        Self {
            local_path: c.local_path,
            remote_path: c.remote_path,
            local_hash: c.local_hash,
            remote_hash: c.remote_hash,
            base_hash: c.base_hash,
            created_at: c.created_at,
            description: c.description,
        }
    }
}

impl From<crate::sync::SyncState> for SyncStateDto {
    fn from(s: crate::sync::SyncState) -> Self {
        Self {
            status: "idle".to_string(),
            remote_url: s.remote_url,
            backend_type: None,
            transport: s.transport.map(sync_transport_to_wire),
            last_synced_commit: s.last_synced_commit,
            last_sync_time: s.last_sync_time,
            last_error: s.last_error,
            conflicts: Some(s.conflicts.into_iter().map(Into::into).collect()),
        }
    }
}

impl From<SyncConflictDto> for crate::sync::SyncConflict {
    fn from(c: SyncConflictDto) -> Self {
        crate::sync::SyncConflict {
            local_path: c.local_path,
            remote_path: c.remote_path,
            local_hash: c.local_hash,
            remote_hash: c.remote_hash,
            base_hash: c.base_hash,
            created_at: c.created_at,
            description: c.description,
        }
    }
}

impl From<SyncStateDto> for crate::sync::SyncState {
    fn from(s: SyncStateDto) -> Self {
        crate::sync::SyncState {
            remote_url: s.remote_url,
            transport: s.transport.map(sync_transport_from_wire_owned),
            last_synced_commit: s.last_synced_commit,
            last_sync_time: s.last_sync_time,
            last_error: s.last_error,
            conflicts: s
                .conflicts
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncDiagnosticsResultDto {
    pub success: bool,
    pub backend_type: String,
    pub has_network_permission: bool,
    pub has_network_state_permission: bool,
    pub network_state: String,
    pub network_ok: bool,
    pub auth_ok: bool,
    pub repo_ok: bool,
    pub branch_ok: bool,
    pub network_status: String,
    pub auth_status: String,
    pub repo_status: String,
    pub branch_status: String,
    pub remote_url_sanitized: String,
    pub transport: String,
    pub error_category: String,
    pub raw_error: Option<String>,
}

impl From<crate::sync::SyncDiagnosticsResult> for SyncDiagnosticsResultDto {
    fn from(d: crate::sync::SyncDiagnosticsResult) -> Self {
        Self {
            success: d.success,
            backend_type: d.backend_type,
            has_network_permission: d.has_network_permission,
            has_network_state_permission: d.has_network_state_permission,
            network_state: d.network_state,
            network_ok: d.network_ok,
            auth_ok: d.auth_ok,
            repo_ok: d.repo_ok,
            branch_ok: d.branch_ok,
            network_status: d.network_status,
            auth_status: d.auth_status,
            repo_status: d.repo_status,
            branch_status: d.branch_status,
            remote_url_sanitized: d.remote_url_sanitized,
            transport: d.transport,
            error_category: d.error_category,
            raw_error: d.raw_error,
        }
    }
}

/// 同步能力查询结果 — 告知平台端同步是否可执行及阻塞原因。
///
/// - `can_run`：同步是否可立即执行
/// - `block_reason_code`：阻塞原因代码（如 `"no_internet"`、`"sync_in_progress"`），
///   平台端 i18n 层根据此代码映射用户可见提示
/// - `block_message_key`：阻塞原因的 i18n key（备选，当 reason_code 不够描述时使用）
/// - `message_args`：i18n 模板参数（如同步进度百分比）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncCapabilityDto {
    pub can_run: bool,
    pub block_reason_code: Option<String>,
    pub block_message_key: Option<String>,
    pub message_args: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub struct SyncOperationCountsDto {
    pub uploaded: u32,
    pub downloaded: u32,
    pub local_deleted: u32,
    pub remote_deleted: u32,
    pub overwritten: u32,
    pub ignored: u32,
    pub conflicts: u32,
}

/// 同步操作状态 — 描述一次同步操作的进度和结果。
///
/// - `operation_id`：操作唯一标识（用于取消和进度跟踪）
/// - `operation_kind`：操作类型（`"sync"` / `"pull"` / `"push"` / `"diagnose"`）
/// - `status_code`：操作状态（`"idle"` / `"running"` / `"completed"` / `"failed"`）
/// - `phase_key`：当前阶段 i18n key（如 `"uploading"` / `"downloading"` / `"resolving_conflicts"`）
/// - `summary_key`：完成摘要 i18n key（如 `"sync_completed"` / `"sync_completed_with_conflicts"`）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncOperationStateDto {
    pub operation_id: String,
    pub operation_kind: String,
    pub status_code: String,
    pub phase_key: Option<String>,
    pub summary_key: Option<String>,
    pub summary_args: std::collections::HashMap<String, String>,
    pub counts: SyncOperationCountsDto,
    pub raw_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncPlanDto {
    pub files_to_upload: Vec<String>,
    pub files_to_download: Vec<String>,
    pub files_to_delete_local: Vec<String>,
    pub files_to_delete_remote: Vec<String>,
    pub ignored_files: Vec<String>,
    pub conflicts: Vec<String>,
}

impl From<crate::sync::SyncPlan> for SyncPlanDto {
    fn from(p: crate::sync::SyncPlan) -> Self {
        Self {
            files_to_upload: p.files_to_upload,
            files_to_download: p.files_to_download,
            files_to_delete_local: p.files_to_delete_local,
            files_to_delete_remote: p.files_to_delete_remote,
            ignored_files: p.ignored_files,
            conflicts: p.conflicts,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncResultDto {
    pub status: String,
    pub uploaded_files: Vec<String>,
    pub downloaded_files: Vec<String>,
    pub local_deletes: Vec<String>,
    pub remote_deletes: Vec<String>,
    pub overwritten_files: Vec<String>,
    pub ignored_files: Vec<String>,
    pub conflicts: Vec<SyncConflictDto>,
    pub commit_hash: Option<String>,
    pub error: Option<String>,
    pub error_category: Option<String>,
    pub message_key: Option<String>,
    pub first_sync_mode: String,
    pub search_index_rebuild_error: Option<String>,
}

impl From<crate::sync::SyncResult> for SyncResultDto {
    fn from(r: crate::sync::SyncResult) -> Self {
        Self {
            status: sync_status_to_wire(&r.status),
            uploaded_files: r.uploaded_files,
            downloaded_files: r.downloaded_files,
            local_deletes: r.local_deletes,
            remote_deletes: r.remote_deletes,
            overwritten_files: r.overwritten_files,
            ignored_files: r.ignored_files,
            conflicts: r.conflicts.into_iter().map(Into::into).collect(),
            commit_hash: r.commit_hash,
            error: r.error,
            error_category: r.error_category,
            message_key: r.message_key,
            first_sync_mode: first_sync_mode_to_wire(&r.first_sync_mode),
            search_index_rebuild_error: r.search_index_rebuild_error,
        }
    }
}

fn sync_transport_to_wire(transport: crate::sync::SyncProtocol) -> String {
    match transport {
        crate::sync::SyncProtocol::HttpsToken => "https_token".to_string(),
        crate::sync::SyncProtocol::SshDeployKey => "ssh_deploy_key".to_string(),
    }
}

fn sync_transport_from_wire(s: &str) -> crate::sync::SyncProtocol {
    match s {
        "ssh" | "ssh_deploy_key" => crate::sync::SyncProtocol::SshDeployKey,
        _ => crate::sync::SyncProtocol::HttpsToken,
    }
}

fn sync_transport_from_wire_owned(s: String) -> crate::sync::SyncProtocol {
    sync_transport_from_wire(&s)
}

/// 将 `SyncStatus` 枚举转换为线格式字符串。
/// `RecoverableError`/`FatalError`/`Error` 携带的详细信息在线格式中丢失，
/// 平台端需通过 `error`/`error_category` 字段获取具体错误原因。
fn sync_status_to_wire(status: &crate::sync::SyncStatus) -> String {
    match status {
        crate::sync::SyncStatus::Idle => "idle",
        crate::sync::SyncStatus::Syncing => "syncing",
        crate::sync::SyncStatus::Success => "success",
        crate::sync::SyncStatus::ConfiguredNotTested => "configured_not_tested",
        crate::sync::SyncStatus::Conflict => "conflict",
        crate::sync::SyncStatus::PartialConflict => "partial_conflict",
        crate::sync::SyncStatus::RecoverableError(_) => "recoverable_error",
        crate::sync::SyncStatus::FatalError(_) => "fatal_error",
        crate::sync::SyncStatus::DirtyRepoBlocked => "dirty_repo_blocked",
        crate::sync::SyncStatus::BranchMissingRecovered => "branch_missing_recovered",
        crate::sync::SyncStatus::Error(_) => "error",
        crate::sync::SyncStatus::NoChanges => "no_changes",
        crate::sync::SyncStatus::LatestWinsApplied => "latest_wins_applied",
    }
    .to_string()
}

fn first_sync_mode_to_wire(mode: &crate::sync::FirstSyncMode) -> String {
    match mode {
        crate::sync::FirstSyncMode::NotAttempted => "not_attempted",
        crate::sync::FirstSyncMode::CloneIntoEmptyProject => "clone_into_empty_project",
        crate::sync::FirstSyncMode::InitExistingProject => "init_existing_project",
        crate::sync::FirstSyncMode::AlreadyGitRepo => "already_git_repo",
        crate::sync::FirstSyncMode::BlockedNonEmptyRemote => "blocked_non_empty_remote",
        crate::sync::FirstSyncMode::UnrelatedHistories => "unrelated_histories",
    }
    .to_string()
}
