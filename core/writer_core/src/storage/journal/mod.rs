//! Journal 状态机模块
//!
//! 本模块包含持久化 journal 的状态机实现，用于崩溃恢复。

pub mod project_delete;
pub mod starmap_child_embed;
pub mod starmap_delete;
pub mod starmap_object_delete;
pub mod workspace_change;

pub use project_delete::*;
pub use starmap_child_embed::*;
pub use starmap_delete::{apply_planned_delete_starmap, plan_delete_starmap, PlannedStarmapDelete};
pub use starmap_object_delete::{
    PlannedStarMapObjectDelete, StarMapObjectDeletePhase, StarMapObjectDeleteTarget,
    StarMapObjectKind,
};
pub use workspace_change::{
    ensure_sync_tombstones_from_facts, recover_unfinished, RecoveredWorkspaceChange,
    WorkspaceChangeJournal, WorkspaceChangeOpType, WorkspaceChangePhase,
};
