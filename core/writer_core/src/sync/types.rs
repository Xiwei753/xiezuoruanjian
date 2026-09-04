use serde::{Deserialize, Serialize};

/// 同步错误分类 — provider-neutral 纯枚举，不携带可变文案，不含 GitHub/Git 特定语义。
///
/// Issue #645 评论 5504296097 第1点：通用 core 只保留 provider-neutral 分类，
/// GitHub/Git 特定变体（TokenMissing/TokenInvalid/RepoNotFoundOrNoPermission/
/// BranchMissing/RemoteBranchMissing/NetworkProbeFailed/DnsFailed/TlsFailed/
/// NonFastForward/CheckoutConflict/LocalBlockingFile/UnrelatedHistories/
/// ApiRateLimited/ApiError/DirtyRepo/FileNotFound 等）已删除，
/// GitHub 401/403/404/409/422 在 `sync/provider/github/error.rs` 转成
/// 通用 `ProviderError` 变体（AuthFailed/PermissionDenied/NotFound/PreconditionFailed）。
///
/// 平台端通过 `to_ui_status()` 和 `to_message_key()` 做错误分类和 i18n 映射，
/// 不得依赖错误文案的包含范围作为主判断（见 AGENTS.md）。
/// `from_code()` 将字符串反序列化回枚举，只认识 provider-neutral code，未知 code
/// 统一映射为 `Other`；旧 GitHub/Git legacy code 兼容由 [`legacy_category_compat`]
/// 在 migration 边界显式处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SyncErrorCategory {
    #[default]
    None,
    /// 认证失败（token 无效/过期/缺失）。不可重试，需用户干预。
    AuthFailed,
    /// 权限不足（token 有效但无对应资源写/读权限）。不可重试，需用户干预。
    PermissionDenied,
    /// 远端对象不存在。不可重试。
    NotFound,
    /// 前置条件失败（乐观并发冲突）。不可重试，需上层拉取远端最新后重新决策或上报冲突。
    PreconditionFailed,
    /// 速率限制。可重试，等待 retry_after 后重试。
    RateLimited,
    /// 网络错误（DNS/TLS/连接失败/超时）。可重试。
    Network,
    /// 远端临时不可用（5xx 但非网络层错误）。可重试。
    TemporaryUnavailable,
    /// 本地 I/O 错误。可恢复（磁盘临时不可用等）。
    LocalIo,
    /// 冲突（双端修改、checkout 冲突等）。不可重试，需用户介入解决。
    Conflict,
    /// 兜底分类。保守视为可恢复，允许 engine 退避重试。
    Other,
}

