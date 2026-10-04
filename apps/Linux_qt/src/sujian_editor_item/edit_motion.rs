//! Issue #735: Linux 私有编辑运动类型。
//!
//! Core 已删除 `CursorRect`、`EditorAnimationKind`、`EditorVisualTransaction`、
//! `PreeditVisualTransaction`、`VisualCoordinateMode`、`EditorEngine`、
//! `diff_plain_text` 等视觉类型和 FFI 契约。本模块定义 Linux_qt 自己的替代类型，
//! 不再从 Core 导入任何视觉 DTO。
//!
//! 设计原则：
//! - `PreparedEditMotion` 由 pipeline 从 `EditorEditResult` + old/new `EditorSnapshot`
//!   直接构造，不经过 `EditorEngine`。
//! - `CursorRect` 是纯几何值类型，不携带 Core 语义。
//! - `diff_plain_text` 是本地文本 diff 实现，供 composition update 路径使用。

use writer_core::editor::{
    EditorChange, EditorCursor, EditorEditResult, EditorOperationKind, EditorRevision,
    EditorSelection, EditorTransactionCause, OffsetMap, Utf8ByteOffset, Utf8ByteRange,
};

// ── CompositionSession ──────────────────────────────────────────────────────

/// Issue #735: Linux 私有组合会话 — 替代已变为 `pub(crate)` 的 Core `CompositionSessionState`。
///
/// 存储平台端所需的组合会话数据：replace range、base text、preedit text。
///
/// Issue #826: preedit 是独立临时显示层。这个 session 只负责维护虚拟文本
/// （base text 的 replace range 被 preedit 替换后的结果）和 preedit 的 byte
/// range，不参与正文动画，也不进遮罩前沿。
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
/// 坐标空间：文档逻辑坐标系（不含滚动偏移），与 `AnimatedSlice` / `StaticPatch`
/// 一致。scene graph 渲染时按当前 `scroll_y` 做 viewport transform。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CursorRect {
    pub x: f64,
    pub top: f64,
    pub bottom: f64,
    pub baseline_y: f64,
}

// ── EditorAnimationKind ─────────────────────────────────────────────────────

/// Linux 私有动画类别 — 只从 `EditorEditResult.display_patches` 的
/// inserted / deleted 事实派生。
///
/// Issue #824 评论 5971089641 第 2 节：删除
/// “`EditorOperationKind` → 正文动画种类”这条主判断。CompositionCommit、
/// 普通 Insert、IME replace、粘贴最终都不能因为 `operation_kind` 不同而走出
/// 不同的正文动画结果：只要 patch 事实一样，正文 Reveal/Conceal 就一样。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorAnimationKind {
    /// inserted 非空、deleted 为空。
    Insert,
    /// inserted 为空、deleted 非空。
    Delete,
    /// 两边都有（Replace / IME 选区替换 / 粘贴覆盖选区）。
    Replace,
    /// 两边都空 —— 没有正文变化，只有光标移动。
    CursorOnly,
}

impl EditorAnimationKind {
    /// 从 patch 事实派生动画类别。
    ///
    /// - inserted 非空、deleted 为空：Insert
    /// - inserted 为空、deleted 非空：Delete
    /// - 两边都有：Replace
    /// - 两边都空：CursorOnly
    pub fn from_patch_facts(has_inserted: bool, has_deleted: bool) -> Self {
        match (has_inserted, has_deleted) {
            (true, false) => EditorAnimationKind::Insert,
            (false, true) => EditorAnimationKind::Delete,
            (true, true) => EditorAnimationKind::Replace,
            (false, false) => EditorAnimationKind::CursorOnly,
        }
    }
}

// ── PreparedEditMotion ──────────────────────────────────────────────────────

/// Linux 私有编辑运动 — 替代已删除的 Core `EditorVisualTransaction`。
///
/// 由 `pipeline::prepare_edit_motion()` 从 `EditorEditResult` + old/new `EditorSnapshot`
/// 直接构造，不经过 `EditorEngine`。`animation_coordinator::process_transaction`
/// 消费此结构创建 `PreparedTextVisualTransaction`。
///
/// 字段语义与旧 `EditorVisualTransaction` 对应字段一致：
/// - `kind`：动画类别（Insert / Delete / Replace / CursorOnly），只从
///   `display_patches` 的 inserted/deleted 事实派生，不再看 `operation_kind`
/// - `inserted_ranges`：新文本坐标系中的全部插入范围，从 `display_patches` 派生
/// - `deleted_ranges`：旧文本坐标系中的全部删除范围，从 `display_patches` 派生
/// - `inserted_range` / `deleted_range`：首个非零范围，供只需要单段连续编辑的
///   调用方（段落扩展）使用
/// - `old_text` / `new_text`：编辑前后的纯文本快照
///   由 pipeline 从 Qt 设置传入（Issue #756 评论 5821042551 拆分独立 duration）
/// - `old_selection` / `new_selection`：编辑前后的选区，从 `EditorEditResult` 直接取
/// - `old_cursor_rect` / `new_cursor_rect`：由 pipeline 从布局快照计算后填入
#[derive(Clone, Debug)]
pub(crate) struct PreparedEditMotion {
    pub kind: EditorAnimationKind,
    pub inserted_ranges: Vec<(usize, usize)>,
    pub deleted_ranges: Vec<(usize, usize)>,
    pub inserted_range: Option<Utf8ByteRange>,
    pub deleted_range: Option<Utf8ByteRange>,
    pub old_text: String,
    pub new_text: String,
    pub old_selection: EditorSelection,
    pub new_selection: EditorSelection,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
}

