//! Journal 状态机模块
//!
//! 本模块包含持久化 journal 的状态机实现，用于崩溃恢复。

pub mod project_delete;
pub mod workspace_change;

pub use project_delete::*;
pub use workspace_change::{
    recover_unfinished, RecoveredWorkspaceChange, WorkspaceChangeJournal, WorkspaceChangeOpType,
    WorkspaceChangePhase,
};
