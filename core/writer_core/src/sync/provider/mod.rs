//! 同步 Provider 模块
//!
//! 本模块包含所有同步后端的实现：
//! - `backend.rs` - 同步后端抽象（SyncBackend trait）
//! - `git_backend.rs` - Git 同步后端（libgit2 实现）
//! - `github_backend.rs` - GitHub API 同步后端
//! - `github_api_client.rs` - GitHub API 客户端

pub mod backend;

#[cfg(feature = "git-https")]
pub mod git_backend;

#[cfg(feature = "github-api")]
pub mod github_backend;

#[cfg(feature = "github-api")]
pub mod github_api_client;

pub use backend::*;

#[cfg(feature = "git-https")]
pub use git_backend::*;

#[cfg(feature = "github-api")]
pub use github_backend::*;
