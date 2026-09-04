//! 本地 Git 版本历史基础设施（唯一 workspace repo）。
//!
//! 本模块提供 workspace 级别唯一 Git 仓库的 finalize / rollback / crash recovery 能力，
//! 不认识 remote、branch、clone/pull/push、Provider 或同步 staging。
//!
//! 最终边界：
//!
//! ```text
//! storage/workspace_git/*
//!     = 本地版本历史 / diff / rollback，唯一 workspace repo
//!
//! sync/provider/*
//!     = 远端存储 Provider
//!
//! sync/lww + full_sync + staging
//!     = provider-neutral 同步算法
//! ```

pub(crate) mod finalize;
pub(crate) mod locks;
pub mod model;
pub(crate) mod rollback;
pub mod seed;
pub(crate) mod tx;

pub use finalize::*;
pub use locks::*;
pub use model::*;
pub use rollback::*;
pub use seed::*;
pub use tx::*;

#[cfg(test)]
mod tests;
