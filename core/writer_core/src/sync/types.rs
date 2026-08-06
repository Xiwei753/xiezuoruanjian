use serde::{Deserialize, Serialize};

/// 同步错误分类 — 纯枚举，不携带可变文案。
///
/// 平台端通过 `to_ui_status()` 和 `to_message_key()` 做错误分类和 i18n 映射，
/// 不得依赖错误文案的包含关系作为主判断（见 AGENTS.md）。
/// `from_code()` 将字符串反序列化回枚举，未知 code 统一映射为 `Other`。
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
    /// 映射为 UI 状态字符串——供平台端决定同步状态图标和提示文案。
    /// 返回值是 API 契约，不可随意更改。
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
            // NotFound/FileNotFound 映射到 auth_failed 而非 not_found，因为 GitHub API
            // 对无权限访问的仓库也返回 404（不区分"不存在"和"无权限"），
            // 所以 404 在同步语境下等同于认证/权限问题。
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

    /// 映射为 i18n message key——供 UI 层做本地化映射。key 是 API 契约。
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

    /// 从线格式 code 字符串反序列化。未知 code 映射为 `Other`。
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

/// 同步后端类型 — 当前仅支持 GitHub API，Git SSH 为预留。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BackendType {
    Git,
    #[default]
    GithubApi,
}

/// 同步协议方式 — HTTPS token 或 SSH deploy key。
///
/// 命名为 `SyncProtocol` 以区别于 `writer_platform_api::SyncTransport` trait。
/// `SyncProtocol` 描述用户选择的同步认证方式，`SyncTransport` trait 描述 HTTP 执行能力。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncProtocol {
    HttpsToken,
    SshDeployKey,
}

/// 首次同步模式 — 记录工作区与远端仓库的初始关系。
///
/// - CloneIntoEmptyWorkspace：远端有内容，本地为空，直接 clone。
/// - InitExistingWorkspace：本地已有内容，远端为空，push 本地内容。
/// - AlreadyGitRepo：本地已是 git 仓库，直接 fetch+merge。
/// - BlockedNonEmptyRemote：双方都有内容且无共同祖先，需用户决策。
/// - UnrelatedHistories：git merge 时遇到 unrelated histories。
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

/// 同步配置 — 持久化为 `app-meta/sync/config.json`，UI 层读写。
///
/// 非线程安全：只在主线程读写，同步引擎在同步期间持有快照。
/// `sync_interval_seconds` 最小有效值为 60（引擎侧 clamp），0 表示仅手动同步。
///
/// 敏感字段（token、ssh_private_key）不在 SyncConfig 中，
/// 由 SyncSecrets 单独管理，平台端安全存储注入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// 是否启用同步
    pub enabled: bool,
    /// 同步后端类型（当前仅 GithubApi）
    #[serde(default)]
    pub backend_type: BackendType,
    /// 远端仓库 URL（HTTPS 或 SSH）
    pub remote_url: String,
    /// 传输方式（HTTPS token 或 SSH deploy key）
    pub transport: SyncProtocol,
    /// 远端分支名，默认 "main"
    #[serde(default = "default_branch")]
    pub branch: String,
    /// 是否启用自动同步
    pub auto_sync: bool,
    /// 自动同步间隔（秒），最小有效值 60，0 表示仅手动
    pub sync_interval_seconds: u32,

    /// GitHub username for HTTPS credential callback.
    /// Defaults to "x-access-token" when empty.
    #[serde(default)]
    pub username: String,
    /// Whether the platform grants network access permission.
    /// Android sets this based on INTERNET permission; Linux Qt and Windows platforms always true.
    #[serde(default = "default_true", alias = "android_has_internet_permission")]
    pub has_network_permission: bool,
    /// Whether the platform grants network state query permission.
    /// Android sets this based on ACCESS_NETWORK_STATE permission; Linux Qt and Windows platforms always true.
    #[serde(
        default = "default_true",
        alias = "android_has_access_network_state_permission"
    )]
    pub has_network_state_permission: bool,
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_branch() -> String {
    "main".to_string()
}

/// 同步密钥 — 敏感凭证，不持久化到 config.json，由平台端安全存储注入。
///
/// `token`：GitHub personal access token（HTTPS 模式）。
/// `ssh_private_key`：SSH deploy key（SSH 模式，当前未使用）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncSecrets {
    pub token: Option<String>,
    pub ssh_private_key: Option<String>,
}

/// 同步状态 — UI 展示和引擎内部共用的终端状态枚举。
///
/// `RecoverableError`：网络/限流等临时错误，下次自动重试可恢复。
/// `FatalError`：认证/权限等不可自动恢复的错误，需用户干预。
/// `LatestWinsApplied`：LWW 决胜后自动应用了较新版本（仅 Metadata/GeneratedCache）。
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

