//! 同步 Provider 模块
//!
//! 本模块包含所有同步后端的实现：
//! - `model.rs` / `capabilities.rs` / `error.rs` / `memory.rs` - provider-neutral 契约层（Issue #645）
//! - `git_backend.rs` - Git 同步后端（libgit2 实现）
//! - `github/` - GitHub API Provider 实现（Issue #645）
//!
//! ## SyncProvider trait
//!
//! [`SyncProvider`] 是 provider-neutral 的远端同步契约，只描述远端对象的
//! list/read/write/delete 四个原语，不涉及 SyncConfig/SyncSecrets/SyncTransport。
//! LWW engine 通过此 trait 与具体后端解耦，GitHub/Git/Memory 各自实现。

pub mod capabilities;
pub mod error;
pub mod memory;
pub mod model;

#[cfg(feature = "git-https")]
pub mod git_backend;

#[cfg(feature = "github-api")]
pub mod github;

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

#[cfg(feature = "git-https")]
pub use git_backend::*;

#[cfg(feature = "github-api")]
pub use github::*;