impl SyncErrorCategory {
    /// 映射为 UI 状态字符串——供平台端决定同步状态图标和提示文案。
    /// 返回值是 API 契约，不可随意更改。旧 UI status 字符串（如 "token_missing"/
    /// "auth_failed"/"network_failed" 等）继续从新变体返回，保证平台端兼容。
    pub fn to_ui_status(&self) -> &'static str {
        match self {
            SyncErrorCategory::None => "error",
            SyncErrorCategory::AuthFailed => "auth_failed",
            SyncErrorCategory::PermissionDenied => "token_permission_denied",
            SyncErrorCategory::NotFound => "not_found",
            SyncErrorCategory::PreconditionFailed => "conflict",
            SyncErrorCategory::RateLimited => "error",
            SyncErrorCategory::Network | SyncErrorCategory::TemporaryUnavailable => {
                "network_failed"
            }
            SyncErrorCategory::LocalIo => "error",
            SyncErrorCategory::Conflict => "conflict",
            SyncErrorCategory::Other => "error",
        }
    }

    /// 映射为 i18n message key——供 UI 层做本地化映射。key 是 API 契约。
    pub fn to_message_key(&self) -> &'static str {
        match self {
            SyncErrorCategory::None => "sync.result.generic_error",
            SyncErrorCategory::AuthFailed => "sync.result.auth_failed",
            SyncErrorCategory::PermissionDenied => "sync.result.token_permission_denied",
            SyncErrorCategory::NotFound => "sync.result.auth_failed",
            SyncErrorCategory::PreconditionFailed => "sync.result.conflict_summary",
            SyncErrorCategory::RateLimited => "sync.result.generic_error",
            SyncErrorCategory::Network | SyncErrorCategory::TemporaryUnavailable => {
                "sync.result.network_failed"
            }
            SyncErrorCategory::LocalIo => "sync.result.generic_error",
            SyncErrorCategory::Conflict => "sync.result.conflict_summary",
            SyncErrorCategory::Other => "sync.result.generic_error",
        }
    }

    /// 从线格式 code 字符串反序列化。未知 code 映射为 `Other`。
    ///
    /// #645 评论 5504296097 第2点：`from_code` 只认识 provider-neutral code
    /// （`auth_failed`/`auth_error`/`permission_denied`/`token_permission_denied`/
    /// `missing_permission`/`not_found`/`precondition_failed`/`remote_sha_conflict`/
    /// `conflict`/`rate_limited`/`network`/`network_failed`/`network_error`/
    /// `temporary_unavailable`/`local_io`/`local_io_error`），不再识别旧
    /// GitHub/Git 特定 code。旧持久化数据中的 legacy code 由
    /// [`legacy_category_compat`] 在 migration 边界显式处理。
    pub fn from_code(code: &str, _fallback_msg: &str) -> Self {
        match code {
            "none" | "" => SyncErrorCategory::Other,
            // provider-neutral code
            "auth_failed" | "auth_error" => SyncErrorCategory::AuthFailed,
            "permission_denied" | "token_permission_denied" | "missing_permission" => {
                SyncErrorCategory::PermissionDenied
            }
            "not_found" => SyncErrorCategory::NotFound,
            "precondition_failed" | "remote_sha_conflict" | "conflict" => {
                SyncErrorCategory::PreconditionFailed
            }
            "rate_limited" => SyncErrorCategory::RateLimited,
            "network" | "network_failed" | "network_error" => SyncErrorCategory::Network,
            "temporary_unavailable" => SyncErrorCategory::TemporaryUnavailable,
            "local_io" | "local_io_error" => SyncErrorCategory::LocalIo,
            _ => SyncErrorCategory::Other,
        }
    }
}

/// 旧 GitHub/Git 特定 code → 新通用 `SyncErrorCategory` 的兼容映射（#645 评论 5504296097 第2点）。
///
/// `from_code` 只认识 provider-neutral code；本函数显式处理旧 GitHub/Git code
/// 字符串（`token_missing`/`token_invalid`/`github_unauthorized`/`github_forbidden`/
/// `empty_url`/`repo_not_found_or_no_permission`/`file_not_found`/`checkout_conflict`/
/// `local_blocking_file`/`api_rate_limited`/`network_probe_failed`/`github_network_failed`/
/// `dns_failed`/`tls_failed`/`api_error`/`branch_missing`/`remote_branch_missing`/
/// `non_fast_forward`/`unrelated_histories`/`dirty_repo`），供 migration 边界
/// （如加载旧 `SyncState.last_error` / 旧 diagnostics JSON）显式调用。
/// 不识别的 code 返回 `None`（调用方应回退到 `from_code` 或 `Other`）。
pub fn legacy_category_compat(code: &str) -> Option<SyncErrorCategory> {
    match code {
        "token_missing"
        | "token_invalid"
        | "github_unauthorized"
        | "github_forbidden"
        | "empty_url" => Some(SyncErrorCategory::AuthFailed),
        "repo_not_found_or_no_permission" => Some(SyncErrorCategory::PermissionDenied),
        "file_not_found" => Some(SyncErrorCategory::NotFound),
        "checkout_conflict" | "local_blocking_file" => Some(SyncErrorCategory::PreconditionFailed),
        "api_rate_limited" => Some(SyncErrorCategory::RateLimited),
        "network_probe_failed" | "github_network_failed" | "dns_failed" | "tls_failed" => {
            Some(SyncErrorCategory::Network)
        }
        "api_error" => Some(SyncErrorCategory::TemporaryUnavailable),
        "branch_missing"
        | "remote_branch_missing"
        | "non_fast_forward"
        | "unrelated_histories"
        | "dirty_repo" => Some(SyncErrorCategory::Other),
        _ => None,
    }
}

