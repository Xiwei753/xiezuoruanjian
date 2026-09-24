//! 编辑器快照 — 动画/事务的 old/new 状态记录单元。
//!
//! 仅携带 text/cursor/selection_anchor 的不可变快照，不维护 undo/redo 栈。
//! 正文真相由 EditorKernel 持有，CommittedTextMirror 作为 Qt 只读平台投影。
//!
//! cursor 和 selection_anchor 均为 UTF-8 byte offset。选区为半开区间 [start, end)。

/// 编辑器快照 — 动画/事务的 old/new 状态记录单元。
///
/// cursor 和 selection_anchor 均为 UTF-8 byte offset。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorSnapshot {
    pub text: String,
    pub cursor: usize,
    pub selection_anchor: usize,
}
