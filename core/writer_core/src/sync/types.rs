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

    /// #645 评论 5504296097 问题3：创建"无变更"结果——跳过删除（远端 LWW 胜出时）。
    pub fn no_changes() -> Self {
        Self {
            status: SyncStatus::NoChanges,
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
///
/// #645 评论 5504296097 问题1/2：`deleted_resolution` 仅 deleted_project target 有值，
/// `cleanup_completed_deleted_targets` 按此精确确认是否移除本地 `PendingDeletedTarget`，
/// 不再按 `SyncStatus` 猜。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSyncResult {
    pub target_kind: String,
    pub project_id: Option<String>,
    pub remote_prefix: String,
    pub result: SyncResult,
    /// #645 评论 5504296097 问题1/2：deleted target 的 LWW 决策结果。
    ///
    /// 仅 `target_kind == "deleted_project"` 时有值。`cleanup_completed_deleted_targets`
    /// 按此精确确认：`LocalDeleteWins` 且远端删除+catalog tombstone 写入成功才移除 pending；
    /// `RemoteTargetWins` 且本地恢复成功才移除 pending；`Retry` 保留。
    #[serde(default)]
    pub deleted_resolution: Option<DeletedTargetResolution>,
    /// #645 评论 5504296097 问题1：本地 lifecycle commit action —
    /// Transfer 阶段产出，Commit 阶段执行。
    ///
    /// - `None`：无 lifecycle action，走普通 staging commit；
    /// - `DeleteProject { project_id }`：远端 delete 胜出，Commit 阶段执行完整
    ///   Project 本地删除事务（move worktree / unbind starmaps / history），
    ///   **不**生成 `PendingDeletedTarget`（不反向要求删远端，远端已删）；
    /// - `RestoreProject { project_id }`：预留，当前 RestoreProject 在 Transfer 直接下载。
    #[serde(default)]
    pub local_lifecycle_action: LocalLifecycleCommitAction,
}

/// #645 评论 5504296097 问题1：本地 lifecycle commit action —
/// Transfer 阶段产出，Commit 阶段执行。
///
/// 把"删除本地 project"从 Transfer 阶段（裸 `remove_dir_all`）移到 Commit 阶段
/// （完整业务删除事务），避免 staging commit 把刚删掉的旧作品重新写回来。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LocalLifecycleCommitAction {
    /// 无 lifecycle action，走普通 staging commit。
    #[default]
    None,
    /// 远端 delete 胜出，Commit 阶段执行完整 Project 本地删除事务。
    DeleteProject {
        /// 被删除的 project id。
        project_id: String,
    },
    /// 预留：RestoreProject 在 Commit 阶段恢复本地 project。
    RestoreProject {
        /// 被恢复的 project id。
        project_id: String,
    },
}