/// 同步范围 — 内部路径过滤语义，不再携带产品配置含义（Issue #630）。
///
/// 一个全局 `SyncConfig` + 一份全局凭据，`perform_full_sync` 内部按 `SyncTarget`
/// 把不同本地根映射到同一个远端仓库的不同前缀：
/// - `Project`：同步根为单个作品目录，白名单为作品正文/元数据。
/// - `App`：同步根为 `app_data_root`，白名单为设置/全局星图/主题调色板。
///
/// 该字段不暴露到 `SyncConfigDto`，由 `SyncTarget` 内部携带。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SyncScope {
    /// 作品级同步：单部作品正文、元数据、作品自己的同步状态。
    #[default]
    Project,
    /// 应用级同步：设置、全局星图、主题调色板。
    App,
}

/// 同步目标 — 一次全量同步中的一个本地根 → 远端前缀映射（Issue #630）。
///
/// 一个远端仓库内部按目录分流：
/// - App 目标固定 `remote_prefix = "app"`
/// - Project 目标固定 `remote_prefix = "projects/{project_id}"`
///
/// `scope` 仅用于本地路径白名单/黑名单过滤，不再决定 `SyncConfig` 的产品语义。
/// `remote_prefix` 用于远端 GitHub Contents API 路径拼装：
/// 所有远端路径统一走 `remote_prefix + "/" + local_relative_path`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncTarget {
    pub scope: SyncScope,
    pub remote_prefix: String,
}

impl SyncTarget {
    /// 应用级目标：本地根 = `app_data_root`，远端前缀 = `app`。
    pub fn app() -> Self {
        Self {
            scope: SyncScope::App,
            remote_prefix: "app".to_string(),
        }
    }

    /// 作品级目标：本地根 = `projects_root/<project_id>`，远端前缀 = `projects/<project_id>`。
    pub fn project(project_id: &str) -> Self {
        Self {
            scope: SyncScope::Project,
            remote_prefix: format!("projects/{}", project_id),
        }
    }

    /// 将本地相对路径映射为远端路径：`remote_prefix + "/" + local_relative_path`。
    pub fn remote_path(&self, local_relative_path: &str) -> String {
        format!("{}/{}", self.remote_prefix, local_relative_path)
    }
}

/// 同步配置 — 全局唯一，持久化为 `<app_data_root>/app-meta/sync/config.local.json`（Issue #630）。
///
/// 一次全量同步 = 设置 + 全局星图 + 主题调色板 + 全部作品。
/// App/Project 的区分由 `SyncTarget` 内部携带，`SyncConfig` 不再携带"我是应用同步还是作品同步"的产品配置含义。
///
/// 非线程安全：只在主线程读写，同步引擎在同步期间持有快照。
/// `sync_interval_seconds` 最小有效值为 60（引擎侧 clamp），0 表示仅手动同步。
///
/// 敏感字段（token）不在 SyncConfig 中，由 [`SyncSecrets`] 单独管理，
/// 平台端安全存储注入。
///
/// ## 通用字段 vs Provider 特定字段（Issue #645 评论第 2 点）
///
/// `enabled / auto_sync / sync_interval_seconds / active_provider / provider_config`
/// 为通用字段，所有 Provider 共用。GitHub 特定字段（remote_url / branch / username /
/// transport）只存在 `sync/provider/github/config.rs` 的 [`GitHubProviderConfig`] 中，
/// 通过 `provider_config: Option<ProviderConfig>` 容纳。
///
/// 旧 JSON 在 `facade/sync_config_ops.rs::load_sync_config` 边界一次性迁移：
/// 顶层 `remote_url`/`branch`/`username`/`transport`/`backend_type` 转换为
/// `provider_config = ProviderConfig::GitHub(...)`，保存新格式后旧字段不再出现。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncConfig {
    /// 是否启用同步
    pub enabled: bool,
    /// 活跃 Provider 类型（"github_api" 等）。
    /// 后续 WebDAV/CloudKit 等 Provider 接入后在此处选择。
    #[serde(default = "default_active_provider")]
    pub active_provider: String,
    /// Provider 特定持久化配置 — provider-neutral 强类型枚举。
    /// `None` 表示尚未配置具体 Provider（如刚启用同步但未填 GitHub URL）。
    #[serde(default)]
    pub provider_config: Option<crate::sync::provider::ProviderConfig>,
    /// 是否启用自动同步
    pub auto_sync: bool,
    /// 自动同步间隔（秒），最小有效值 60，0 表示仅手动
    pub sync_interval_seconds: u32,
    /// Whether the platform grants network access permission.
    /// Android sets this based on INTERNET permission; desktop platforms always true.
    #[serde(default = "default_true", alias = "android_has_internet_permission")]
    pub has_network_permission: bool,
    /// Whether the platform grants network state query permission.
    /// Android sets this based on ACCESS_NETWORK_STATE permission; desktop platforms always true.
    #[serde(
        default = "default_true",
        alias = "android_has_access_network_state_permission"
    )]
    pub has_network_state_permission: bool,
}

