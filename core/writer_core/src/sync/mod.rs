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

/// 同步取消令牌 — 平台无关的同步生命周期标记（Issue #729）。
pub mod cancellation_token;
pub(crate) mod commit_helpers;
pub mod config_store;
pub mod conflict;
/// 纯内容分类/三方比较（始终可用，不依赖 feature gate）。
pub mod content_class;
pub mod diagnostics;
pub mod full_sync;
pub mod full_sync_state;
pub(crate) mod full_sync_utils;
///   generation GC（provider-neutral 清理未引用 generation）。
pub mod generation_gc;
/// 同步哈希语义集中模块（MD5 内容哈希 / Git blob OID / 旧基线归一化）。
pub(crate) mod hash;
pub mod lww;
///   待删除同步 target 的持久化（provider-neutral）。
pub mod path;
pub mod pending_deleted;
/// 待清理远端残留的持久化（provider-neutral）。
pub mod pending_remote_cleanup;
pub mod provider;
pub mod scanner;
pub mod service;
pub mod staging;
/// 跨 WriterCoreApi 实例串行化同一 sync root 的 state/conflict mutation（Issue #762 评论 5834136935）。
pub(crate) mod state_lock;
///   target 生命周期 catalog（远端持久、provider-neutral）。
pub mod target_lifecycle;
pub mod tests;
pub mod types;
pub mod url;

pub use provider::*;
pub use service::*;
pub use types::*;
pub use url::*;

// 同步取消令牌是同步语义的一部分，顶层 re-export 方便平台层使用。
pub use cancellation_token::SyncCancellationToken;
// 同步进度 sink 同属同步语义（Issue #763），顶层 re-export 方便平台层与诊断包使用。
pub use cancellation_token::{SyncProgressSink, SyncTargetProgressDto};

#[cfg(test)]
mod api_tests;