/// #645 评论 5504296097 问题2修复：本地 lifecycle commit 的完整 receipt。
///
/// `commit_full_sync` 返回 `Vec<LocalLifecycleCommitReceipt>`，每个元素对应一个
/// RemoteLifecycle 删除事务。API 层用 `change_set` 调 `record_workspace_change_set`
/// 记本地 history，成功后调 `ack_project_delete_history` 推进 journal 到
/// `HistoryRecorded` → `Completed`（RemoteLifecycle origin 跳过 `RemoteDeleteQueued`）。
/// history 失败时 journal 保留在 `StarMapsUnbound`，下次启动 recover 补记。
#[derive(Debug, Clone)]
pub struct LocalLifecycleCommitReceipt {
    /// 本次删除的 journal token（用于 ack 推进 journal）。
    pub journal_token: String,
    /// 删除产生的 workspace 变更集（DeleteTree + 解绑 starmap 的 Upsert）。
    pub change_set: crate::storage::workspace_git::WorkspaceChangeSet,
    /// 本次被解绑的 starmap ids（供调用方刷搜索索引等）。
    pub unbound_starmap_ids: Vec<String>,
    /// 删除发起来源（User/RemoteLifecycle）。
    pub origin: crate::project::ProjectDeleteOrigin,
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

/// 待删除的同步 target — provider-neutral 持久状态（Issue #645 评论 5504296097 问题1）。
///
/// 整部作品删除后，远端 `projects/<project_id>/` 下的对象需要被清理。
/// 但 `prepare_full_sync` 只通过 `list_projects()` 枚举现存作品生成 target，
/// 已删除作品不在列表里，远端前缀没有任何 target 会去清理。
///
/// `PendingDeletedTarget` 是 sync engine 自己的 provider-neutral 持久状态
/// （**不是** project tombstone），记录"这个 SyncTarget 已删除，下次同步时
/// 需要走 target-delete 计划清理远端前缀下所有对象"。
///
/// ## 生命周期
///
/// 1. `delete_project_with_changes` 时记录：把 `PendingDeletedTarget` 持久化到
///    `<app_data_root>/app-meta/sync/pending_deleted_targets.json`；
/// 2. `prepare_full_sync` 加载 pending deleted targets，加入 `FullSyncPlan.targets`，
///    `target_kind = "deleted_project"`；
/// 3. `run_transfer` 对 `deleted_project` target 走 target-delete 计划：
///    `provider.list(remote_prefix)` 枚举远端对象 → 逐个 `provider.delete(...)`；
/// 4. 全部远端删除成功后从 pending 列表移除该条目。
///
/// ## provider-neutral
///
/// 只使用 `SyncProvider::list/delete` 和 capabilities，不写 GitHub 专用逻辑。
/// GitHub 的 SHA/branch/API 细节由 `GitHubProvider` 自己处理。
///
/// #645 评论 5504296097 问题3：`deleted_at_ms` / `device_id` 参与 LWW 决策。
/// `run_deleted_target_sync` 先读远端 manifest，用 `deleted_at_ms` 与远端
/// manifest 的 `max(lww_record_time)` 比较，本地 tombstone 胜出才删远端，
/// 远端更晚则不删（远端有更新，下次正常 sync 会下载恢复）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDeletedTarget {
    /// 已删除的同步目标 — `SyncTarget::project(project_id)`。
    pub target: SyncTarget,
    /// 删除时间戳（Unix 毫秒）。
    ///
    /// #645 评论 5504296097 问题3：参与 LWW 决策，不再只用于排序和日志。
    /// 与远端 manifest 的 `max(lww_record_time)` 比较，本地 tombstone 胜出才删远端。
    pub deleted_at_ms: i64,
    /// 关联的 delete journal token，用于 ack 推进 journal。
    pub journal_token: String,
    /// 发起删除的设备 ID，用于 LWW 平局决胜。
    ///
    /// #645 评论 5504296097 问题3：时间戳相同时，字典序大的 device_id 获胜
    /// （与 `sync/lww/compare.rs::resolve_lww_path` 的 tie-break 规则一致）。
    /// `#[serde(default)]` 保持向后兼容：旧文件反序列化得到空字符串。
    #[serde(default)]
    pub device_id: String,
    /// 需要在远端删除的相对路径列表（相对于 `target.remote_prefix`）。
    ///
    /// `None` 表示删除整个 `remote_prefix` 下所有远端对象（由 `provider.list` 枚举）；
    /// `Some(paths)` 表示只删除指定路径（精确删除，不枚举远端）。
    /// 当前实现统一用 `None`（枚举远端前缀下所有对象逐个删除），
    /// `Some` 留作未来精确删除优化的扩展点。
    #[serde(default)]
    pub paths: Option<Vec<String>>,
}

impl PendingDeletedTarget {
    /// 为已删除的 project 构造 `PendingDeletedTarget`。
    ///
    /// `paths` 为 `None`：删除整个 `projects/<project_id>/` 前缀下所有远端对象。
    ///
    /// #645 评论 5504296097 问题3：`device_id` 参与 LWW 决策，必传。
    pub fn for_project(
        project_id: &str,
        deleted_at_ms: i64,
        journal_token: &str,
        device_id: &str,
    ) -> Self {
        Self {
            target: SyncTarget::project(project_id),
            deleted_at_ms,
            journal_token: journal_token.to_string(),
            device_id: device_id.to_string(),
            paths: None,
        }
    }
}

// ── #645 评论 5504296097 问题3：Target 生命周期 catalog（远端持久、provider-neutral） ──

/// target 生命周期操作类型 — provider-neutral，持久化到远端 catalog。
///
/// #645 评论 5504296097 问题3：`TargetLifecycleRecord` 记录单个 sync target
/// （如 `projects/<project_id>`）的生命周期操作，让离线旧设备上线时能读到
/// target 的 delete tombstone，不会把本地旧 project 重新上传（P 被复活）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetOp {
    Upsert,
    Delete,
}

impl TargetOp {
    /// 线格式字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            TargetOp::Upsert => "upsert",
            TargetOp::Delete => "delete",
        }
    }
}