impl SyncConfig {
    /// 返回 GitHub remote_url（若 `provider_config` 为 GitHub 变体）；否则空字符串。
    #[cfg(feature = "github-api")]
    pub fn github_remote_url(&self) -> String {
        match &self.provider_config {
            Some(crate::sync::provider::ProviderConfig::GitHub(gh)) => gh.remote_url.clone(),
            None => String::new(),
        }
    }

    /// 非 github-api feature 下无 GitHub provider，统一返回空字符串。
    #[cfg(not(feature = "github-api"))]
    pub fn github_remote_url(&self) -> String {
        String::new()
    }

    /// 返回 GitHub branch（若 `provider_config` 为 GitHub 变体）；否则空字符串。
    #[cfg(feature = "github-api")]
    pub fn github_branch(&self) -> String {
        match &self.provider_config {
            Some(crate::sync::provider::ProviderConfig::GitHub(gh)) => gh.branch.clone(),
            None => String::new(),
        }
    }

    /// 非 github-api feature 下无 GitHub provider，统一返回 "main"。
    #[cfg(not(feature = "github-api"))]
    pub fn github_branch(&self) -> String {
        "main".to_string()
    }

    /// 设置 GitHub provider_config 字段（测试辅助）。
    #[cfg(test)]
    #[cfg(feature = "github-api")]
    pub fn set_github_config(
        &mut self,
        remote_url: String,
        branch: String,
        username: String,
        transport: crate::sync::provider::github::config::GitHubTransport,
    ) {
        self.provider_config = Some(crate::sync::provider::ProviderConfig::GitHub(
            crate::sync::provider::github::config::GitHubProviderConfig {
                remote_url,
                branch,
                username,
                transport,
            },
        ));
        self.active_provider = "github_api".to_string();
    }

    /// 非 github-api feature 下无 GitHub provider，`set_github_config` 为空操作。
    #[cfg(test)]
    #[cfg(not(feature = "github-api"))]
    pub fn set_github_config(
        &mut self,
        _remote_url: String,
        _branch: String,
        _username: String,
        _transport: (),
    ) {
        // github-api feature 未启用时无 GitHub provider 可设置。
    }
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_active_provider() -> String {
    "github_api".to_string()
}

/// 同步密钥 — 敏感凭证，不持久化到 config.json，由平台端安全存储注入（Issue #645 评论第 2 点）。
///
/// `provider_secrets` 为 provider-neutral 强类型枚举，当前仅 GitHub { token }。
/// 旧 `token`/`ssh_private_key` 顶层字段已删除，由 `ProviderSecrets::GitHub { token }` 容纳。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SyncSecrets {
    #[serde(default)]
    pub provider_secrets: Option<crate::sync::provider::ProviderSecrets>,
}

