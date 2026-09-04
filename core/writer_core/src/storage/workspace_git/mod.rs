//! 本地 Git 版本历史基础设施（唯一 workspace repo）。
//!
//! 本模块提供 workspace 级别唯一 Git 仓库的本地 commit / diff / rollback / crash
//! recovery 能力。不处理远端 staging、remote refs、provider 或旧 Git sync finalize。
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

pub mod model;

pub use model::*;