/// 设置键冲突详情 — 逐键语义合并时双方都修改了同一 key。
///
/// `local_value`/`remote_value` 为 JSON 值，一方删除时对应 `Value::Null`。
/// UI 展示冲突时使用此结构，用户选择 keep local / take remote / mark merged。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingConflictDetail {
    pub key: String,
    pub local_value: serde_json::Value,
    pub remote_value: serde_json::Value,
}

/// 同步文件操作分类。
///
/// - Upload：本地较新或仅本地存在，需上传。
/// - Ignore：双方相同或本地未变更，跳过。
/// - ConflictCandidate：BothChanged，需走冲突解决流程。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncKind {
    Upload,
    Ignore,
    ConflictCandidate,
}

/// 同步扫描结果中的单条文件记录 — 本地文件的快照信息。
///
/// `file_hash` 为 MD5 十六进制摘要。`modified_time` 为 Unix 毫秒时间戳。
/// `sync_kind` 由扫描阶段根据 known_files 初步判定，后续由三路/LWW 比较可能调整。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFileEntry {
    pub relative_path: String,
    pub absolute_path: String,
    pub file_hash: String,
    pub modified_time: i64,
    pub sync_kind: SyncKind,
}

/// 同步冲突记录 — 描述一个 BothChanged 路径的双方版本信息。
///
/// `base_hash` 为三路比较的基准哈希（上次同步后的共识版本）。
/// 冲突解决前，该路径在 `SyncState.conflicted_files` 中，同步引擎跳过自动处理。
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

/// 同步诊断结果 — 逐步检查网络、认证、仓库、分支的可达性。
///
/// 每一步的 `*_ok` 布尔值和 `*_status` 字符串独立记录，
/// UI 可按步骤展示诊断链路。`remote_url_sanitized` 已去除凭证信息，可安全展示。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDiagnosticsResult {
    pub success: bool,
    /// Backend type: git/github_api
    pub backend_type: String,
    /// Whether the platform grants network access permission.
    #[serde(alias = "android_has_internet_permission")]
    pub has_network_permission: bool,
    /// Whether the platform grants network state query permission.
    #[serde(alias = "android_has_access_network_state_permission")]
    pub has_network_state_permission: bool,
    /// Current network connectivity state reported by the platform.
    #[serde(alias = "android_network_state")]
    pub network_state: String,
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
    /// 创建默认诊断结果——所有检查项初始为 "unchecked"/false。
    pub fn new() -> Self {
        Self {
            success: false,
            backend_type: "git".to_string(),
            has_network_permission: true,
            has_network_state_permission: true,
            network_state: "unchecked".to_string(),
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

/// 同步冲突摘要 — 供 UI 展示冲突状态和下一步操作建议。
///
/// `local_dirty`/`remote_changed` 标识双方是否有未提交变更。
/// `conflicted_files` 为具体冲突路径列表（可能为空，此时 local_dirty 兜底填充）。
/// `safe_next_steps` 为用户可执行的操作建议。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflictSummary {
    pub status: String,
    pub local_dirty: bool,
    pub remote_changed: bool,
    pub conflicted_files: Vec<String>,
    pub blocked_reason: String,
    pub safe_next_steps: Vec<String>,
}

/// 同步结果 — 一次 `perform_sync` 的完整输出。
///
/// `status` 是终端状态（Success/Conflict/Error 等），其余字段提供详情。
/// `uploaded_files` / `downloaded_files` / `ignored_files` 仅在 Success 时有意义。
/// `conflicts` 仅在 Conflict/PartialConflict 时非空。
/// `overwritten_files` 记录 LWW 决胜中被覆盖的一方（仅 Metadata/GeneratedCache）。
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
    #[serde(default)]
    pub search_index_rebuild_error: Option<String>,
}

impl SyncResult {
    /// 创建成功结果——无冲突、无错误。
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
            search_index_rebuild_error: None,
        }
    }

    /// 创建错误结果——status 应为 Error/Conflict 等终端状态，error_category 可选。
    pub fn error(
        status: SyncStatus,
        first_sync_mode: FirstSyncMode,
        error: String,
        error_category: Option<String>,
    ) -> Self {
        let message_key = error_category
            .as_deref()
            .map(sync_error_category_to_message_key);
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
            search_index_rebuild_error: None,
        }
    }

    /// 创建冲突结果——包含具体冲突列表和错误描述。
    pub fn conflict(
        conflicts: Vec<SyncConflict>,
        error: String,
        error_category: Option<String>,
    ) -> Self {
        Self {
            status: SyncStatus::Conflict,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts,
            commit_hash: None,
            error: Some(error),
            error_category: error_category.clone(),
            message_key: error_category
                .as_deref()
                .map(sync_error_category_to_message_key),
            conflict_summary: None,
            first_sync_mode: FirstSyncMode::NotAttempted,
            settings_conflicts: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
            search_index_rebuild_error: None,
        }
    }
}

