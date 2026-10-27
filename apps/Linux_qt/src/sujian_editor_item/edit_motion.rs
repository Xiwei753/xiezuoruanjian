//! Issue #904：Linux 私有编辑几何与会话类型。
//!
//! 旧 `EditorVisualTransaction` / `EditorAnimationKind` / `PreparedEditMotion` /
//! `diff_plain_text` 等旧动画数据模型已随 `TextMotion` 单一局部计划删除；
//! 本模块只保留仍被现有路径使用的类型：
//! - `CompositionSession`：IME 组合会话的虚拟文本投影。
//! - `CursorRect`：纯几何值类型，供现有光标 Tween 使用。
//! - `synthetic_edit_result`：`load_text` 等不经过 `kernel.apply()` 路径的合成结果。

use writer_core::editor::{
    EditorCursor, EditorEditResult, EditorOperationKind, EditorRevision, EditorSelection,
    EditorTransactionCause, OffsetMap,
};

// ── CompositionSession ──────────────────────────────────────────────────────

/// Issue #735: Linux 私有组合会话 — 替代已变为 `pub(crate)` 的 Core `CompositionSessionState`。
///
/// 存储平台端所需的组合会话数据：replace range、base text、preedit text。
///
/// Issue #826: preedit 是独立临时显示层。这个 session 只负责维护虚拟文本
/// （base text 的 replace range 被 preedit 替换后的结果）和 preedit 的 byte
/// range，不参与正文动画。
#[derive(Clone, Debug)]
pub(crate) struct CompositionSession {
    pub replace_start: usize,
    pub replace_end_exclusive: usize,
    pub base_text: String,
    pub preedit_text: String,
}

impl CompositionSession {
    pub fn new(text: String, cursor: usize) -> Self {
        Self {
            replace_start: cursor,
            replace_end_exclusive: cursor,
            base_text: text,
            preedit_text: String::new(),
        }
    }

    pub fn new_with_replace_range(text: String, start: usize, end: usize) -> Self {
        Self {
            replace_start: start,
            replace_end_exclusive: end,
            base_text: text,
            preedit_text: String::new(),
        }
    }

    /// Issue #826: 更新 preedit 文本。preedit 是独立临时显示层，只改虚拟文本投影，
    /// 不产生任何正文动画状态。
    pub fn update_preedit(&mut self, text: &str) {
        self.preedit_text = text.to_string();
    }

    /// 返回虚拟文本 — base text 中 [replace_start, replace_end_exclusive) 被
    /// preedit_text 替换后的结果。
    pub fn virtual_text(&self) -> String {
        let mut result = String::with_capacity(self.base_text.len() + self.preedit_text.len());
        let start = self.replace_start.min(self.base_text.len());
        let end = self.replace_end_exclusive.min(self.base_text.len());
        result.push_str(&self.base_text[..start]);
        result.push_str(&self.preedit_text);
        result.push_str(&self.base_text[end..]);
        result
    }

    /// 返回 preedit 在 virtual text 中的 byte range。
    pub fn preedit_byte_range_in_virtual_text(&self) -> (usize, usize) {
        (
            self.replace_start,
            self.replace_start + self.preedit_text.len(),
        )
    }
}

// ── CursorRect ──────────────────────────────────────────────────────────────

/// Linux 私有光标矩形 — 纯几何值类型。
///
/// 坐标空间：文档逻辑坐标系（不含滚动偏移）。scene graph 渲染时按当前
/// `scroll_y` 做 viewport transform。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CursorRect {
    pub x: f64,
    pub top: f64,
    pub bottom: f64,
    pub baseline_y: f64,
}

// ── 辅助：从 EditorEditResult 提取 cause ─────────────────────────────────────

/// 构造合成 `EditorEditResult`——用于 `load_text` 等不经过 `kernel.apply()` 的路径。
///
/// 这些路径没有真正的 `EditorEditResult`，但 `record_transaction` 需要一个
/// `&EditorEditResult` 来构建 summary 和派生动画策略。合成结果的 `display_patches`
/// 为空（不产生动画），`cause` / `operation_kind` 由调用方指定。
pub(crate) fn synthetic_edit_result(
    old_text: &str,
    new_text: &str,
    old_cursor: usize,
    new_cursor: usize,
    cause: EditorTransactionCause,
    operation_kind: EditorOperationKind,
) -> EditorEditResult {
    let old_selection = EditorSelection {
        anchor: EditorCursor::new(old_text, old_cursor),
        head: EditorCursor::new(old_text, old_cursor),
    };
    let new_selection = EditorSelection {
        anchor: EditorCursor::new(new_text, new_cursor),
        head: EditorCursor::new(new_text, new_cursor),
    };
    let offset_map = if old_text != new_text {
        Some(OffsetMap::build(old_text, new_text))
    } else {
        None
    };
    EditorEditResult {
        transaction_id: 0,
        base_revision: EditorRevision::new(0),
        new_revision: EditorRevision::new(1),
        display_patches: Vec::new(),
        old_selection,
        new_selection,
        cause,
        operation_kind,
        offset_map,
        content_delta: writer_core::editor::EditorContentDelta::default(),
        changes: Vec::new(),
        before_text_sha256: String::new(),
        after_text_sha256: String::new(),
    }
}
