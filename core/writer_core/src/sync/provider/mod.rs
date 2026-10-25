//! 同步 Provider 模块
//!
//! 本模块包含所有同步后端的实现：
//! - `model.rs` / `capabilities.rs` / `error.rs` / `memory.rs` - provider-neutral 契约层
//! - `github/` - GitHub API Provider 实现
//!
//! ## SyncProvider trait
//!
//! [`SyncProvider`] 是 provider-neutral 的远端同步契约，只描述远端对象的
//! list/read/write/delete 四个原语，不涉及 SyncConfig/SyncSecrets/SyncTransport。
//! LWW engine 通过此 trait 与具体后端解耦，GitHub/Memory 各自实现。

pub mod capabilities;
pub mod error;
pub mod git_https;
pub mod memory;
pub mod model;
pub mod router;

#[cfg(feature = "github-api")]
pub mod github;

/// Provider 配置选择 — provider-neutral 的强类型枚举。
///
/// 每个 Provider 的持久化配置定义在各自模块，通过此枚举统一容纳。
/// 序列化使用 internally tagged enum（`#[serde(tag = "type")]`），
/// 线格式示例：`{"type":"github","remote_url":"...","branch":"main",...}`。
///
/// 新增 Provider 时在此枚举追加变体，无需改动 `SyncConfig`/`SyncConfigDto` 通用字段。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderConfig {
    #[cfg(feature = "github-api")]
    #[serde(rename = "github")]
    GitHub(github::config::GitHubProviderConfig),
    #[serde(rename = "git_https")]
    GitHttps(git_https::GitHttpsProviderConfig),
}

/// Provider 密钥选择 — provider-neutral 的强类型枚举。
///
/// 敏感凭证不进 `ProviderConfig`（持久化到 config.json 不安全），
/// 由 `SyncSecrets.provider_secrets` 携带，构造 Provider 时注入。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderSecrets {
    /// 通用 Git HTTPS 密码 / PAT / API token。空串等同未提供（匿名读取）。
    #[serde(rename = "git_https")]
    GitHttps { secret: String },
    #[cfg(feature = "github-api")]
    #[serde(rename = "github")]
    GitHub { token: String },
}

/// Provider 边界的累计网络指标快照。
///
/// `set_operation_id()` 会切换上下文并清零当前累计值。字节数按 `metric_scope`
/// 说明的可观测范围统计，不代表网络线缆上的全部字节。
///
/// 当前 scope：
/// - `github_execute_body_bytes`：SyncTransport execute 次数及提交/返回的 HTTP body 字节；
///   不含 headers，也不含失败调用中未返回的响应字节。
/// - `git_https_provider_operations_pack_bytes_lower_bound`：Git provider 网络操作次数及
///   libgit2 回调报告的 pack 字节下界，不是逐 HTTP 请求数或完整 wire 字节数。
/// - `not_instrumented`：Provider 未实现网络计数。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProviderNetworkMetrics {
    pub request_count: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub operation_id: Option<String>,
    pub metric_scope: String,
}

impl Default for ProviderNetworkMetrics {
    fn default() -> Self {
        Self {
            request_count: 0,
            bytes_received: 0,
            bytes_sent: 0,
            operation_id: None,
            metric_scope: "not_instrumented".to_string(),
        }
    }
}

/// Provider 内部共享的指标累计器。只保存计数与 operation_id，不保存请求内容。
#[derive(Debug)]
pub(crate) struct ProviderNetworkAccounting {
    metric_scope: &'static str,
    state: std::sync::Mutex<ProviderNetworkMetrics>,
}

impl ProviderNetworkAccounting {
    pub(crate) fn new(metric_scope: &'static str) -> Self {
        Self {
            metric_scope,
            state: std::sync::Mutex::new(ProviderNetworkMetrics {
                metric_scope: metric_scope.to_string(),
                ..ProviderNetworkMetrics::default()
            }),
        }
    }

    /// 设置当前操作上下文并开始一轮新的累计窗口。
    pub(crate) fn set_operation_id(&self, operation_id: Option<&str>) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.request_count = 0;
        state.bytes_received = 0;
        state.bytes_sent = 0;
        state.operation_id = operation_id.map(str::to_string);
    }

    /// 记录一次 provider 请求尝试以及交给传输层的请求 body 字节数。
    pub(crate) fn record_request(&self, bytes_sent: usize) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.request_count = state.request_count.saturating_add(1);
        state.bytes_sent = state
            .bytes_sent
            .saturating_add(u64::try_from(bytes_sent).unwrap_or(u64::MAX));
    }

    pub(crate) fn record_bytes_received(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.bytes_received = state
            .bytes_received
            .saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
    }

    pub(crate) fn record_bytes_sent(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.bytes_sent = state
            .bytes_sent
            .saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
    }

    pub(crate) fn snapshot(&self) -> ProviderNetworkMetrics {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        ProviderNetworkMetrics {
            request_count: state.request_count,
            bytes_received: state.bytes_received,
            bytes_sent: state.bytes_sent,
            operation_id: state.operation_id.clone(),
            metric_scope: self.metric_scope.to_string(),
        }
    }
}

impl ProviderSecrets {
    /// 返回任意 Provider 的密钥；用于通用安全存储迁移。
    pub fn any_secret(&self) -> Option<&str> {
        match self {
            ProviderSecrets::GitHttps { secret } => Some(secret.as_str()),
            #[cfg(feature = "github-api")]
            ProviderSecrets::GitHub { token } => Some(token.as_str()),
        }
    }