impl SyncSecrets {
    /// 返回 GitHub token（若 `provider_secrets` 为 GitHub 变体）；否则 None。
    #[cfg(feature = "github-api")]
    pub fn github_token(&self) -> Option<String> {
        self.provider_secrets
            .as_ref()
            .and_then(|s| s.github_token().map(|t| t.to_string()))
    }

    /// 非 github-api feature 下无 GitHub provider，统一返回 None。
    #[cfg(not(feature = "github-api"))]
    pub fn github_token(&self) -> Option<String> {
        None
    }

    /// 从 GitHub token 构造 `SyncSecrets`（github-api feature 下）。
    #[cfg(feature = "github-api")]
    pub fn from_github_token(token: String) -> Self {
        SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub { token }),
        }
    }

    /// 非 github-api feature 下无 GitHub provider，token 被丢弃，返回空 secrets。
    #[cfg(not(feature = "github-api"))]
    pub fn from_github_token(_token: String) -> Self {
        SyncSecrets::default()
    }

    /// 是否为空（无任何 Provider 密钥）。
    pub fn is_empty(&self) -> bool {
        self.provider_secrets.is_none()
    }
}

/// 通用同步策略 — LWW engine 所需的 provider-neutral 配置（Issue #645）。
///
/// 从 [`SyncConfig`] 转换而来，只携带 engine 决策需要的通用字段，
/// 不含任何 GitHub 特定参数（token/api_base_url 等在创建 Provider 时解析）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncPolicy {
    /// 是否启用同步。
    pub enabled: bool,
    /// 是否启用自动同步。
    pub auto_sync: bool,
    /// 自动同步间隔（秒），最小有效值 60，0 表示仅手动。
    pub sync_interval_seconds: u32,
    /// 平台是否授予网络访问权限。
    pub has_network_permission: bool,
}

impl SyncPolicy {
    /// 从 `SyncConfig` 转换为通用同步策略。
    pub fn from_config(config: &SyncConfig) -> Self {
        Self {
            enabled: config.enabled,
            auto_sync: config.auto_sync,
            sync_interval_seconds: config.sync_interval_seconds,
            has_network_permission: config.has_network_permission,
        }
    }
}

impl Default for SyncPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_sync: false,
            sync_interval_seconds: 60,
            has_network_permission: true,
        }
    }
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
    Error(String),
    NoChanges,
    LatestWinsApplied,
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

