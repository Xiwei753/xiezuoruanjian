//! 全量同步三段式编排 — Prepare → Transfer → Commit。
//!
//! 把全量同步从"整个流程持一把写锁"拆成三段，每段只持短锁，Transfer 阶段完全不持锁：
//!
//! 1. **Prepare**（短写锁）：写 `Syncing` 状态、加载 secrets 快照、枚举 targets、
//!    算出每个 target 的 `local_root`，产出 [`FullSyncPlan`]（owned，不依赖 core）。
//! 2. **Transfer**（不持锁）：用 plan 里的 secrets/config 创建 backend，对每个 target
//!    调 `backend.sync()`（网络 + 本地文件读写）。本模块的 [`run_transfer`] 是纯函数，
//!    不接触 [`crate::facade::WriterCore`]，调用方在 API 层释放锁后调用。
//! 3. **Commit**（短写锁）：聚合 [`FullSyncTransferResult`] → [`FullSyncResult`]，
//!    原子写终态 `FullSyncState`，成功类重建搜索索引。
//!
//! 本模块只放纯编排逻辑（无 `&self`、无锁、无磁盘状态读写）；
//! 持锁、持久化、搜索索引等副作用留在 `facade/sync_ops.rs` 的薄转发方法里。
//!
//! ## 聚合优先级
//!
//! [`aggregate_full_sync_result`] 按"需要用户处理的终态 > 可重试 > 成功"保留错误类型：
//! `Fatal/Error > Dirty > Conflict > Recoverable > Success`。`error` /
//! `error_category` / `message_key` 从与总体同优先级的第一个 dominant target 取得，
//! 避免"总体是认证失败、文案却拿到前一个网络错误"的错位。

use crate::sync::types::{SyncTarget, TargetSyncResult};

pub(crate) mod aggregate;
pub(crate) mod generation;
pub(crate) mod plan;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod transfer;
pub(crate) mod transfer_cleanup;
pub(crate) mod transfer_helpers;

pub use aggregate::{
    aggregate_full_sync_result, error_to_persist_status, transport_init_failure_error,
};
pub use plan::build_full_sync_target_plan;
pub use transfer::run_transfer;

// ──   generation 原子发布 helpers ──

// ── Plan / Transfer 结果 ──

/// Prepare 阶段产出 — Transfer 阶段需要的全部数据（owned，不依赖 core 锁）。
#[derive(Debug, Clone)]
pub struct FullSyncPlan {
    pub sync_policy: crate::sync::types::SyncPolicy,
    pub force_sync: bool,
    pub targets: Vec<PlannedTarget>,
    pub app_data_root: std::path::PathBuf,
    pub remote_catalog_snapshot: crate::sync::types::RemoteTargetCatalogSnapshot,
}

/// 单个 target 的执行计划 — target 元数据 + 本地根 + 分类标签。
#[derive(Debug, Clone)]
pub struct PlannedTarget {
    pub target: SyncTarget,
    pub local_root: std::path::PathBuf,
    pub staging_root: Option<std::path::PathBuf>,
    pub target_kind: crate::sync::types::PlannedTargetKind,
    pub project_id: Option<String>,
    pub target_live_root: std::path::PathBuf,
    #[allow(clippy::struct_field_names)]
    pub deleted_journal_token: Option<String>,
    pub deleted_lww: Option<DeletedTargetLww>,
    pub live_lww: Option<LiveTargetLww>,
    pub expected_delete_lww: Option<DeletedTargetLww>,
}

///   deleted target 的 LWW 决策元数据。
#[derive(Debug, Clone)]
pub struct DeletedTargetLww {
    pub deleted_at_ms: i64,
    pub device_id: String,
}

///   live project 的 LWW 决策元数据。
#[derive(Debug, Clone)]
pub struct LiveTargetLww {
    pub lww_time_ms: i64,
    pub device_id: String,
}

impl PlannedTarget {
    /// 是否为待删除 target（pending delete target）。
    pub fn is_deleted_target(&self) -> bool {
        self.target_kind.is_pending_deleted()
    }
}

///   无副作用共享 target planner 产生的生命周期候选。
#[derive(Debug, Clone)]
pub(crate) enum LifecycleCandidate {
    Live { lww: LiveTargetLww },
    Retry,
}

/// Transfer 阶段产出 — 各 target 的 `SyncResult`，待 Commit 聚合。
#[derive(Debug, Clone)]
pub struct FullSyncTransferResult {
    pub targets: Vec<TargetSyncResult>,
    pub generation_gc_result: Option<Result<(), String>>,
}
