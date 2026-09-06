//! # 同步服务模块 (Sync Service)
//!
//! 本模块是写作软件的同步服务实现，通过 `SyncProvider` trait 抽象远端存储后端，
//! 提供 provider-neutral 的同步算法。
//!
//! ## 主要功能
//!
//! - **远端同步**: 通过 `SyncProvider` trait 抽象 GitHub / 其它远端存储后端
//! - **LWW 冲突解决**: Last-Writer-Wins 引擎处理设置和元数据同步
//! - **Full Sync**: provider-neutral 的完整同步算法（prepare / transfer / commit）
//! - **同步配置管理**: 管理远程 URL、认证信息、代理设置、分支等配置
//! - **冲突检测与解决**: 提供文件冲突检测、设置冲突语义合并等功能
//! - **同步诊断**: 提供 provider 诊断信息（network_ok / auth_ok / remote_ok 等）
//! - **安全处理**: URL 凭证脱敏、敏感信息保护
//!
//! ## 边界
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

#![allow(clippy::module_inception)]

pub(crate) mod commit_helpers;
pub mod config_store;
pub mod conflict;
/// #644 评论 5473789298 第3节：纯内容分类/三方比较（始终可用，不依赖 feature gate）。
pub mod content_class;
pub mod diagnostics;
pub mod full_sync;
pub mod full_sync_state;
pub(crate) mod full_sync_utils;
pub mod lww;
/// #645 评论 5504296097 问题1：待删除同步 target 的持久化（provider-neutral）。
pub mod pending_deleted;
/// #645 评论 5504296097 问题3 修复：待清理远端残留的持久化（provider-neutral）。
pub mod pending_remote_cleanup;
pub mod provider;
pub mod scanner;
pub mod service;
pub mod staging;
/// #645 评论 5504296097 问题3：target 生命周期 catalog（远端持久、provider-neutral）。
pub mod target_lifecycle;
pub mod tests;
pub mod types;
pub mod url;
pub mod utils;

pub use provider::*;
pub use service::*;
pub use types::*;
pub use url::*;

#[cfg(test)]
mod api_tests;
