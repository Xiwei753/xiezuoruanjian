//! # 同步服务模块 (Sync Service)
//!
//! 本模块是写作软件的同步服务实现，通过 `SyncProvider` trait 抽象远端存储后端，
//! 提供 provider-neutral 的同步算法。
//!
//! ## 主要功能
//!
//! - **远端同步**: 通过 `SyncProvider` trait 抽象 GitHub / Git HTTPS 传输后端；
//!   业务编排只有一套，传输/认证按 provider 适配。
//! - **对象版本比较**: `object_compare` 给出唯一裁决（相同跳过、单侧变更传播、
//!   双侧不同修改交用户、删除按版本化操作处理）。
//! - **Full Sync**: 固定远端 HEAD → 增量下载缺失 blob → 一次 `SaveTransaction`
//!   提交内容与对象索引 → 一次远端提交 → 按回执精确确认。
//! - **同步配置管理**: 管理远程 URL、认证信息、代理设置、分支等配置
//! - **冲突检测与解决**: 冲突只记录对象路径、双方版本/内容与共同基线。
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
//!     = 远端存储 Provider（传输适配器）
//!
//! sync/{object_compare,full_sync,staging,commit_helpers}
//!     = 唯一业务同步引擎
//! ```

#![allow(clippy::module_inception)]

/// Versioned App data-group catalog and legacy layout discovery.
pub mod app_data_group_catalog;
/// 同步取消令牌 — 平台无关的同步生命周期标记（Issue #729）。
pub mod cancellation_token;
pub(crate) mod chapter_mutation_plan;
pub(crate) mod commit_helpers;
pub mod config_store;
pub mod conflict;
/// 纯内容分类/三方比较（始终可用，不依赖 feature gate）。
pub mod content_class;
pub(crate) mod decision;
pub mod diagnostics;
pub mod full_sync;
pub mod full_sync_state;
pub(crate) mod full_sync_utils;
/// 同步哈希语义集中模块（MD5 内容哈希 / Git blob OID / 旧基线归一化）。
pub(crate) mod hash;
/// 成功 Core 事务留下的本地对象变更事实（同步版本投影的提交时间/设备来源）。
pub(crate) mod local_mutations;
pub mod lww;
pub(crate) mod manifest_delta;
pub(crate) mod object_compare;
///   待删除同步 target 的持久化（provider-neutral）。
pub mod path;
pub mod pending_deleted;
/// 待清理远端残留的持久化（provider-neutral）。
pub mod pending_remote_cleanup;
pub mod provider;
pub mod scanner;
pub mod service;
/// 稀疏对象计划（旧 lww 合并路径的只读参数，Issue #1066 后不再参与裁决）。
pub(crate) mod sparse_plan;
pub mod staging;
/// 跨 WriterCoreApi 实例串行化同一 sync root 的 state/conflict mutation（Issue #762）。
pub(crate) mod state_lock;
///   target 生命周期 catalog（远端持久、provider-neutral）。
pub mod target_lifecycle;
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