/// target 生命周期记录 — 远端持久、provider-neutral。
///
/// 持久化到远端 `app/app-meta/sync/targets.sync.json`（app target 的 remote_prefix 下，
/// 不会随 `projects/<id>/` 一起被删除）。catalog 只通过 `SyncProvider::read/write` 操作，
/// GitHub SHA / WebDAV ETag 等留在 Provider 里。
///
/// ## 与 `PendingDeletedTarget` 的职责区分
///
/// - 本地 `pending_deleted_targets.json`（`PendingDeletedTarget`）：负责
///   "本机删除事务还没同步完成"，本机状态。
/// - 远端 `targets.sync.json`（`TargetLifecycleRecord`）：负责
///   "跨设备都必须知道这个 target 的生命周期"，跨设备共识。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetLifecycleRecord {
    /// target 标识，如 `"projects/<project_id>"`，与 `SyncTarget.remote_prefix` 对齐。
    pub target_id: String,
    /// 远端前缀，如 `"projects/<project_id>"`。
    pub remote_prefix: String,
    /// 操作类型：upsert（target 存在）/ delete（target 已删除）。
    pub op: TargetOp,
    /// 记录更新时间（Unix 毫秒）。
    pub updated_at_ms: i64,
    /// 删除时间（仅 `op == Delete` 时有值），优先于 `updated_at_ms` 作为 LWW 时间。
    #[serde(default)]
    pub deleted_at_ms: Option<i64>,
    /// 发起操作的设备 ID，用于 LWW tie-break（字典序大的胜出，与 `resolve_lww_path` 同规则）。
    #[serde(default)]
    pub device_id: String,
    /// schema 版本。
    #[serde(default = "default_target_catalog_schema_version")]
    pub schema_version: u32,
}

fn default_target_catalog_schema_version() -> u32 {
    1
}

impl TargetLifecycleRecord {
    /// 构造一条 upsert 记录。
    pub fn upsert(
        target_id: &str,
        remote_prefix: &str,
        updated_at_ms: i64,
        device_id: &str,
    ) -> Self {
        Self {
            target_id: target_id.to_string(),
            remote_prefix: remote_prefix.to_string(),
            op: TargetOp::Upsert,
            updated_at_ms,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            schema_version: 1,
        }
    }

    /// 构造一条 delete tombstone 记录。
    pub fn delete(
        target_id: &str,
        remote_prefix: &str,
        deleted_at_ms: i64,
        device_id: &str,
    ) -> Self {
        Self {
            target_id: target_id.to_string(),
            remote_prefix: remote_prefix.to_string(),
            op: TargetOp::Delete,
            updated_at_ms: deleted_at_ms,
            deleted_at_ms: Some(deleted_at_ms),
            device_id: device_id.to_string(),
            schema_version: 1,
        }
    }
}

/// target 生命周期 catalog 容器 — 持久化到远端 `targets.sync.json`。
///
/// `records` 按 `target_id` 唯一（合并/upsert 后）。LWW 合并规则：
/// 按 `target_id` 分组，取 `(lww_time, device_id)` 最大的记录
/// （与 `resolve_lww_path` 同规则）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TargetLifecycleCatalog {
    #[serde(default)]
    pub records: Vec<TargetLifecycleRecord>,
}

/// #645 评论 5504296097 问题4：带远端版本的 catalog 快照。
///
/// `load_remote_catalog` 返回此结构，携带远端当前版本。
/// `write_remote_catalog` 用 `version` 做 CAS 写入（`IfMatch`），
/// 防止多设备并发覆盖彼此的 lifecycle record。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTargetCatalogSnapshot {
    pub catalog: TargetLifecycleCatalog,
    /// 远端 `targets.sync.json` 的当前版本标识。
    /// `None` 表示文件不存在（首次写应用 `CreateNew`）。
    pub version: crate::sync::provider::model::RemoteVersion,
}

// ── #645 评论 5504296097 问题1/2：DeletedTargetResolution ──

/// deleted target 的 LWW 决策结果 — provider-neutral typed resolution。
///
/// #645 评论 5504296097 问题1/2：`run_deleted_target_sync` 做完 target-level LWW
/// 后返回这个 typed resolution（而非 `SyncResult`），`cleanup_completed_deleted_targets`
/// 按此精确确认是否移除本地 `PendingDeletedTarget`，不再按 `SyncStatus` 猜。
///
/// - `LocalDeleteWins` → 执行远端删除 + catalog 写 delete tombstone → 才移除 pending；
/// - `RemoteTargetWins` → 下载远端到 staging → commit 恢复本地 project → 才移除 pending；
/// - `Retry` → 什么都不删/恢复 → pending 保留（manifest/读取失败时）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletedTargetResolution {
    /// 本地 delete 胜出：执行远端删除 + catalog 写 delete tombstone。
    LocalDeleteWins,
    /// 远端 target 胜出：下载远端内容到 staging，commit 恢复本地 project。
    RemoteTargetWins,
    /// 无法确定（manifest/读取失败）：不删不恢复，pending 保留。
    Retry,
}

// ── #645 评论 5504296097 问题1：PlannedTargetKind ──