/// 同步诊断结果 — provider-neutral，逐步检查网络、认证、远端可达性。
///
/// #645 评论 5504296097 第2点：删除旧 Git/GitHub 远端假设字段
/// （`backend_type`/`repo_ok`/`branch_ok`/`repo_status`/`branch_status`/
/// `remote_url_sanitized`/`transport`），改为 provider-neutral 结构。
/// `provider_type` 标识 Provider 类型（"github_api"/"webdav"/"cloudkit" 等），
/// `remote_ok` 合并远端可达性，`provider_details` 由各 Provider 自行填充特定诊断详情。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDiagnosticsResult {
    pub success: bool,
    /// Provider 类型（"github_api"/"webdav"/"cloudkit" 等），替代旧 backend_type。
    pub provider_type: String,
    /// Whether the platform grants network access permission.
    #[serde(default, alias = "android_has_internet_permission")]
    pub has_network_permission: bool,
    /// Whether the platform grants network state query permission.
    #[serde(default, alias = "android_has_access_network_state_permission")]
    pub has_network_state_permission: bool,
    /// Current network connectivity state reported by the platform.
    #[serde(default, alias = "android_network_state")]
    pub network_state: String,
    pub network_ok: bool,
    pub auth_ok: bool,
    /// 远端可达性（替代旧 repo_ok+branch_ok）。
    pub remote_ok: bool,
    pub network_status: String,
    pub auth_status: String,
    /// Error category for sync failures
    pub error_category: String,
    pub raw_error: Option<String>,
    /// Provider 特定诊断详情（JSON 字符串或人类可读摘要），由各 Provider 自行填充。
    #[serde(default)]
    pub provider_details: Option<String>,
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
            provider_type: "unknown".to_string(),
            has_network_permission: true,
            has_network_state_permission: true,
            network_state: "unchecked".to_string(),
            network_ok: false,
            auth_ok: false,
            remote_ok: false,
            network_status: "unchecked".to_string(),
            auth_status: "unchecked".to_string(),
            error_category: "none".to_string(),
            raw_error: None,
            provider_details: None,
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
    pub error: Option<String>,
    pub error_category: Option<String>,
    pub message_key: Option<String>,
    pub conflict_summary: Option<SyncConflictSummary>,
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
            error: None,
            error_category: None,
            message_key: None,
            conflict_summary: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
            search_index_rebuild_error: None,
        }
    }

    /// 创建错误结果——status 应为 Error/Conflict 等终端状态，error_category 可选。
    pub fn error(status: SyncStatus, error: String, error_category: Option<String>) -> Self {
        let message_key = error_category
            .as_deref()
            .map(sync_error_category_to_message_key);
        Self {
            status,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts: Vec::new(),
            error: Some(error),
            error_category: error_category.clone(),
            message_key,
            conflict_summary: None,
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
            error: Some(error),
            error_category: error_category.clone(),
            message_key: error_category
                .as_deref()
                .map(sync_error_category_to_message_key),
            conflict_summary: None,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub last_sync_time: Option<i64>,
    pub last_error: Option<String>,
    /// 三路比较基准：path → 上次同步后的共识哈希（MD5 hex）。
    #[serde(default)]
    pub known_files: std::collections::HashMap<String, String>,
    /// 已记录的冲突详情，供 resolve_conflict_* 查找 remote_hash
    #[serde(default)]
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

/// 单个 target 的同步结果 — `perform_full_sync` 中一个本地根 → 远端前缀目标的输出。
///
/// `target_kind` 为 `"app"` 或 `"project"`；`project_id` 仅在 Project target 时有值。
/// `result` 为该 target 的 `SyncResult`；`error` 为该 target 执行失败时的错误描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSyncResult {
    pub target_kind: String,
    pub project_id: Option<String>,
    pub remote_prefix: String,
    pub result: SyncResult,
}

/// 单个 target 的 dry-run 计划。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSyncPlan {
    pub target_kind: String,
    pub project_id: Option<String>,
    pub remote_prefix: String,
    pub plan: SyncPlan,
}

/// 全量同步聚合结果 — 一次 `perform_full_sync` 的完整输出（Issue #630）。
///
/// `overall_status` 为总体状态（Success/PartialConflict/Error 等）：
/// - 所有 target 成功 → Success
/// - 部分 target 冲突 → PartialConflict
/// - 部分 target 错误 → Error
///
/// `targets` 为每个 target 的结果列表，顺序为 App target 在前、Project targets 在后。
/// `total_*` 为上传/下载/删除/冲突的聚合统计。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullSyncResult {
    pub overall_status: SyncStatus,
    pub targets: Vec<TargetSyncResult>,
    pub total_uploaded: u32,
    pub total_downloaded: u32,
    pub total_local_deletes: u32,
    pub total_remote_deletes: u32,
    pub total_overwritten: u32,
    pub total_ignored: u32,
    pub total_conflicts: u32,
    pub error: Option<String>,
    pub error_category: Option<String>,
    pub message_key: Option<String>,
}

/// 全量同步 dry-run 聚合结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullSyncDryRunResult {
    pub targets: Vec<TargetSyncPlan>,
    pub total_to_upload: u32,
    pub total_to_download: u32,
    pub total_to_delete_local: u32,
    pub total_to_delete_remote: u32,
    pub total_ignored: u32,
    pub total_conflicts: u32,
}

/// 全量同步诊断结果 — 只测一次仓库、分支、token（Issue #630）。
///
/// `diagnostics` 为单次诊断结果；`error` 为诊断失败时的错误描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullSyncDiagnosticsResult {
    pub diagnostics: SyncDiagnosticsResult,
}
