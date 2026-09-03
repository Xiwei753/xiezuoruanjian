//! 同步 Provider 模块
//!
//! 本模块包含所有同步后端的实现：
//! - `model.rs` / `capabilities.rs` / `error.rs` / `memory.rs` - provider-neutral 契约层（Issue #645）
//! - `github/` - GitHub API Provider 实现（Issue #645）
//!
//! ## SyncProvider trait
//!
//! [`SyncProvider`] 是 provider-neutral 的远端同步契约，只描述远端对象的
//! list/read/write/delete 四个原语，不涉及 SyncConfig/SyncSecrets/SyncTransport。
//! LWW engine 通过此 trait 与具体后端解耦，GitHub/Memory 各自实现。

pub mod capabilities;
pub mod error;
pub mod memory;
pub mod model;

#[cfg(feature = "github-api")]
pub mod github;

/// Provider 配置选择 — provider-neutral 的强类型枚举（Issue #645 评论第 2 点）。
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
}

/// Provider 密钥选择 — provider-neutral 的强类型枚举（Issue #645 评论第 2 点）。
///
/// 敏感凭证不进 `ProviderConfig`（持久化到 config.json 不安全），
/// 由 `SyncSecrets.provider_secrets` 携带，构造 Provider 时注入。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderSecrets {
    #[cfg(feature = "github-api")]
    #[serde(rename = "github")]
    GitHub { token: String },
}

impl ProviderSecrets {
    /// 返回 GitHub token（若为 GitHub 变体）；其他 Provider 返回 None。
    #[cfg(feature = "github-api")]
    pub fn github_token(&self) -> Option<&str> {
        match self {
            ProviderSecrets::GitHub { token } => Some(token.as_str()),
        }
    }
}

/// Provider-neutral 远端同步契约 — 所有同步后端必须满足此接口。
///
/// trait 只描述远端对象的 CRUD 原语，不携带 SyncConfig/SyncSecrets/SyncTransport，
/// 具体后端的认证/传输在构造 Provider 实例时注入（见 `GitHubProvider::new`）。
///
/// ## 方法语义
///
/// - `capabilities()`：返回远端能力集合，engine 据此调整策略。
/// - `list(prefix)`：枚举远端以 `prefix + "/"` 开头的对象，剥掉前缀返回路径。
///   `prefix` 为空时返回全部。返回 `RemoteEntry`（path + version，无内容）。
/// - `read(path)`：读取远端对象完整内容，返回 `Option<RemoteObject>`（None 表示不存在）。
/// - `write(path, content, precondition)`：写入对象，返回新版本。
///   precondition 检查失败返回 `ProviderError::PreconditionFailed`。
/// - `delete(path, precondition)`：删除对象。
///
/// ## 线程安全
///
/// trait 要求 `Send + Sync`，方法接受 `&self`，实现内部处理并发。
/// 调用方可在多线程共享 `&dyn SyncProvider`。
pub trait SyncProvider: Send + Sync {
    /// 返回远端能力集合。
    fn capabilities(&self) -> capabilities::SyncCapabilities;

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
}

// ---- Provider-neutral 契约层 re-export ----
pub use capabilities::*;
pub use error::*;
pub use memory::*;
pub use model::*;

#[cfg(feature = "github-api")]
pub use github::*;