/// 计划阶段 target 的明确类型 — 替代字符串 `target_kind`，强类型决策结果。
///
/// #645 评论 5504296097 问题1：`build_full_sync_target_plan` 按 `target_id` 合并
/// local live project / local pending delete / remote lifecycle record，
/// 生成此明确类型，`run_transfer` 按此走对应执行路径。
///
/// ## 决策语义
///
/// - `App`：app target，正常 LWW 同步。
/// - `LiveProject`：本地 live project，远端无 delete tombstone 或本地更新 → 正常 upsert 同步。
/// - `DeleteRemoteProject`：本地 pending delete，本地 tombstone 胜出 → 删远端对象 + 写 delete tombstone。
/// - `DeleteLocalProject`：本地 live project，远端 delete tombstone 更新 → 不上传，本地 project 应删除。
/// - `RestoreProject`：本地 pending delete，远端 upsert 更新 → 下载远端恢复本地 project。
/// - `Retry`：无法决策（catalog/manifest 读取失败）→ 不删不恢复，pending 保留。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedTargetKind {
    App,
    LiveProject,
    DeleteRemoteProject,
    DeleteLocalProject,
    RestoreProject,
    Retry,
}

impl PlannedTargetKind {
    /// 转为 `TargetSyncResult.target_kind` 的字符串标识（API 契约，不可随意更改）。
    ///
    /// - `App` → `"app"`
    /// - `LiveProject` / `DeleteLocalProject` → `"project"`（都是 live project target）
    /// - `DeleteRemoteProject` / `RestoreProject` / `Retry` → `"deleted_project"`（都是 pending delete target）
    pub fn as_target_kind_str(&self) -> &'static str {
        match self {
            PlannedTargetKind::App => "app",
            PlannedTargetKind::LiveProject | PlannedTargetKind::DeleteLocalProject => "project",
            PlannedTargetKind::DeleteRemoteProject
            | PlannedTargetKind::RestoreProject
            | PlannedTargetKind::Retry => "deleted_project",
        }
    }

    /// 是否为待删除 target（pending delete target）。
    ///
    /// `DeleteRemoteProject` / `RestoreProject` / `Retry` 为 true（都是 pending delete target）。
    pub fn is_pending_deleted(&self) -> bool {
        matches!(
            self,
            PlannedTargetKind::DeleteRemoteProject
                | PlannedTargetKind::RestoreProject
                | PlannedTargetKind::Retry
        )
    }
}

// ── #645 评论 5504296097 问题2：TargetLifecycleApplyResult ──

/// provider-neutral 原子决策接口的返回值 — catalog CAS 写入后的决策结果。
///
/// #645 评论 5504296097 问题1修复：原 `LostToRemote(snapshot)` 让调用方猜 op 反转，
/// 导致 LWW 相等时被误判为"远端 delete 赢"或"远端 upsert 赢"。新枚举明确四种结果：
///
/// - `Applied(snapshot)` → candidate 严格赢，merge + IfMatch 写成功，携带持久化后的完整 snapshot；
/// - `AlreadyCurrent(snapshot)` → candidate 与远端 record 完全相等（同 op/同时间/同 device_id），
///   不需要再写 catalog，调用方按 candidate.op 继续后续动作（不删本地/不恢复）；
/// - `RemoteWinner { snapshot, record }` → 远端严格赢，携带远端最新 snapshot 和**真实**赢的 record，
///   调用方按 `record.op` 决策（Delete → 删本地；Upsert → 恢复本地），不再猜 op 反转；
/// - `Retry(err)` → 无法写入（重试耗尽/网络错误等），不删任何远端文件，pending 保留。
#[derive(Debug)]
pub enum TargetLifecycleApplyResult {
    /// candidate 严格赢，CAS 写成功，携带持久化后的完整 snapshot。
    Applied(RemoteTargetCatalogSnapshot),
    /// candidate 与远端 record 完全相等，不需要再写 catalog。
    AlreadyCurrent(RemoteTargetCatalogSnapshot),
    /// 远端严格赢，携带远端最新 snapshot 和真实赢的 record（含 op）。
    RemoteWinner {
        snapshot: RemoteTargetCatalogSnapshot,
        record: TargetLifecycleRecord,
    },
    /// 无法写入（重试耗尽/网络错误等），不删任何远端文件，pending 保留。
    Retry(crate::Error),
}

impl TargetLifecycleApplyResult {
    /// 取出内嵌的 snapshot（Applied/AlreadyCurrent/RemoteWinner 共用）。
    ///
    /// `Retry` 返回 `None`（无 snapshot）。供调用方统一更新本地 catalog 缓存。
    pub fn snapshot(&self) -> Option<&RemoteTargetCatalogSnapshot> {
        match self {
            TargetLifecycleApplyResult::Applied(s)
            | TargetLifecycleApplyResult::AlreadyCurrent(s) => Some(s),
            TargetLifecycleApplyResult::RemoteWinner { snapshot, .. } => Some(snapshot),
            TargetLifecycleApplyResult::Retry(_) => None,
        }
    }
}