impl PreparedEditMotion {
    /// 从 `EditorEditResult` + old/new 文本快照构造 `PreparedEditMotion`。
    ///
    /// `old_cursor_rect` / `new_cursor_rect` 初始为 `None`，由 pipeline 在布局
    /// 计算后直接赋值到返回的结构体字段。
    pub fn from_edit_result(result: &EditorEditResult, old_text: &str, new_text: &str) -> Self {
        let (inserted_ranges, deleted_ranges) = ranges_from_display_patches(result);
        // Issue #824 评论 5971089641 第 2 节：动画类别只认 patch 事实，
        // 不再从 operation_kind 派生（CompositionCommit 也不再有专属分类）。
        let kind = EditorAnimationKind::from_patch_facts(
            !inserted_ranges.is_empty(),
            !deleted_ranges.is_empty(),
        );
        let inserted_range = inserted_ranges
            .first()
            .map(|&(start, end)| Utf8ByteRange::from_ordered(start, end));
        let deleted_range = deleted_ranges
            .first()
            .map(|&(start, end)| Utf8ByteRange::from_ordered(start, end));
        Self {
            kind,
            inserted_ranges,
            deleted_ranges,
            inserted_range,
            deleted_range,
            old_text: old_text.to_string(),
            new_text: new_text.to_string(),
            old_selection: result.old_selection.clone(),
            new_selection: result.new_selection.clone(),
            old_cursor_rect: None,
            new_cursor_rect: None,
        }
    }
}

/// 从 `EditorEditResult.display_patches` 派生全部 inserted/deleted ranges。
///
/// `DisplayPatch.replace_byte_range` 是旧文本坐标系中被替换的范围，
/// `DisplayPatch.inserted_text` 是插入的新文本。
///
/// - deleted range = `replace_byte_range`（旧文本坐标系）
/// - inserted range = `(replace_byte_range.start, replace_byte_range.start + inserted_text.len())`
///   （新文本坐标系）
///
/// Issue #824 评论 5971089641 第 2 节：本函数是正文动画分类的**唯一事实源**。
/// 普通 Insert/Delete/Replace、IME commit、粘贴都从这里派生 inserted/deleted，
/// 不再各自按 `operation_kind` 决定正文动画种类。
///
/// 这是 `pub(crate)` 而不是私有：`record_composition_commit_transaction`
/// 必须用同一份 patch 事实构造 commit 的正文 Reveal/Conceal。
pub(crate) fn ranges_from_display_patches(
    result: &EditorEditResult,
) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let mut inserted = Vec::new();
    let mut deleted = Vec::new();
    for patch in &result.display_patches {
        let replace_start = patch.replace_byte_range.start().value();
        let replace_end = patch.replace_byte_range.end().value();
        let inserted_len = patch.inserted_text.len();
        if replace_end > replace_start {
            deleted.push((replace_start, replace_end));
        }
        if inserted_len > 0 {
            inserted.push((replace_start, replace_start + inserted_len));
        }
    }
    (inserted, deleted)
}

// ── diff_plain_text ─────────────────────────────────────────────────────────

/// 本地文本 diff — 替代已删除的 Core `writer_core::editor::diff_plain_text`。
///
/// 找出 old_text → new_text 的最小编辑序列（Insert / Delete），按 byte offset 返回。
/// 算法：公共前缀 + 公共后缀，中间段为一次 Replace（拆成 Delete + Insert）。
///
/// 供 `animation_coordinator::handle_composition_update` 使用——composition update
/// 不经过 `EditorEditResult`，需要独立 diff 找 inserted/deleted range。
pub(crate) fn diff_plain_text(old_text: &str, new_text: &str) -> Vec<EditorChange> {
    let old_bytes = old_text.as_bytes();
    let new_bytes = new_text.as_bytes();
    let old_len = old_bytes.len();
    let new_len = new_bytes.len();

    // 找公共前缀（按 byte，但停在 char boundary 上）。
    let mut prefix = 0usize;
    while prefix < old_len
        && prefix < new_len
        && old_bytes[prefix] == new_bytes[prefix]
        && old_text.is_char_boundary(prefix + 1)
        && new_text.is_char_boundary(prefix + 1)
    {
        prefix += 1;
    }
    // 回退到 char boundary（上面的循环条件保证 prefix 是 char boundary）。

    // 找公共后缀。
    let mut suffix = 0usize;
    while prefix + suffix < old_len
        && prefix + suffix < new_len
        && old_bytes[old_len - 1 - suffix] == new_bytes[new_len - 1 - suffix]
        && old_text.is_char_boundary(old_len - suffix - 1)
        && new_text.is_char_boundary(new_len - suffix - 1)
    {
        suffix += 1;
    }

    let mut changes = Vec::new();

    // Delete 段：old_text[prefix..old_len-suffix)
    if prefix + suffix < old_len {
        let deleted_text = &old_text[prefix..old_len - suffix];
        if !deleted_text.is_empty() {
            changes.push(EditorChange::Delete {
                index: Utf8ByteOffset::unchecked(prefix),
                text: deleted_text.to_string(),
            });
        }
    }

    // Insert 段：new_text[prefix..new_len-suffix)
    if prefix + suffix < new_len {
        let inserted_text = &new_text[prefix..new_len - suffix];
        if !inserted_text.is_empty() {
            changes.push(EditorChange::Insert {
                index: Utf8ByteOffset::unchecked(prefix),
                text: inserted_text.to_string(),
            });
        }
    }

    changes
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
    }
}