fn sync_error_category_to_message_key(category: &str) -> String {
    let cat = SyncErrorCategory::from_code(category, "");
    cat.to_message_key().to_string()
}

/// 同步计划 — 三路/LWW 比较后、实际执行前的文件操作清单。
///
/// `files_to_upload`：本地较新需上传的文件。
/// `files_to_download`：远端较新需下载的文件。
/// `files_to_delete_local`：远端已删除、本地需移至 trash 的文件。
/// `files_to_delete_remote`：本地已删除、需从远端删除的文件。
/// `conflicts`：BothChanged 且未自动解决的路径，等待用户决策。
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
    /// 创建空同步计划——无上传/下载/删除/冲突。
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

/// 同步清单中的单条文件记录，持久化为 `manifest.sync.json`。
///
/// `op` 区分 upsert（新增/修改）和 delete（删除）两种操作。
/// `content_hash` 为 MD5 十六进制摘要，用于三路比较和变更检测。
/// `deleted_at_ms` 仅在 `op == "delete"` 时有值，记录精确的删除时间戳，
/// 优先于 `updated_at_ms` 作为 LWW 比较时间（见 `lww_record_time`）。
/// `device_id` 用于 LWW 平局决胜：时间戳相同时字典序大的 device_id 获胜。
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

/// 同步清单 — 持久化为 `app-meta/sync/manifest.sync.json`，记录所有已同步文件的元数据。
///
/// 本地和远端各维护一份 manifest，同步时交换比较。
/// `files` 中的 `content_hash` 用于三路比较和变更检测。
/// manifest 是 LWW 同步的唯一事实来源：所有文件的存在/删除/修改状态
/// 均以 manifest 记录为准，而非文件系统快照。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncManifest {
    pub files: Vec<ManifestFileRecord>,
}

/// 删除墓碑 — 记录已删除文件的信息，用于同步时通知远端。
///
/// 本地删除文件后不立即从 known_files 移除，而是创建墓碑，
/// 使下次同步能向远端发送 delete 操作。`purge_after` 过期后墓碑被清理。
/// `kind` 区分本地主动删除（local_delete）和远端删除同步到本地（remote_delete）。
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

/// 同步持久状态，保存为 `app-meta/sync/state.json`。
///
/// 非线程安全：只在同步引擎主路径读写，同步期间持有独占可变引用。
///
/// 字段关系：
/// - `known_files` + `known_files_updated_at` 是一对伴生映射，
///   同一个 path 在两个映射中必须同时存在或同时不存在。
///   `known_files[path]` 存储上次同步后的共识哈希（base_hash），
///   `known_files_updated_at[path]` 存储该条目的更新时间戳（毫秒），
///   两者共同构成 LWW 比较的基准。
/// - `conflicted_files` 中的路径在同步时被跳过，不参与三路/LWW 比较，
///   直到用户通过 resolve_conflict_* 显式解决。
/// - `pending_take_remote` 记录用户选择"采用远端"但远端内容尚未下载的路径，
///   下次 perform_sync 时强制下载这些路径后再进入正常比较流程。
/// - `tombstones` 记录本地已删除文件的墓碑，用于下次同步时向远端发送 delete 操作，
///   `purge_after` 过期后由同步引擎清理。
///
/// 同步完成后 known_files 会被 post-sync scan 重建，但冲突路径的 base_hash 会被保留，
/// 以确保下次同步仍能检测到 BothChanged。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub remote_url: Option<String>,
    pub transport: Option<SyncProtocol>,
    pub last_synced_commit: Option<String>,
    pub last_sync_time: Option<i64>,
    pub last_error: Option<String>,
    /// 三路比较基准：path → 上次同步后的共识哈希（MD5 hex）。
    pub known_files: std::collections::HashMap<String, String>,
    /// 已记录的冲突详情，供 resolve_conflict_* 查找 remote_hash
    pub conflicts: Vec<SyncConflict>,
    /// 已删除文件的墓碑记录，用于同步时生成本地 delete 操作。
    /// purge_after 过期后由同步引擎清理。
    #[serde(default)]
    pub tombstones: Vec<Tombstone>,
    /// 本地已删除的文件路径集合——记录因远端删除而同步移除的本地文件。
    /// 与 tombstones 的区别：tombstones 记录本地主动删除的文件（用于上传 delete 操作），
    /// deleted_files 记录因远端删除而本地移除的文件（用于跳过已删除文件的三路比较）。
    /// 两者不重叠：同一文件不会同时出现在两个集合中。
    #[serde(default)]
    pub deleted_files: std::collections::HashSet<String>,
    /// 本设备唯一标识，用于 LWW 平局决胜（字典序大的 device_id 获胜）。
    #[serde(default)]
    pub device_id: String,
    /// known_files 中各条目的更新时间戳（毫秒），用于 LWW 时间戳比较。
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
