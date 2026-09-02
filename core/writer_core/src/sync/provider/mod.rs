//! 同步 Provider 模块
//!
//! 本模块包含所有同步后端的实现：
//! - `backend.rs` - 同步后端抽象（SyncBackend trait）
//! - `git_backend.rs` - Git 同步后端（libgit2 实现）
//! - `github_backend.rs` - GitHub API 同步后端
//! - `github_api_client.rs` - GitHub API 客户端

mod backend;
mod git_backend;
mod github_backend;
mod github_api_client;

pub use backend::*;
pub use git_backend::*;
pub use github_backend::*;
pub use github_api_client::*;