    /// 返回可作为 GitHub API token 使用的密钥。
    ///
    /// 旧 GitHub 配置保存为 `GitHub` 变体；切换到通用 Git HTTPS 后，
    /// 同一串令牌仍可用于 `api.github.com` 自动后备。
    #[cfg(feature = "github-api")]
    pub fn github_token(&self) -> Option<&str> {
        self.any_secret().filter(|secret| !secret.is_empty())
    }
}

/// Provider-neutral 远端同步契约 — 所有同步后端必须满足此接口。
///
/// trait 只描述远端对象的 CRUD 原语，不携带 SyncConfig/SyncSecrets/SyncTransport,
/// 具体后端的认证/传输在构造 Provider 实例时注入（见 `GitHubProvider::new`）。
///
/// ## 方法语义
///
/// - `capabilities()`：返回远端能力集合，engine 据此调整策略。
/// - `remote_revision()`：读取分支当前提交标识，不支持此能力的 Provider 返回 `None`。
/// - `list(prefix)`：枚举远端以 `prefix + "/"` 开头的对象，剥掉前缀返回。
///   `prefix` 为空时返回全部。返回 `RemoteEntry`（path + version，无内容）。
/// - `read(path)`：读取远端对象完整内容，返回 `Option<RemoteObject>`（None 表示不存在）。
/// - `write(path, content, precondition)`：写入对象，返回新版本。
///   precondition 检查失败返回 `ProviderError::PreconditionFailed`。
/// - `delete(path, precondition)`：删除对象。
/// - `commit_batch(mutations, message)`：一次原子提交一组 mutation。
///   仅在 `capabilities().batch == true` 时保证实现；默认实现返回
///   `ProviderError::Other { "batch not supported" }`，让不支持 batch 的
///   Provider 仍能实现 trait 而不破坏现有调用方。调用方必须在 `batch == false`
///   时降级到逐文件 write/delete 路径，不调用此方法。
///
/// ## 线程安全
///
/// trait 要求 `Send + Sync`，方法接受 `&self`，实现内部处理并发。
/// 调用方可在多线程共享 `&dyn SyncProvider`。
pub trait SyncProvider: Send + Sync {
    /// 设置当前 operation_id 并重置本 Provider 的累计网络指标。
    ///
    /// 调用方可在一个同步 phase/run 开始前设置 ID，随后调用
    /// [`network_metrics`](Self::network_metrics) 读取该窗口内的累计值。
    fn set_operation_id(&self, _operation_id: Option<&str>) {}

    /// 读取自最近一次 [`set_operation_id`](Self::set_operation_id) 后累计的指标。
    fn network_metrics(&self) -> ProviderNetworkMetrics {
        ProviderNetworkMetrics::default()
    }

    /// 返回远端能力集合。
    fn capabilities(&self) -> capabilities::SyncCapabilities;

    /// 只读查询远端当前分支的版本标识。
    ///
    /// 默认返回 `None` 表示 Provider 不支持分支级版本查询；支持该能力的 Provider
    /// 在分支尚未创建时也可返回 `None`。查询失败必须返回错误，不能当作版本未变化。
    fn remote_revision(&self) -> Result<Option<String>, error::ProviderError> {
        Ok(None)
    }

    /// 枚举远端以 `prefix + "/"` 开头的对象，剥掉前缀返回。
    fn list(&self, prefix: &str) -> Result<Vec<model::RemoteEntry>, error::ProviderError>;

    /// 读取远端对象完整内容，不存在返回 `None`。
    fn read(&self, path: &str) -> Result<Option<model::RemoteObject>, error::ProviderError>;

    /// 写入对象，返回新版本。precondition 检查失败返回 `PreconditionFailed`。
    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: model::WritePrecondition,
    ) -> Result<model::RemoteVersion, error::ProviderError>;

    /// 删除对象。precondition 检查失败返回 `PreconditionFailed` 或 `NotFound`。
    fn delete(
        &self,
        path: &str,
        precondition: model::DeletePrecondition,
    ) -> Result<(), error::ProviderError>;

    /// 一次原子提交一组 mutation。
    ///
    /// 默认实现返回 `ProviderError::Other { "batch not supported" }`，
    /// 不支持 batch 的 Provider 不需重写。支持 batch 的 Provider（GitHub、Memory）
    /// 各自重写为 Git Database API / 锁内事务。
    ///
    /// 调用方必须在 `capabilities().batch == true` 时才调用此方法；
    /// `batch == false` 时应降级到逐文件 write/delete 路径。
    fn commit_batch(
        &self,
        mutations: &[model::BatchMutation],
        message: &str,
    ) -> Result<model::BatchCommitResult, error::ProviderError> {
        let _ = (mutations, message);
        Err(error::ProviderError::Other {
            reason: "batch not supported".to_string(),
        })
    }

    /// 远端连通性/认证诊断。不支持诊断的 Provider 返回 `Other`。
    fn diagnose(&self) -> Result<crate::sync::types::SyncDiagnosticsResult, error::ProviderError> {
        Err(error::ProviderError::Other {
            reason: "diagnostics not supported".to_string(),
        })
    }
}

// ---- Provider-neutral 契约层 re-export ----
pub use capabilities::*;
pub use error::*;
pub use git_https::{GitHttpsProvider, GitHttpsProviderConfig, GitHttpsRuntimeConfig};
pub use memory::*;
pub use model::*;

#[cfg(feature = "github-api")]
pub use github::*;
