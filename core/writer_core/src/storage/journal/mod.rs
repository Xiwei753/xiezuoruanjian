//! Journal 状态机模块
//!
//! 本模块包含持久化 journal 的状态机实现，用于崩溃恢复。

pub mod project_delete;

pub use project_delete::*;
