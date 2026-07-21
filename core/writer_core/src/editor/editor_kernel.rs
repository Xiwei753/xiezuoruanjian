//! Rust EditorKernel — 正文和业务唯一真相。
//!
//! EditorKernel 是 Editor V2 的核心入口，负责：
//! - 正文、章节 revision、逻辑选区和编辑事务
//! - insert / delete / replace / composition commit 等标准编辑命令
//! - undo / redo、自动缩进、查找替换、统计、保存、同步所需业务语义
//! - 编辑前后差异、受影响 UTF-8 byte range、事务原因
//! - 动画是否启用、动画语义、模式和时长等产品规则
//! - 返回可增量应用的 EditResult 与 VisualIntent
//!
//! EditorKernel 不负责：
//! - 字体、glyph、行号、像素坐标、Bitmap/QImage、纹理和 Canvas/QSG 节点
//! - 平台 UTF-16 布局算法
//! - preedit 的像素表现
//! - 动画每一帧的推进和图形资源所有权

use serde::{Deserialize, Serialize};

use super::transaction::{
    choose_animation_mode, count_grapheme_clusters,
    diff_plain_text, text_contains_complex_grapheme,
    AnimationMode, CompositionUpdateTransaction,
    CompositionCommitOrCancelTransaction, CompositionVisualRevision,
    EditorChange, EditorTransactionCause,
};

/// 编辑命令 — 平台输入适配器翻译系统事件后的标准化命令。
///
/// 所有平台输入（IME、键盘、触摸选择）都翻译成 EditorCommand，
/// 交给 EditorKernel.apply() 处理。平台不能再维护第二份可独立编辑的正文真相。
///
/// 偏移量单位统一为 UTF-8 byte boundary（半开区间 `[start, end_exclusive)`）。
/// Android/Windows 的 UTF-16、Qt 的 QChar index 只允许存在于平台 TextIndexMap 内，
/// 传入 Core 前必须通过 `utf16ToUtf8` / `utf16_code_unit_to_utf8_byte` 转换。
///
/// `expected_revision` 用于乐观并发控制：若当前正文 revision 与 expected 不匹配，
/// apply() 返回 EditResult::conflict()，平台端需刷新后重试。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum EditorCommand {
    Insert {
        byte_offset: usize,
        text: String,
        cause: EditorTransactionCause,
        expected_revision: u64,
    },
    Delete {
        byte_start: usize,
        byte_end_exclusive: usize,
        deleted_text: String,
        cause: EditorTransactionCause,
        expected_revision: u64,
    },
    Replace {
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement_text: String,
        original_text: String,
        cause: EditorTransactionCause,
        expected_revision: u64,
    },
    SetSelection {
        anchor_byte_offset: usize,
        head_byte_offset: usize,
        expected_revision: u64,
    },
    Undo { expected_revision: u64 },
    Redo { expected_revision: u64 },
    ReplaceAll {
        search: String,
        replacement: String,
        expected_revision: u64,
    },
    InsertLineBreak {
        byte_offset: usize,
        auto_indent_prefix: String,
        cause: EditorTransactionCause,
        expected_revision: u64,
    },
    CommitText {
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement_text: String,
        resulting_selection_anchor: usize,
        resulting_selection_head: usize,
        composition_session_id: u64,
        composition_base_revision: u64,
        composition_generation: u64,
        cause: EditorTransactionCause,
        expected_revision: u64,
    },
    DeleteSurrounding {
        before_byte_start: usize,
        before_byte_end_exclusive: usize,
        after_byte_start: usize,
        after_byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        expected_revision: u64,
    },
    BeginComposition {
        replace_start: usize,
        replace_end_exclusive: usize,
        expected_revision: u64,
    },
    UpdateComposition {
        composition_session_id: u64,
        composition_generation: u64,
        new_preedit_text: String,
        new_preedit_cursor_offset: usize,
        expected_revision: u64,
    },
    FinishComposition {
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    },
    CancelComposition {
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    },
}

/// 显示补丁 — 平台显示镜像唯一允许消费的正文变化。
///
/// DisplayTextMirror 按 DisplayPatch 增量更新 SpannableStringBuilder，
/// 不得根据 old/new 全文重新 diff，也不得先本地改 Buffer 再通知 Core。
///
/// `replace_byte_range` 为半开区间 `[start, end)`（UTF-8 byte offset），
/// 表示要被替换的范围。`inserted_text` 为替换后的新文本。
/// `resulting_selection_byte_range` 为替换完成后的选区（半开区间）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayPatch {
    /// 基础 revision ID
    pub base_revision: u64,
    /// 新 revision ID
    pub new_revision: u64,
    /// 替换范围（UTF-8 byte offset）
    pub replace_byte_range: (usize, usize),
    /// 插入的文本
    pub inserted_text: String,
    /// 替换后的选区（UTF-8 byte offset）
    pub resulting_selection_byte_range: (usize, usize),
}

/// 视觉意图 — Core 告诉平台层应该做什么动画。
///
/// Rust 决定动画模式和时长（产品规则）；
/// glyph、cluster、line、rect、stable suffix 和 snapshot 属于平台排版事实，
/// 由平台 Planner 决定。
///
/// VisualIntent 不包含平台渲染结构（QImage / RenderNode / Bitmap 等）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorVisualIntent {
    /// 触发原因
    pub cause: EditorTransactionCause,
    /// 操作类型
    pub operation_kind: EditorOperationKind,
    /// 旧文本中受影响的 UTF-8 byte range 列表
    pub old_affected_byte_ranges: Vec<(usize, usize)>,
    /// 新文本中受影响的 UTF-8 byte range 列表
    pub new_affected_byte_ranges: Vec<(usize, usize)>,
    /// 动画模式
    pub animation_mode: AnimationMode,
    /// 动画时长（毫秒）
    pub duration_ms: u64,
    /// 光标协同信息
    pub coordinated_cursor: CoordinatedCursor,
}

/// 操作类型 — 区分不同编辑操作的视觉语义
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorOperationKind {
    Insert,
    Delete,
    Replace,
    CursorOnly,
    CompositionUpdate,
    CompositionCommit,
    CompositionCancel,
    Load,
    Format,
}

/// 光标协同 — 描述光标移动的视觉语义
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatedCursor {
    /// 旧光标位置（UTF-8 byte offset）
    pub old_byte_offset: usize,
    /// 新光标位置（UTF-8 byte offset）
    pub new_byte_offset: usize,
    /// 是否需要动画
    pub should_animate: bool,
}

/// 编辑结果分类 — 平台必须区分不同结果走不同恢复路径。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorEditOutcome {
    /// 编辑成功应用，正文已变更
    Applied(EditorEditResult),
    /// 编辑成功应用，但平台传入的选区 offset 不在 char boundary 上，内核已自动对齐
    AppliedWithAdjustedSelection(EditorEditResult),
    /// 命令无实际效果（如空替换、空选区变更），正文未变
    NoChange(EditorEditResult),
    /// expected_revision 与当前 revision 不匹配，平台需用结果中的最新 revision 重试
    StaleRevision(EditorEditResult),
    /// offset 不在 UTF-8 char boundary 上或超出文本范围
    InvalidOffset(EditorEditResult),
    /// range 语义非法（如 start ≥ end 对于 delete）
    InvalidRange(EditorEditResult),
}

impl EditorEditOutcome {
    pub fn into_result(self) -> EditorEditResult {
        match self {
            EditorEditOutcome::Applied(r)
            | EditorEditOutcome::AppliedWithAdjustedSelection(r)
            | EditorEditOutcome::NoChange(r)
            | EditorEditOutcome::StaleRevision(r)
            | EditorEditOutcome::InvalidOffset(r)
            | EditorEditOutcome::InvalidRange(r) => r,
        }
    }

    pub fn is_applied(&self) -> bool {
        matches!(
            self,
            EditorEditOutcome::Applied(_) | EditorEditOutcome::AppliedWithAdjustedSelection(_)
        )
    }

    pub fn is_stale(&self) -> bool {
        matches!(self, EditorEditOutcome::StaleRevision(_))
    }

    pub fn is_invalid(&self) -> bool {
        matches!(
            self,
            EditorEditOutcome::InvalidOffset(_) | EditorEditOutcome::InvalidRange(_)
        )
    }
}

/// 编辑结果 — EditorKernel.apply() 的返回值。
///
/// 包含正文变化（display_patches）、选区变化和视觉意图。
/// 平台端按此结果增量更新显示镜像、布局和动画。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorEditResult {
    /// 事务 ID
    pub transaction_id: u64,
    /// 基础 revision ID
    pub base_revision: u64,
    /// 新 revision ID
    pub new_revision: u64,
    /// 显示补丁列表
    pub display_patches: Vec<DisplayPatch>,
    /// 旧选区（UTF-8 byte offset）
    pub old_selection_byte_range: (usize, usize),
    /// 新选区（UTF-8 byte offset）
    pub new_selection_byte_range: (usize, usize),
    /// 视觉意图
    pub visual_intent: EditorVisualIntent,
}

/// 编辑器输入校验错误
#[derive(Debug, Clone)]
pub enum EditorInputError {
    /// 光标 offset 超出文本长度或不在 UTF-8 char boundary 上
    InvalidCursorOffset { offset: usize, text_len: usize },
}

impl std::fmt::Display for EditorInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCursorOffset { offset, text_len } => {
                write!(f, "cursor offset {} is not a valid UTF-8 char boundary (text len {})", offset, text_len)
            }
        }
    }
}

impl std::error::Error for EditorInputError {}

/// EditorKernel — 正文和业务唯一真相。
///
/// 平台不能再维护第二份可独立编辑的正文真相。
/// 平台只持有与 Rust revision 对应的显示镜像。
///
/// 选区模型：selection_anchor 是选区锚点（非移动端），
/// cursor 是选区光标（移动端/插入点）。当无选区时两者相等。
/// 所有 offset 均为 UTF-8 byte offset。
#[derive(Debug, Clone)]
pub struct EditorKernel {
    text: String,
    revision: u64,
    cursor: usize,
    selection_anchor: usize,
    next_transaction_id: u64,
    animation_duration_ms: u64,
    max_animated_chars: usize,
    animation_enabled: bool,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    composition_session: Option<CompositionSessionState>,
    next_composition_session_id: u64,
}

/// Composition 会话状态 — 跟踪一次 IME composition 的生命周期。
///
/// 从 BeginComposition 到 FinishComposition/CancelComposition 之间有效。
/// `generation` 在 TextEditSession reset 时递增，使过期的 composition 操作被内核拒绝。
///
/// `preedit_cursor_utf16` 使用 UTF-16 code unit 单位，因为 IME 协议
/// （Android InputConnection、Qt QInputMethodEvent）以 UTF-16 报告光标位置。
/// 内核其余字段统一使用 UTF-8 byte offset。
#[derive(Debug, Clone)]
struct CompositionSessionState {
    session_id: u64,
    base_revision: u64,
    generation: u64,
    replace_start: usize,
    replace_end_exclusive: usize,
    preedit_text: String,
    preedit_cursor_utf16: usize,
}

/// Undo 快照 — 存储编辑前后的全文快照与光标位置。
///
/// 每次编辑操作 push 一条 UndoEntry 到 undo_stack 并清空 redo_stack，
/// 保证 undo/redo 栈的互斥性：新编辑使 redo 历史作废。
#[derive(Debug, Clone)]
struct UndoEntry {
    old_text: String,
    new_text: String,
    old_cursor: usize,
    new_cursor: usize,
}

impl Default for EditorKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorKernel {
    /// 创建空编辑器。默认 animation_duration_ms=160, max_animated_chars=8, animation_enabled=true。
    pub fn new() -> Self {
        Self {
            text: String::new(),
            revision: 0,
            cursor: 0,
            selection_anchor: 0,
            next_transaction_id: 1,
            animation_duration_ms: 160,
            max_animated_chars: 8,
            animation_enabled: true,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            composition_session: None,
            next_composition_session_id: 1,
        }
    }

    /// 用初始文本和光标创建编辑器。`cursor` 必须是合法 UTF-8 char boundary 且 ≤ text.len()，
    /// 否则返回 `EditorInputError::InvalidCursorOffset`。
    pub fn with_text(text: String, cursor: usize) -> Result<Self, EditorInputError> {
        if cursor > text.len() || !text.is_char_boundary(cursor) {
            return Err(EditorInputError::InvalidCursorOffset {
                offset: cursor,
                text_len: text.len(),
            });
        }
        Ok(Self {
            text,
            revision: 0,
            cursor,
            selection_anchor: cursor,
            next_transaction_id: 1,
            animation_duration_ms: 160,
            max_animated_chars: 8,
            animation_enabled: true,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            composition_session: None,
            next_composition_session_id: 1,
        })
    }

    /// 将 offset 对齐到最近的合法 UTF-8 char boundary。
    ///
    /// 超出文本长度时返回文本长度（末尾始终是合法 boundary）。
    /// 落在多字节序列中间时向前回退到该字符的起始位置。
    fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
        if offset > text.len() {
            return text.len();
        }
        if text.is_char_boundary(offset) {
            return offset;
        }
        let mut clamped = offset;
        while clamped > 0 && !text.is_char_boundary(clamped) {
            clamped -= 1;
        }
        clamped
    }

    /// 设置动画时长（毫秒）。仅影响后续事务，不改变已生成的事务。
    pub fn set_animation_duration_ms(&mut self, duration_ms: u64) {
        self.animation_duration_ms = duration_ms;
    }

    /// 设置动画总开关。false 时所有编辑返回 `AnimationMode::SystemSuppressed`。
    pub fn set_animation_enabled(&mut self, enabled: bool) {
        self.animation_enabled = enabled;
    }

    /// 当前正文（UTF-8 纯文本）。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 修订号——每次成功编辑单调递增，用于乐观并发校验（expected_revision）。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 主光标位置（UTF-8 byte offset，保证 char boundary）。
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// 选区固定端（UTF-8 byte offset）。与 cursor 不同时表示有选中范围。
    pub fn selection_anchor(&self) -> usize {
        self.selection_anchor
    }

    /// 完整选区 (anchor, cursor)。anchor ≤ cursor 不保证——取决于选择方向。
    pub fn selection(&self) -> (usize, usize) {
        (self.selection_anchor, self.cursor)
    }

    /// 应用编辑命令 — 唯一正文修改入口。
    ///
    /// 平台输入适配器只调用此方法，不能直接修改正文。
    /// 返回 EditorEditOutcome，区分 Applied/NoChange/StaleRevision/InvalidOffset/InvalidRange。
    pub fn apply(&mut self, command: EditorCommand) -> EditorEditOutcome {
        let base_revision = self.revision;

        match &command {
            EditorCommand::Insert { expected_revision, .. }
            | EditorCommand::Delete { expected_revision, .. }
            | EditorCommand::Replace { expected_revision, .. }
            | EditorCommand::SetSelection { expected_revision, .. }
            | EditorCommand::ReplaceAll { expected_revision, .. }
            | EditorCommand::InsertLineBreak { expected_revision, .. }
            | EditorCommand::Undo { expected_revision }
            | EditorCommand::Redo { expected_revision }
            | EditorCommand::CommitText { expected_revision, .. }
            | EditorCommand::DeleteSurrounding { expected_revision, .. }
            | EditorCommand::BeginComposition { expected_revision, .. }
            | EditorCommand::UpdateComposition { expected_revision, .. }
            | EditorCommand::FinishComposition { expected_revision, .. }
            | EditorCommand::CancelComposition { expected_revision, .. } => {
                if *expected_revision != base_revision {
                    return EditorEditOutcome::StaleRevision(self.stale_session_result());
                }
            }
        }

        let old_cursor = self.cursor;
        let old_selection = (self.selection_anchor, self.cursor);

        match command {
            EditorCommand::Insert { byte_offset, text, cause, .. } => {
                self.apply_insert(byte_offset, &text, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::Delete { byte_start, byte_end_exclusive, deleted_text: _, cause, .. } => {
                self.apply_delete(byte_start, byte_end_exclusive, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::Replace { byte_start, byte_end_exclusive, replacement_text, original_text: _, cause, .. } => {
                self.apply_replace(byte_start, byte_end_exclusive, &replacement_text, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::SetSelection { anchor_byte_offset, head_byte_offset, .. } => {
                self.apply_set_selection(anchor_byte_offset, head_byte_offset, base_revision, old_cursor, old_selection)
            }
            EditorCommand::Undo { .. } => {
                self.apply_undo(base_revision, old_cursor, old_selection)
            }
            EditorCommand::Redo { .. } => {
                self.apply_redo(base_revision, old_cursor, old_selection)
            }
            EditorCommand::ReplaceAll { search, replacement, .. } => {
                self.apply_replace_all(&search, &replacement, base_revision, old_cursor, old_selection)
            }
            EditorCommand::InsertLineBreak { byte_offset, auto_indent_prefix, cause, .. } => {
                self.apply_insert_line_break(byte_offset, auto_indent_prefix, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::CommitText {
                byte_start,
                byte_end_exclusive,
                replacement_text,
                resulting_selection_anchor,
                resulting_selection_head,
                composition_session_id,
                composition_base_revision,
                composition_generation,
                cause,
                ..
            } => {
                self.apply_commit_text(
                    byte_start,
                    byte_end_exclusive,
                    &replacement_text,
                    resulting_selection_anchor,
                    resulting_selection_head,
                    composition_session_id,
                    composition_base_revision,
                    composition_generation,
                    cause,
                    base_revision,
                    old_cursor,
                    old_selection,
                )
            }
            EditorCommand::DeleteSurrounding {
                before_byte_start,
                before_byte_end_exclusive,
                after_byte_start,
                after_byte_end_exclusive,
                cause,
                ..
            } => {
                self.apply_delete_surrounding(
                    before_byte_start,
                    before_byte_end_exclusive,
                    after_byte_start,
                    after_byte_end_exclusive,
                    cause,
                    base_revision,
                    old_cursor,
                    old_selection,
                )
            }
            EditorCommand::BeginComposition {
                replace_start,
                replace_end_exclusive,
                ..
            } => {
                self.apply_begin_composition(replace_start, replace_end_exclusive, base_revision, old_cursor, old_selection)
            }
            EditorCommand::UpdateComposition {
                composition_session_id,
                composition_generation,
                new_preedit_text,
                new_preedit_cursor_offset,
                ..
            } => {
                self.apply_update_composition(
                    composition_session_id,
                    composition_generation,
                    &new_preedit_text,
                    new_preedit_cursor_offset,
                    base_revision,
                    old_cursor,
                    old_selection,
                )
            }
            EditorCommand::FinishComposition {
                composition_session_id,
                composition_generation,
                ..
            } => {
                self.apply_finish_composition(composition_session_id, composition_generation, base_revision, old_cursor, old_selection)
            }
            EditorCommand::CancelComposition {
                composition_session_id,
                composition_generation,
                ..
            } => {
                self.apply_cancel_composition(composition_session_id, composition_generation, base_revision, old_cursor, old_selection)
            }
        }
    }

    fn apply_insert(
        &mut self,
        byte_offset: usize,
        text: &str,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        if byte_offset > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(byte_offset) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        self.composition_session = None;

        self.text.insert_str(byte_offset, text);
        self.revision = self.revision.saturating_add(1);
        self.cursor = byte_offset + text.len();
        self.selection_anchor = self.cursor;

        self.undo_stack.push(UndoEntry {
            old_text: self.text[..byte_offset].to_string() + &self.text[byte_offset + text.len()..],
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = (self.cursor, self.cursor);
        let new_affected = vec![(byte_offset, byte_offset + text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (byte_offset, byte_offset),
            inserted_text: text.to_string(),
            resulting_selection_byte_range: new_selection,
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let cluster_count = count_grapheme_clusters(text);
            let contains_newline = text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                is_loading,
                is_format,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Insert,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: self.animation_enabled && old_cursor != self.cursor && !is_loading && !is_format,
            },
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }

    fn apply_delete(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let (byte_start, byte_end_exclusive) = Self::normalize_range(byte_start, byte_end_exclusive);
        if byte_start > self.text.len() || byte_end_exclusive > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(byte_start) || !self.text.is_char_boundary(byte_end_exclusive) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if byte_start >= byte_end_exclusive {
            return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let old_text = self.text.clone();

        self.composition_session = None;

        self.text.replace_range(byte_start..byte_end_exclusive, "");
        self.revision = self.revision.saturating_add(1);
        self.cursor = byte_start;
        self.selection_anchor = self.cursor;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = (self.cursor, self.cursor);
        let old_affected = vec![(byte_start, byte_end_exclusive)];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (byte_start, byte_end_exclusive),
            inserted_text: String::new(),
            resulting_selection_byte_range: new_selection,
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let deleted_text = &old_text[byte_start..byte_end_exclusive];
            let cluster_count = count_grapheme_clusters(deleted_text);
            let contains_newline = deleted_text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(deleted_text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                is_loading,
                is_format,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Delete,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: vec![],
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: self.animation_enabled && old_cursor != self.cursor && !is_loading && !is_format,
            },
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_replace(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement_text: &str,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let (byte_start, byte_end_exclusive) = Self::normalize_range(byte_start, byte_end_exclusive);
        if byte_start > self.text.len() || byte_end_exclusive > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(byte_start) || !self.text.is_char_boundary(byte_end_exclusive) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let old_text = self.text.clone();

        self.composition_session = None;

        self.text.replace_range(byte_start..byte_end_exclusive, replacement_text);
        self.revision = self.revision.saturating_add(1);
        self.cursor = byte_start + replacement_text.len();
        self.selection_anchor = self.cursor;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = (self.cursor, self.cursor);
        let old_affected = vec![(byte_start, byte_end_exclusive)];
        let new_affected = vec![(byte_start, byte_start + replacement_text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (byte_start, byte_end_exclusive),
            inserted_text: replacement_text.to_string(),
            resulting_selection_byte_range: new_selection,
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let diff_text = if !replacement_text.is_empty() {
                replacement_text
            } else {
                &old_text[byte_start..byte_end_exclusive]
            };
            let cluster_count = count_grapheme_clusters(diff_text);
            let contains_newline = diff_text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(diff_text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                is_loading,
                is_format,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Replace,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: self.animation_enabled && old_cursor != self.cursor && !is_loading && !is_format,
            },
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }

    fn apply_set_selection(
        &mut self,
        anchor_byte_offset: usize,
        head_byte_offset: usize,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let anchor = anchor_byte_offset;
        let head = head_byte_offset;
        if anchor > self.text.len() || head > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(anchor) || !self.text.is_char_boundary(head) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        self.selection_anchor = anchor;
        self.cursor = head;

        let new_selection = (self.selection_anchor, self.cursor);

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Programmatic,
            operation_kind: EditorOperationKind::CursorOnly,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: vec![],
            animation_mode: if self.animation_enabled && old_cursor != self.cursor {
                AnimationMode::GlyphAnimation
            } else {
                AnimationMode::SystemSuppressed
            },
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: self.animation_enabled && old_cursor != self.cursor,
            },
        };

        EditorEditOutcome::NoChange(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }

    fn apply_undo(
        &mut self,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let entry = match self.undo_stack.pop() {
            Some(e) => e,
            None => return EditorEditOutcome::NoChange(self.noop_result(base_revision, old_cursor, old_selection)),
        };

        let old_text = self.text.clone();
        self.text = entry.old_text.clone();
        self.cursor = entry.old_cursor;
        self.selection_anchor = self.cursor;
        self.revision = self.revision.saturating_add(1);
        self.composition_session = None;

        self.redo_stack.push(entry);

        let new_revision = self.revision;
        let new_selection = (self.cursor, self.cursor);

        let (replace_range, inserted_text) = Self::compute_single_patch(&old_text, &self.text);

        let display_patches = if replace_range.0 < replace_range.1 || !inserted_text.is_empty() {
            vec![DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: replace_range,
                inserted_text,
                resulting_selection_byte_range: new_selection,
            }]
        } else {
            vec![]
        };

        let changes = diff_plain_text(&old_text, &self.text);
        let (old_affected, new_affected) = Self::affected_ranges_from_changes(&changes);

        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            AnimationMode::SnapshotAnimation
        };

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Undo,
            operation_kind: EditorOperationKind::Replace,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: self.animation_enabled && old_cursor != self.cursor,
            },
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }

    fn apply_redo(
        &mut self,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let entry = match self.redo_stack.pop() {
            Some(e) => e,
            None => return EditorEditOutcome::NoChange(self.noop_result(base_revision, old_cursor, old_selection)),
        };

        let old_text = self.text.clone();
        self.text = entry.new_text.clone();
        self.cursor = entry.new_cursor;
        self.selection_anchor = self.cursor;
        self.revision = self.revision.saturating_add(1);
        self.composition_session = None;

        self.undo_stack.push(entry);

        let new_revision = self.revision;
        let new_selection = (self.cursor, self.cursor);

        let (replace_range, inserted_text) = Self::compute_single_patch(&old_text, &self.text);

        let display_patches = if replace_range.0 < replace_range.1 || !inserted_text.is_empty() {
            vec![DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: replace_range,
                inserted_text,
                resulting_selection_byte_range: new_selection,
            }]
        } else {
            vec![]
        };

        let changes = diff_plain_text(&old_text, &self.text);
        let (old_affected, new_affected) = Self::affected_ranges_from_changes(&changes);

        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            AnimationMode::SnapshotAnimation
        };

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Redo,
            operation_kind: EditorOperationKind::Replace,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: self.animation_enabled && old_cursor != self.cursor,
            },
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }

    /// 计算从 old_text 到 new_text 的最小单段替换补丁。
    ///
    /// 返回 `(replace_byte_range, inserted_text)`，其中 replace_byte_range
    /// 是半开区间 [start, end)，表示 old_text 中被替换的范围；
    /// inserted_text 是替换后的新文本。
    ///
    /// 算法：先找公共前缀和公共后缀，中间部分即为差异。
    /// 前缀/后缀长度会向 UTF-8 char boundary 对齐，保证返回的范围始终合法。
    fn compute_single_patch(old_text: &str, new_text: &str) -> ((usize, usize), String) {
        if old_text == new_text {
            return ((0, 0), String::new());
        }

        let mut prefix_len = 0;
        for (ob, nb) in old_text.bytes().zip(new_text.bytes()) {
            if ob != nb { break; }
            prefix_len += 1;
        }
        while prefix_len > 0 && !old_text.is_char_boundary(prefix_len) {
            prefix_len -= 1;
        }
        while prefix_len > 0 && !new_text.is_char_boundary(prefix_len) {
            prefix_len -= 1;
        }

        let mut old_suffix_len = 0;
        let mut new_suffix_len = 0;
        {
            let old_remaining = &old_text[prefix_len..];
            let new_remaining = &new_text[prefix_len..];
            let old_rev = old_remaining.bytes().rev();
            let new_rev = new_remaining.bytes().rev();
            for (ob, nb) in old_rev.zip(new_rev) {
                if ob != nb { break; }
                old_suffix_len += 1;
                new_suffix_len += 1;
            }
        }

        while old_suffix_len > 0 && !old_text.is_char_boundary(old_text.len() - old_suffix_len) {
            old_suffix_len -= 1;
        }
        while new_suffix_len > 0 && !new_text.is_char_boundary(new_text.len() - new_suffix_len) {
            new_suffix_len -= 1;
        }

        let old_remaining_after_prefix = old_text.len() - prefix_len;
        let new_remaining_after_prefix = new_text.len() - prefix_len;
        if old_suffix_len > old_remaining_after_prefix {
            old_suffix_len = old_remaining_after_prefix;
            while old_suffix_len > 0 && !old_text.is_char_boundary(old_text.len() - old_suffix_len) {
                old_suffix_len -= 1;
            }
        }
        if new_suffix_len > new_remaining_after_prefix {
            new_suffix_len = new_remaining_after_prefix;
            while new_suffix_len > 0 && !new_text.is_char_boundary(new_text.len() - new_suffix_len) {
                new_suffix_len -= 1;
            }
        }

        let replace_start = prefix_len;
        let replace_end = old_text.len() - old_suffix_len;
        let inserted_end = new_text.len() - new_suffix_len;

        if replace_start > replace_end && inserted_end <= prefix_len {
            return ((replace_start, replace_start), String::new());
        }

        let inserted_text = if prefix_len < inserted_end {
            new_text[prefix_len..inserted_end].to_string()
        } else {
            String::new()
        };

        ((replace_start, replace_end), inserted_text)
    }

    fn apply_replace_all(
        &mut self,
        search: &str,
        replacement: &str,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let old_text = self.text.clone();
        let new_text = old_text.replace(search, replacement);

        if new_text == old_text {
            return EditorEditOutcome::NoChange(self.noop_result(base_revision, old_cursor, old_selection));
        }

        self.text = new_text;
        self.revision = self.revision.saturating_add(1);
        self.cursor = Self::clamp_to_char_boundary(&self.text, self.cursor);
        self.selection_anchor = self.cursor;
        self.composition_session = None;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = (self.cursor, self.cursor);

        let (replace_range, inserted_text) = Self::compute_single_patch(&old_text, &self.text);

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: replace_range,
            inserted_text,
            resulting_selection_byte_range: new_selection,
        }];

        let changes = diff_plain_text(&old_text, &self.text);
        let (old_affected, new_affected) = Self::affected_ranges_from_changes(&changes);

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Format,
            operation_kind: EditorOperationKind::Format,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: false,
            },
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }

    fn apply_insert_line_break(
        &mut self,
        byte_offset: usize,
        auto_indent_prefix: String,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        if byte_offset > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(byte_offset) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        let text = format!("\n{}", auto_indent_prefix);

        self.composition_session = None;

        self.text.insert_str(byte_offset, &text);
        self.revision = self.revision.saturating_add(1);
        self.cursor = byte_offset + text.len();
        self.selection_anchor = self.cursor;

        self.undo_stack.push(UndoEntry {
            old_text: self.text[..byte_offset].to_string() + &self.text[byte_offset + text.len()..],
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = (self.cursor, self.cursor);
        let new_affected = vec![(byte_offset, byte_offset + text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (byte_offset, byte_offset),
            inserted_text: text.clone(),
            resulting_selection_byte_range: new_selection,
        }];

        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            let cluster_count = count_grapheme_clusters(&text);
            let contains_newline = text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(&text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                false,
                false,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Insert,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: self.animation_enabled && old_cursor != self.cursor,
            },
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }

    fn stale_session_result(
        &mut self,
    ) -> EditorEditResult {
        let current_selection = (self.selection_anchor, self.cursor);
        EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision: self.revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: current_selection,
            new_selection_byte_range: current_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::Programmatic,
                operation_kind: EditorOperationKind::CursorOnly,
                old_affected_byte_ranges: vec![],
                new_affected_byte_ranges: vec![],
                animation_mode: AnimationMode::SystemSuppressed,
                duration_ms: 0,
                coordinated_cursor: CoordinatedCursor {
                    old_byte_offset: self.cursor,
                    new_byte_offset: self.cursor,
                    should_animate: false,
                },
            },
        }
    }

    fn noop_result(
        &mut self,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditResult {
        EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: old_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::Programmatic,
                operation_kind: EditorOperationKind::CursorOnly,
                old_affected_byte_ranges: vec![],
                new_affected_byte_ranges: vec![],
                animation_mode: AnimationMode::SystemSuppressed,
                duration_ms: 0,
                coordinated_cursor: CoordinatedCursor {
                    old_byte_offset: old_cursor,
                    new_byte_offset: old_cursor,
                    should_animate: false,
                },
            },
        }
    }

    #[allow(clippy::type_complexity)]
    fn affected_ranges_from_changes(changes: &[EditorChange]) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
        let mut old_ranges = Vec::new();
        let mut new_ranges = Vec::new();
        for c in changes {
            match c {
                EditorChange::Delete { index, text } => {
                    old_ranges.push((*index, *index + text.len()));
                }
                EditorChange::Insert { index, text } => {
                    new_ranges.push((*index, *index + text.len()));
                }
            }
        }
        (old_ranges, new_ranges)
    }

    fn take_transaction_id(&mut self) -> u64 {
        let id = self.next_transaction_id;
        self.next_transaction_id = self.next_transaction_id.saturating_add(1);
        id
    }

    /// 确保 start ≤ end，使范围始终满足半开区间 [start, end) 语义。
    fn normalize_range(start: usize, end: usize) -> (usize, usize) {
        if start > end {
            (end, start)
        } else {
            (start, end)
        }
    }

    /// 加载文本（章节打开时调用）
    ///
    /// 始终生成完整 replacement patch，即使正文相同。
    /// 平台 Mirror 通过 loadFromSessionSnapshot 重建，不依赖增量 patch。
    pub fn load_text(&mut self, text: String, cursor: usize) -> EditorEditOutcome {
        let base_revision = self.revision;
        let old_cursor = self.cursor;
        let old_selection = (self.selection_anchor, self.cursor);

        let needs_clamp = cursor > text.len() || !text.is_char_boundary(cursor);
        let resolved_cursor = if needs_clamp {
            Self::clamp_to_char_boundary(&text, cursor)
        } else {
            cursor
        };

        let old_text = self.text.clone();
        self.text = text;
        self.cursor = resolved_cursor;
        self.selection_anchor = resolved_cursor;
        self.revision = self.revision.saturating_add(1);
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.composition_session = None;

        let new_selection = (self.cursor, self.cursor);
        let new_revision = self.revision;

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (0, old_text.len()),
            inserted_text: self.text.clone(),
            resulting_selection_byte_range: new_selection,
        }];

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Load,
            operation_kind: EditorOperationKind::Load,
            old_affected_byte_ranges: if old_text.is_empty() { vec![] } else { vec![(0, old_text.len())] },
            new_affected_byte_ranges: if self.text.is_empty() { vec![] } else { vec![(0, self.text.len())] },
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: false,
            },
        };

        let result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        };

        if needs_clamp {
            EditorEditOutcome::AppliedWithAdjustedSelection(result)
        } else {
            EditorEditOutcome::Applied(result)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_commit_text(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement_text: &str,
        resulting_selection_anchor: usize,
        resulting_selection_head: usize,
        composition_session_id: u64,
        composition_base_revision: u64,
        composition_generation: u64,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        if let Some(ref session) = self.composition_session {
            if session.session_id != composition_session_id
                || session.base_revision != composition_base_revision
                || session.generation != composition_generation
            {
                return EditorEditOutcome::StaleRevision(self.stale_session_result());
            }
        } else if composition_session_id != 0 {
            return EditorEditOutcome::StaleRevision(self.stale_session_result());
        }

        let (byte_start, byte_end_exclusive) = Self::normalize_range(byte_start, byte_end_exclusive);

        if let Some(ref session) = self.composition_session {
            if byte_start != session.replace_start || byte_end_exclusive != session.replace_end_exclusive {
                return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
            }
        }

        if byte_start == byte_end_exclusive && replacement_text.is_empty() && self.composition_session.is_none() {
            return EditorEditOutcome::NoChange(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if byte_start > self.text.len() || byte_end_exclusive > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(byte_start) || !self.text.is_char_boundary(byte_end_exclusive) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let old_text = self.text.clone();

        self.text.replace_range(byte_start..byte_end_exclusive, replacement_text);
        self.revision = self.revision.saturating_add(1);

        let sel_anchor = Self::clamp_to_char_boundary(&self.text, resulting_selection_anchor);
        let sel_head = Self::clamp_to_char_boundary(&self.text, resulting_selection_head);
        let selection_was_adjusted = sel_anchor != resulting_selection_anchor || sel_head != resulting_selection_head;
        self.selection_anchor = sel_anchor;
        self.cursor = sel_head;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
        });
        self.redo_stack.clear();
        let preedit_byte_len = self.composition_session.as_ref().map(|s| s.preedit_text.len()).unwrap_or(0);
        self.composition_session = None;

        let new_revision = self.revision;
        let new_selection = (self.selection_anchor, self.cursor);
        let old_affected = if preedit_byte_len > 0 {
            vec![(byte_start, byte_start + preedit_byte_len)]
        } else {
            vec![(byte_start, byte_end_exclusive)]
        };
        let new_affected = vec![(byte_start, byte_start + replacement_text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (byte_start, byte_end_exclusive),
            inserted_text: replacement_text.to_string(),
            resulting_selection_byte_range: new_selection,
        }];

        let cluster_count = count_grapheme_clusters(replacement_text);
        let contains_newline = replacement_text.contains('\n');
        let contains_complex = text_contains_complex_grapheme(replacement_text);
        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false, false, false, false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::CompositionCommit,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: self.animation_enabled && old_cursor != self.cursor,
            },
        };

        let edit_result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        };

        if selection_was_adjusted {
            EditorEditOutcome::AppliedWithAdjustedSelection(edit_result)
        } else {
            EditorEditOutcome::Applied(edit_result)
        }
    }

    /// 删除选区前后的文本（IME DeleteSurrounding 命令）。
    ///
    /// after_range 必须在选区右侧（start ≥ sel_max），
    /// before_range 必须在选区左侧（end ≤ sel_min）。
    /// 先删 after 再删 before，避免 offset 偏移。
    /// 删除后根据删除量调整选区 anchor/head。
    fn apply_delete_surrounding(
        &mut self,
        before_byte_start: usize,
        before_byte_end_exclusive: usize,
        after_byte_start: usize,
        after_byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let sel_anchor = self.selection_anchor;
        let sel_head = self.cursor;
        let (sel_min, sel_max) = if sel_anchor <= sel_head { (sel_anchor, sel_head) } else { (sel_head, sel_anchor) };

        let mut patches = Vec::new();
        let mut text = self.text.clone();

        let after_range = if after_byte_start < after_byte_end_exclusive {
            Some((after_byte_start, after_byte_end_exclusive))
        } else {
            None
        };
        let before_range = if before_byte_start < before_byte_end_exclusive {
            Some((before_byte_start, before_byte_end_exclusive))
        } else {
            None
        };

        if let Some((as_, ae)) = after_range {
            if as_ > text.len() || ae > text.len() || !text.is_char_boundary(as_) || !text.is_char_boundary(ae) {
                return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
            }
            if as_ >= ae || as_ < sel_max {
                return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
            }
            text.replace_range(as_..ae, "");
            patches.push((as_, ae, String::new()));
        }

        if let Some((bs, be)) = before_range {
            if bs > text.len() || be > text.len() || !text.is_char_boundary(bs) || !text.is_char_boundary(be) {
                return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
            }
            if bs >= be || be > sel_min {
                return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
            }
            text.replace_range(bs..be, "");
            patches.push((bs, be, String::new()));
        }

        if patches.is_empty() {
            return EditorEditOutcome::NoChange(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let old_text = self.text.clone();
        self.text = text;
        self.revision = self.revision.saturating_add(1);
        self.composition_session = None;

        let before_deleted_len: usize = if let Some((bs, be)) = before_range {
            be.saturating_sub(bs)
        } else {
            0
        };

        let new_sel_anchor = if sel_anchor == sel_min {
            sel_min.saturating_sub(before_deleted_len)
        } else {
            sel_max.saturating_sub(before_deleted_len)
        };
        let new_sel_head = if sel_head == sel_min {
            sel_min.saturating_sub(before_deleted_len)
        } else {
            sel_max.saturating_sub(before_deleted_len)
        };
        self.selection_anchor = new_sel_anchor;
        self.cursor = new_sel_head;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = (self.selection_anchor, self.cursor);

        let (replace_range, inserted_text) = Self::compute_single_patch(&old_text, &self.text);
        let display_patches = if replace_range.0 < replace_range.1 || !inserted_text.is_empty() {
            vec![DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: replace_range,
                inserted_text,
                resulting_selection_byte_range: new_selection,
            }]
        } else {
            vec![]
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Delete,
            old_affected_byte_ranges: patches.iter().map(|(s, e, _)| (*s, *e)).collect(),
            new_affected_byte_ranges: vec![],
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: self.cursor,
                should_animate: false,
            },
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }

    /// 开始一次 IME composition 会话。
    ///
    /// 同一时刻只允许一个活跃 composition 会话（重复调用返回 InvalidRange）。
    /// 会话记录 replace 范围（半开区间），后续 UpdateComposition/FinishComposition
    /// 必须与此范围匹配，否则返回 InvalidRange。
    fn apply_begin_composition(
        &mut self,
        replace_start: usize,
        replace_end_exclusive: usize,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        if self.composition_session.is_some() {
            return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if replace_start > replace_end_exclusive {
            return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if replace_start > self.text.len() || replace_end_exclusive > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(replace_start) || !self.text.is_char_boundary(replace_end_exclusive) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let session_id = self.next_composition_session_id;
        self.next_composition_session_id = self.next_composition_session_id.saturating_add(1);

        self.composition_session = Some(CompositionSessionState {
            session_id,
            base_revision,
            generation: 0,
            replace_start,
            replace_end_exclusive,
            preedit_text: String::new(),
            preedit_cursor_utf16: 0,
        });

        let new_selection = (self.selection_anchor, self.cursor);
        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::ImeComposition,
                operation_kind: EditorOperationKind::CompositionUpdate,
                old_affected_byte_ranges: vec![],
                new_affected_byte_ranges: vec![],
                animation_mode: AnimationMode::SystemSuppressed,
                duration_ms: 0,
                coordinated_cursor: CoordinatedCursor {
                    old_byte_offset: old_cursor,
                    new_byte_offset: self.cursor,
                    should_animate: false,
                },
            },
        })
    }

    /// 更新 composition 的 preedit 文本。
    ///
    /// 必须匹配活跃会话的 session_id、generation 和 base_revision，
    /// 否则返回 StaleRevision（会话可能已被 reset）。
    /// 更新后 generation 递增，使后续操作必须使用新的 generation。
    /// 此操作不修改正文，只更新 composition 状态供平台渲染 preedit。
    fn apply_update_composition(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        new_preedit_text: &str,
        new_preedit_cursor_offset: usize,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let session = match &mut self.composition_session {
            Some(s) if s.session_id == composition_session_id
                && s.generation == composition_generation
                && s.base_revision == base_revision => s,
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        let old_preedit_text = session.preedit_text.clone();
        let replace_start = session.replace_start;

        session.preedit_text = new_preedit_text.to_string();
        session.preedit_cursor_utf16 = new_preedit_cursor_offset;
        session.generation = session.generation.saturating_add(1);

        let old_affected = if old_preedit_text.is_empty() {
            vec![]
        } else {
            vec![(replace_start, replace_start + old_preedit_text.len())]
        };
        let new_affected = if new_preedit_text.is_empty() {
            vec![]
        } else {
            vec![(replace_start, replace_start + new_preedit_text.len())]
        };

        let changed_text: &str = if new_preedit_text.len() >= old_preedit_text.len() {
            new_preedit_text
        } else {
            &old_preedit_text
        };
        let cluster_count = count_grapheme_clusters(changed_text);
        let contains_newline = changed_text.contains('\n');
        let contains_complex = text_contains_complex_grapheme(changed_text);
        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false, false, false, false,
                self.animation_enabled,
            )
        };

        let new_selection = (self.selection_anchor, self.cursor);
        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::ImeComposition,
                operation_kind: EditorOperationKind::CompositionUpdate,
                old_affected_byte_ranges: old_affected,
                new_affected_byte_ranges: new_affected,
                animation_mode,
                duration_ms: self.animation_duration_ms,
                coordinated_cursor: CoordinatedCursor {
                    old_byte_offset: old_cursor,
                    new_byte_offset: self.cursor,
                    should_animate: self.animation_enabled && old_cursor != self.cursor,
                },
            },
        })
    }

    /// 确认 composition，将 preedit 文本写入正文。
    ///
    /// 会话的 preedit 文本替换 [replace_start, replace_end_exclusive) 范围。
    /// 光标位置由 preedit_cursor_utf16（UTF-16 单位）转换为 UTF-8 byte offset。
    /// 空预输入文本时仅关闭会话，不修改正文。
    /// 确认后会话销毁，undo/redo 栈正常记录。
    fn apply_finish_composition(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let session = match &self.composition_session {
            Some(s) if s.session_id == composition_session_id
                && s.generation == composition_generation
                && s.base_revision == base_revision => s.clone(),
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        if session.preedit_text.is_empty() {
            self.composition_session = None;
            let new_selection = (self.selection_anchor, self.cursor);
            return EditorEditOutcome::Applied(EditorEditResult {
                transaction_id: self.take_transaction_id(),
                base_revision,
                new_revision: self.revision,
                display_patches: vec![],
                old_selection_byte_range: old_selection,
                new_selection_byte_range: new_selection,
                visual_intent: EditorVisualIntent {
                    cause: EditorTransactionCause::TypingCommit,
                    operation_kind: EditorOperationKind::CompositionCommit,
                    old_affected_byte_ranges: vec![],
                    new_affected_byte_ranges: vec![],
                    animation_mode: AnimationMode::SystemSuppressed,
                    duration_ms: 0,
                    coordinated_cursor: CoordinatedCursor {
                        old_byte_offset: old_cursor,
                        new_byte_offset: self.cursor,
                        should_animate: false,
                    },
                },
            });
        }

        let replace_start = session.replace_start;
        let replace_end = session.replace_end_exclusive;
        let committed_text = session.preedit_text.clone();
        let preedit_cursor_utf16 = session.preedit_cursor_utf16;

        if replace_start > self.text.len() || replace_end > self.text.len() {
            self.composition_session = None;
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let old_text = self.text.clone();
        self.text.replace_range(replace_start..replace_end, &committed_text);
        self.revision = self.revision.saturating_add(1);

        let committed_utf16_len: usize = committed_text.chars().map(|c| c.len_utf16()).sum();
        let preedit_cursor_utf16_clamped = preedit_cursor_utf16.min(committed_utf16_len);

        let resulting_cursor_before_clamp = if preedit_cursor_utf16_clamped > 0 {
            let mut utf16_count = 0usize;
            let mut byte_offset = 0usize;
            for ch in committed_text.chars() {
                if utf16_count >= preedit_cursor_utf16_clamped {
                    break;
                }
                let ch_len_utf16 = ch.len_utf16();
                utf16_count += ch_len_utf16;
                byte_offset += ch.len_utf8();
            }
            replace_start + byte_offset
        } else {
            replace_start
        };
        let resulting_cursor = Self::clamp_to_char_boundary(&self.text, resulting_cursor_before_clamp);
        let selection_was_adjusted = resulting_cursor != resulting_cursor_before_clamp
            || preedit_cursor_utf16 != preedit_cursor_utf16_clamped;
        self.cursor = resulting_cursor;
        self.selection_anchor = self.cursor;
        self.composition_session = None;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = (self.cursor, self.cursor);

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (replace_start, replace_end),
            inserted_text: committed_text.clone(),
            resulting_selection_byte_range: new_selection,
        }];

        let cluster_count = count_grapheme_clusters(&committed_text);
        let contains_newline = committed_text.contains('\n');
        let contains_complex = text_contains_complex_grapheme(&committed_text);
        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false, false, false, false,
                self.animation_enabled,
            )
        };

        let edit_result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::TypingCommit,
                operation_kind: EditorOperationKind::CompositionCommit,
                old_affected_byte_ranges: vec![(replace_start, replace_start + committed_text.len())],
                new_affected_byte_ranges: vec![(replace_start, replace_start + committed_text.len())],
                animation_mode,
                duration_ms: self.animation_duration_ms,
                coordinated_cursor: CoordinatedCursor {
                    old_byte_offset: old_cursor,
                    new_byte_offset: self.cursor,
                    should_animate: self.animation_enabled && old_cursor != self.cursor,
                },
            },
        };

        if selection_was_adjusted {
            EditorEditOutcome::AppliedWithAdjustedSelection(edit_result)
        } else {
            EditorEditOutcome::Applied(edit_result)
        }
    }

    /// 取消 composition，恢复到 composition 开始前的正文状态。
    ///
    /// 不修改正文（preedit 文本从未写入正文），仅销毁会话。
    /// 返回的 old_affected 告知平台需要清除的 preedit 渲染区域。
    fn apply_cancel_composition(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let session = match &self.composition_session {
            Some(s) if s.session_id == composition_session_id
                && s.generation == composition_generation
                && s.base_revision == base_revision => s.clone(),
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        let replace_start = session.replace_start;
        let replace_end = session.replace_end_exclusive;

        if replace_start != replace_end && (replace_start > self.text.len() || replace_end > self.text.len()) {
            self.composition_session = None;
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        self.composition_session = None;

        let preedit_byte_len = session.preedit_text.len();
        let old_affected = if preedit_byte_len > 0 {
            vec![(replace_start, replace_start + preedit_byte_len)]
        } else if replace_start != replace_end {
            vec![(replace_start, replace_end)]
        } else {
            vec![]
        };

        let animation_mode = if !self.animation_enabled || old_affected.is_empty() {
            AnimationMode::SystemSuppressed
        } else {
            AnimationMode::ClusterAnimation
        };

        let new_selection = (self.selection_anchor, self.cursor);
        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::ImeComposition,
                operation_kind: EditorOperationKind::CompositionCancel,
                old_affected_byte_ranges: old_affected,
                new_affected_byte_ranges: vec![],
                animation_mode,
                duration_ms: self.animation_duration_ms,
                coordinated_cursor: CoordinatedCursor {
                    old_byte_offset: old_cursor,
                    new_byte_offset: self.cursor,
                    should_animate: self.animation_enabled && old_cursor != self.cursor,
                },
            },
        })
    }

    pub fn composition_session_info(&self) -> Option<(u64, u64, u64)> {
        self.composition_session.as_ref().map(|s| (s.session_id, s.base_revision, s.generation))
    }

    /// 创建 CompositionUpdate 事务 — 预输入更新。
    ///
    /// composing 更新不会修改 committed text、Undo、保存和同步状态。
    pub fn composition_update(
        &mut self,
        composition_replace_range: Option<(usize, usize)>,
        old_preedit_text: &str,
        new_preedit_text: &str,
    ) -> CompositionUpdateTransaction {
        let mut engine = super::transaction::EditorEngine::with_animation_limits(
            self.max_animated_chars,
            self.animation_duration_ms,
        );
        engine.composition_update_transaction(
            &self.text,
            composition_replace_range,
            old_preedit_text,
            new_preedit_text,
        )
    }

    /// 获取 CompositionUpdate 的 VisualIntent。
    ///
    /// 平台端在 composition update 时调用此方法获取动画意图，
    /// 然后交给 VisualPlanner 生成视觉事务。
    /// committed text 不变，displayPatches 为空。
    pub fn composition_update_visual_intent(
        &self,
        composition_replace_range: Option<(usize, usize)>,
        old_preedit_text: &str,
        new_preedit_text: &str,
    ) -> EditorVisualIntent {
        let replace_start = composition_replace_range
            .map(|(s, _)| s)
            .unwrap_or(self.cursor);
        let new_end = replace_start + new_preedit_text.len();

        let changed_text = if new_preedit_text.len() >= old_preedit_text.len() {
            new_preedit_text
        } else {
            old_preedit_text
        };
        let cluster_count = count_grapheme_clusters(changed_text);
        let contains_newline = changed_text.contains('\n');
        let contains_complex = text_contains_complex_grapheme(changed_text);
        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false, false, false, false,
                self.animation_enabled,
            )
        };

        EditorVisualIntent {
            cause: EditorTransactionCause::ImeComposition,
            operation_kind: EditorOperationKind::CompositionUpdate,
            old_affected_byte_ranges: if old_preedit_text.is_empty() {
                vec![]
            } else {
                vec![(replace_start, replace_start + old_preedit_text.len())]
            },
            new_affected_byte_ranges: if new_preedit_text.is_empty() {
                vec![]
            } else {
                vec![(replace_start, new_end)]
            },
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: self.cursor,
                new_byte_offset: new_end,
                should_animate: self.animation_enabled,
            },
        }
    }

    /// 创建 CompositionCommitOrCancel 事务。
    pub fn composition_commit_or_cancel(
        &mut self,
        composition_revision: CompositionVisualRevision,
        committed_text_after: &str,
        is_commit: bool,
    ) -> CompositionCommitOrCancelTransaction {
        let mut engine = super::transaction::EditorEngine::with_animation_limits(
            self.max_animated_chars,
            self.animation_duration_ms,
        );
        engine.composition_commit_or_cancel_transaction(
            &self.text,
            committed_text_after,
            composition_revision,
            is_commit,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_command_produces_display_patch() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 6,
            text: "世界".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();

        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);
        assert_eq!(result.base_revision, 0);
        assert_eq!(result.new_revision, 1);
        assert!(!result.display_patches.is_empty());
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Insert);
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::Typing);
    }

    #[test]
    fn delete_command_produces_display_patch() {
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let result = kernel.apply(EditorCommand::Delete {
            byte_start: 6,
            byte_end_exclusive: 12,
            deleted_text: "世界".to_string(),
            cause: EditorTransactionCause::Delete,
            expected_revision: 0,
        }).into_result();

        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Delete);
    }

    #[test]
    fn replace_command_produces_display_patch() {
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 6,
            byte_end_exclusive: 12,
            replacement_text: "朋友".to_string(),
            original_text: "世界".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();

        assert_eq!(kernel.text(), "你好朋友");
        assert_eq!(kernel.cursor(), 12);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Replace);
    }

    #[test]
    fn set_selection_does_not_modify_text() {
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let result = kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 0,
            head_byte_offset: 3,
            expected_revision: 0,
        }).into_result();
        assert_eq!(kernel.text(), "hello");
        assert_eq!(kernel.cursor(), 3);
        assert_eq!(result.display_patches.len(), 0);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::CursorOnly);
    }

    #[test]
    fn undo_restores_previous_state() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let r1 = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();
        assert_eq!(kernel.text(), "abc");

        let result = kernel.apply(EditorCommand::Undo { expected_revision: r1.new_revision }).into_result();
        assert_eq!(kernel.text(), "ab");
        assert_eq!(kernel.cursor(), 2);
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::Undo);
    }

    #[test]
    fn redo_restores_undone_state() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let r1 = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();
        let r2 = kernel.apply(EditorCommand::Undo { expected_revision: r1.new_revision }).into_result();
        assert_eq!(kernel.text(), "ab");

        let result = kernel.apply(EditorCommand::Redo { expected_revision: r2.new_revision }).into_result();
        assert_eq!(kernel.text(), "abc");
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::Redo);
    }

    #[test]
    fn load_text_resets_state() {
        let mut kernel = EditorKernel::with_text("old".to_string(), 3).unwrap();
        kernel.apply(EditorCommand::Insert {
            byte_offset: 3,
            text: " text".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();

        let result = kernel.load_text("new content".to_string(), 0).into_result();
        assert_eq!(kernel.text(), "new content");
        assert_eq!(kernel.cursor(), 0);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Load);
        assert_eq!(result.visual_intent.animation_mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn revision_increments_on_each_edit() {
        let mut kernel = EditorKernel::new();
        assert_eq!(kernel.revision(), 0);

        let r1 = kernel.apply(EditorCommand::Insert {
            byte_offset: 0,
            text: "a".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();
        assert_eq!(kernel.revision(), 1);

        kernel.apply(EditorCommand::Insert {
            byte_offset: 1,
            text: "b".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: r1.new_revision,
        }).into_result();
        assert_eq!(kernel.revision(), 2);
    }

    #[test]
    fn display_patch_contains_correct_ranges() {
        let mut kernel = EditorKernel::with_text("hello world".to_string(), 11).unwrap();
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 6,
            byte_end_exclusive: 11,
            replacement_text: "rust".to_string(),
            original_text: "world".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();

        assert_eq!(result.display_patches.len(), 1);
        assert_eq!(result.display_patches[0].replace_byte_range, (6, 11));
        assert_eq!(result.display_patches[0].inserted_text, "rust");
    }

    #[test]
    fn coordinated_cursor_tracks_movement() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let result = kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 0,
            head_byte_offset: 0,
            expected_revision: 0,
        }).into_result();
        assert_eq!(result.visual_intent.coordinated_cursor.old_byte_offset, 3);
        assert_eq!(result.visual_intent.coordinated_cursor.new_byte_offset, 0);
        assert!(result.visual_intent.coordinated_cursor.should_animate);
    }

    #[test]
    fn animation_disabled_suppresses_animation() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        kernel.set_animation_enabled(false);

        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();

        assert_eq!(result.visual_intent.animation_mode, AnimationMode::SystemSuppressed);
        assert!(!result.visual_intent.coordinated_cursor.should_animate);
    }

    #[test]
    fn undo_then_new_edit_clears_redo() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let r1 = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();
        let r2 = kernel.apply(EditorCommand::Undo { expected_revision: r1.new_revision }).into_result();
        assert_eq!(kernel.text(), "ab");

        let r3 = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "d".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: r2.new_revision,
        }).into_result();

        let result = kernel.apply(EditorCommand::Redo { expected_revision: r3.new_revision }).into_result();
        assert_eq!(kernel.text(), "abd");
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::CursorOnly);
    }

    #[test]
    fn edit_result_serializes_camel_case() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"transactionId\":"), "JSON should use camelCase for transactionId, got: {}", json);
        assert!(json.contains("\"baseRevision\":"), "JSON should use camelCase for baseRevision, got: {}", json);
        assert!(json.contains("\"newRevision\":"), "JSON should use camelCase for newRevision, got: {}", json);
        assert!(json.contains("\"displayPatches\":"), "JSON should use camelCase for displayPatches, got: {}", json);
        assert!(json.contains("\"visualIntent\":"), "JSON should use camelCase for visualIntent, got: {}", json);
        assert!(json.contains("\"operationKind\":"), "JSON should use camelCase for operationKind, got: {}", json);
        assert!(json.contains("\"animationMode\":"), "JSON should use camelCase for animationMode, got: {}", json);
        assert!(json.contains("\"durationMs\":"), "JSON should use camelCase for durationMs, got: {}", json);
        assert!(json.contains("\"coordinatedCursor\":"), "JSON should use camelCase for coordinatedCursor, got: {}", json);
        assert!(json.contains("\"oldByteOffset\":"), "JSON should use camelCase for oldByteOffset, got: {}", json);
        assert!(json.contains("\"newByteOffset\":"), "JSON should use camelCase for newByteOffset, got: {}", json);
        assert!(json.contains("\"shouldAnimate\":"), "JSON should use camelCase for shouldAnimate, got: {}", json);
        assert!(json.contains("\"replaceByteRange\":"), "JSON should use camelCase for replaceByteRange, got: {}", json);
        assert!(json.contains("\"insertedText\":"), "JSON should use camelCase for insertedText, got: {}", json);
        assert!(json.contains("\"resultingSelectionByteRange\":"), "JSON should use camelCase for resultingSelectionByteRange, got: {}", json);
    }

    #[test]
    fn display_patch_serializes_camel_case() {
        let patch = DisplayPatch {
            base_revision: 0,
            new_revision: 1,
            replace_byte_range: (2, 3),
            inserted_text: "c".to_string(),
            resulting_selection_byte_range: (3, 3),
        };
        let json = serde_json::to_string(&patch).unwrap();
        assert!(json.contains("\"replaceByteRange\":"), "DisplayPatch JSON should use camelCase, got: {}", json);
        assert!(json.contains("\"insertedText\":"), "DisplayPatch JSON should use camelCase, got: {}", json);
        assert!(json.contains("\"resultingSelectionByteRange\":"), "DisplayPatch JSON should use camelCase, got: {}", json);
    }

    #[test]
    fn composition_update_does_not_modify_text() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let tx = kernel.composition_update(
            None,
            "",
            "nihao",
        );
        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
        assert_eq!(tx.new_revision.preedit_text, "nihao");
    }

    #[test]
    fn composition_commit_modifies_text_via_replace() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 6,
            byte_end_exclusive: 6,
            replacement_text: "你好".to_string(),
            original_text: String::new(),
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: 0,
        }).into_result();
        assert_eq!(kernel.text(), "你好你好");
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Replace);
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::TypingCommit);
    }

    #[test]
    fn delete_empty_range_is_noop() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let outcome = kernel.apply(EditorCommand::Delete {
            byte_start: 2,
            byte_end_exclusive: 2,
            deleted_text: String::new(),
            cause: EditorTransactionCause::Delete,
            expected_revision: 0,
        });
        assert_eq!(kernel.text(), "abc");
        assert!(matches!(outcome, EditorEditOutcome::InvalidRange(_)));
    }

    #[test]
    fn insert_beyond_length_returns_invalid_offset() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let outcome = kernel.apply(EditorCommand::Insert {
            byte_offset: 100,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        });
        assert!(matches!(outcome, EditorEditOutcome::InvalidOffset(_)));
        assert_eq!(kernel.text(), "ab");
    }

    #[test]
    fn replace_same_text_produces_patch() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 1,
            byte_end_exclusive: 2,
            replacement_text: "X".to_string(),
            original_text: "b".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();
        assert_eq!(kernel.text(), "aXc");
        assert!(!result.display_patches.is_empty());
    }

    #[test]
    fn undo_after_multiple_edits_restores_correctly() {
        let mut kernel = EditorKernel::new();
        let r1 = kernel.apply(EditorCommand::Insert {
            byte_offset: 0,
            text: "a".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();
        let r2 = kernel.apply(EditorCommand::Insert {
            byte_offset: 1,
            text: "b".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: r1.new_revision,
        }).into_result();
        let r3 = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: r2.new_revision,
        }).into_result();
        assert_eq!(kernel.text(), "abc");

        let r4 = kernel.apply(EditorCommand::Undo { expected_revision: r3.new_revision }).into_result();
        assert_eq!(kernel.text(), "ab");

        let r5 = kernel.apply(EditorCommand::Undo { expected_revision: r4.new_revision }).into_result();
        assert_eq!(kernel.text(), "a");

        kernel.apply(EditorCommand::Undo { expected_revision: r5.new_revision }).into_result();
        assert_eq!(kernel.text(), "");
    }

    #[test]
    fn cjk_insert_and_delete() {
        let mut kernel = EditorKernel::new();
        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 0,
            text: "你好世界".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);

        let result = kernel.apply(EditorCommand::Delete {
            byte_start: 6,
            byte_end_exclusive: 12,
            deleted_text: "世界".to_string(),
            cause: EditorTransactionCause::Delete,
            expected_revision: result.new_revision,
        }).into_result();
        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Delete);
    }

    #[test]
    fn set_selection_with_same_position_no_cursor_animation() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 1).unwrap();
        let result = kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 1,
            head_byte_offset: 1,
            expected_revision: 0,
        }).into_result();
        assert!(!result.visual_intent.coordinated_cursor.should_animate);
    }

    #[test]
    fn load_text_clears_undo_stack() {
        let mut kernel = EditorKernel::with_text("old".to_string(), 3).unwrap();
        kernel.apply(EditorCommand::Insert {
            byte_offset: 3,
            text: " text".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();
        kernel.load_text("new".to_string(), 3).into_result();
        let result = kernel.apply(EditorCommand::Undo { expected_revision: 0 });
        assert!(matches!(result, EditorEditOutcome::StaleRevision(_)));
        assert_eq!(kernel.text(), "new");
    }

    #[test]
    fn with_text_rejects_invalid_cursor_offset() {
        let result = EditorKernel::with_text("你好".to_string(), 4);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EditorInputError::InvalidCursorOffset { .. }));
    }

    #[test]
    fn stale_revision_returns_stale_outcome() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let outcome = kernel.apply(EditorCommand::Insert {
            byte_offset: 3,
            text: "d".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 99,
        });
        assert!(matches!(outcome, EditorEditOutcome::StaleRevision(_)));
    }

    #[test]
    fn invalid_offset_returns_invalid_outcome() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let outcome = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "X".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
    }

    #[test]
    fn atomic_display_patch_for_replace() {
        let mut kernel = EditorKernel::with_text("hello world".to_string(), 11).unwrap();
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 6,
            byte_end_exclusive: 11,
            replacement_text: "rust".to_string(),
            original_text: "world".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();
        assert_eq!(result.display_patches.len(), 1);
        let patch = &result.display_patches[0];
        assert_eq!(patch.replace_byte_range, (6, 11));
        assert_eq!(patch.inserted_text, "rust");
    }

    #[test]
    fn composition_update_visual_intent_returns_correct_intent() {
        let kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let intent = kernel.composition_update_visual_intent(
            None,
            "",
            "nihao",
        );
        assert_eq!(intent.cause, EditorTransactionCause::ImeComposition);
        assert_eq!(intent.operation_kind, EditorOperationKind::CompositionUpdate);
        assert_eq!(intent.animation_mode, AnimationMode::GlyphAnimation);
        assert_eq!(intent.duration_ms, 160);
        assert!(intent.coordinated_cursor.should_animate);
        assert!(!intent.new_affected_byte_ranges.is_empty());
    }

    #[test]
    fn composition_update_visual_intent_with_replace_range() {
        let kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let intent = kernel.composition_update_visual_intent(
            Some((6, 12)),
            "世界",
            "朋友",
        );
        assert_eq!(intent.operation_kind, EditorOperationKind::CompositionUpdate);
        assert_eq!(intent.old_affected_byte_ranges, vec![(6, 12)]);
        assert_eq!(intent.new_affected_byte_ranges, vec![(6, 12)]);
    }

    #[test]
    fn compute_single_patch_cjk_replace() {
        let ((start, end), inserted) = EditorKernel::compute_single_patch("你好", "你坏");
        assert_eq!(start, 3);
        assert_eq!(end, 6);
        assert_eq!(inserted, "坏");
    }

    #[test]
    fn compute_single_patch_cjk_insert() {
        let ((start, end), inserted) = EditorKernel::compute_single_patch("你好", "你好世界");
        assert_eq!(start, 6);
        assert_eq!(end, 6);
        assert_eq!(inserted, "世界");
    }

    #[test]
    fn compute_single_patch_cjk_delete() {
        let ((start, end), inserted) = EditorKernel::compute_single_patch("你好世界", "你好");
        assert_eq!(start, 6);
        assert_eq!(end, 12);
        assert_eq!(inserted, "");
    }

    #[test]
    fn compute_single_patch_mixed_ascii_cjk() {
        let ((start, end), inserted) = EditorKernel::compute_single_patch("a你好b", "a你坏b");
        assert_eq!(start, 4);
        assert_eq!(end, 7);
        assert_eq!(inserted, "坏");
    }

    #[test]
    fn compute_single_patch_empty_old() {
        let ((start, end), inserted) = EditorKernel::compute_single_patch("", "你好");
        assert_eq!(start, 0);
        assert_eq!(end, 0);
        assert_eq!(inserted, "你好");
    }

    #[test]
    fn compute_single_patch_empty_new() {
        let ((start, end), inserted) = EditorKernel::compute_single_patch("你好", "");
        assert_eq!(start, 0);
        assert_eq!(end, 6);
        assert_eq!(inserted, "");
    }

    #[test]
    fn compute_single_patch_identical() {
        let ((start, end), inserted) = EditorKernel::compute_single_patch("你好", "你好");
        assert_eq!(start, 0);
        assert_eq!(end, 0);
        assert_eq!(inserted, "");
    }

    #[test]
    fn delete_surrounding_both_sides_preserves_selection() {
        let mut kernel = EditorKernel::with_text("ABCDEFGHIJ".to_string(), 3).unwrap();
        kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 3,
            head_byte_offset: 6,
            expected_revision: 0,
        }).into_result();

        let outcome = kernel.apply(EditorCommand::DeleteSurrounding {
            before_byte_start: 0,
            before_byte_end_exclusive: 2,
            after_byte_start: 7,
            after_byte_end_exclusive: 9,
            cause: EditorTransactionCause::Delete,
            expected_revision: 0,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "CDEFGJ");
        assert_eq!(kernel.selection_anchor(), 1);
        assert_eq!(kernel.cursor(), 4);
    }

    #[test]
    fn delete_surrounding_before_only_shifts_selection() {
        let mut kernel = EditorKernel::with_text("ABCDE".to_string(), 3).unwrap();
        kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 3,
            head_byte_offset: 5,
            expected_revision: 0,
        }).into_result();

        let outcome = kernel.apply(EditorCommand::DeleteSurrounding {
            before_byte_start: 0,
            before_byte_end_exclusive: 2,
            after_byte_start: 0,
            after_byte_end_exclusive: 0,
            cause: EditorTransactionCause::Delete,
            expected_revision: 0,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "CDE");
        assert_eq!(kernel.selection_anchor(), 1);
        assert_eq!(kernel.cursor(), 3);
    }

    #[test]
    fn delete_surrounding_after_only_preserves_selection() {
        let mut kernel = EditorKernel::with_text("ABCDE".to_string(), 3).unwrap();
        kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 0,
            head_byte_offset: 3,
            expected_revision: 0,
        }).into_result();

        let outcome = kernel.apply(EditorCommand::DeleteSurrounding {
            before_byte_start: 0,
            before_byte_end_exclusive: 0,
            after_byte_start: 4,
            after_byte_end_exclusive: 5,
            cause: EditorTransactionCause::Delete,
            expected_revision: 0,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "ABCD");
        assert_eq!(kernel.selection_anchor(), 0);
        assert_eq!(kernel.cursor(), 3);
    }

    #[test]
    fn commit_text_with_session_validation() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel.apply(EditorCommand::BeginComposition {
            replace_start: 6,
            replace_end_exclusive: 6,
            expected_revision: 0,
        }).into_result();

        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_start: 6,
            byte_end_exclusive: 6,
            replacement_text: "世界".to_string(),
            resulting_selection_anchor: 12,
            resulting_selection_head: 12,
            composition_session_id: session_id,
            composition_base_revision: base_rev,
            composition_generation: gen,
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn commit_text_wrong_session_returns_stale() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        kernel.apply(EditorCommand::BeginComposition {
            replace_start: 6,
            replace_end_exclusive: 6,
            expected_revision: 0,
        }).into_result();

        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_start: 6,
            byte_end_exclusive: 6,
            replacement_text: "世界".to_string(),
            resulting_selection_anchor: 12,
            resulting_selection_head: 12,
            composition_session_id: 999,
            composition_base_revision: 0,
            composition_generation: 0,
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: 1,
        });
        assert!(matches!(outcome, EditorEditOutcome::StaleRevision(_)));
        assert_eq!(kernel.text(), "你好");
    }

    #[test]
    fn commit_text_empty_string_deletes_range() {
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let begin = kernel.apply(EditorCommand::BeginComposition {
            replace_start: 6,
            replace_end_exclusive: 12,
            expected_revision: 0,
        }).into_result();

        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_start: 6,
            byte_end_exclusive: 12,
            replacement_text: String::new(),
            resulting_selection_anchor: 6,
            resulting_selection_head: 6,
            composition_session_id: session_id,
            composition_base_revision: base_rev,
            composition_generation: gen,
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
    }

    #[test]
    fn finish_composition_materializes_preedit() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel.apply(EditorCommand::BeginComposition {
            replace_start: 6,
            replace_end_exclusive: 6,
            expected_revision: 0,
        }).into_result();

        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();

        kernel.apply(EditorCommand::UpdateComposition {
            composition_session_id: session_id,
            composition_generation: gen,
            new_preedit_text: "世界".to_string(),
            new_preedit_cursor_offset: 2,
            expected_revision: begin.new_revision,
        }).into_result();

        let (_, _, new_gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::FinishComposition {
            composition_session_id: session_id,
            composition_generation: new_gen,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn finish_composition_empty_preedit_no_text_change() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel.apply(EditorCommand::BeginComposition {
            replace_start: 6,
            replace_end_exclusive: 6,
            expected_revision: 0,
        }).into_result();

        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::FinishComposition {
            composition_session_id: session_id,
            composition_generation: gen,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn cancel_composition_preserves_text() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel.apply(EditorCommand::BeginComposition {
            replace_start: 3,
            replace_end_exclusive: 6,
            expected_revision: 0,
        }).into_result();

        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();

        kernel.apply(EditorCommand::UpdateComposition {
            composition_session_id: session_id,
            composition_generation: gen,
            new_preedit_text: "坏".to_string(),
            new_preedit_cursor_offset: 3,
            expected_revision: begin.new_revision,
        }).into_result();

        let (_, _, new_gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::CancelComposition {
            composition_session_id: session_id,
            composition_generation: new_gen,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好");
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn stale_revision_returns_current_revision() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let r1 = kernel.apply(EditorCommand::Insert {
            byte_offset: 3,
            text: "d".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        }).into_result();
        assert_eq!(r1.new_revision, 1);

        let outcome = kernel.apply(EditorCommand::Insert {
            byte_offset: 4,
            text: "e".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 99,
        });
        match outcome {
            EditorEditOutcome::StaleRevision(result) => {
                assert_eq!(result.base_revision, 1);
                assert_eq!(result.new_revision, 1);
            }
            _ => panic!("expected StaleRevision"),
        }
    }

    #[test]
    fn load_text_with_invalid_cursor_returns_adjusted() {
        let mut kernel = EditorKernel::with_text("old".to_string(), 3).unwrap();
        let outcome = kernel.load_text("new".to_string(), 100);
        assert!(matches!(outcome, EditorEditOutcome::AppliedWithAdjustedSelection(_)));
        assert_eq!(kernel.text(), "new");
        assert_eq!(kernel.cursor(), 3);
    }

    #[test]
    fn load_text_with_valid_cursor_returns_applied() {
        let mut kernel = EditorKernel::with_text("old".to_string(), 3).unwrap();
        let outcome = kernel.load_text("new".to_string(), 2);
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "new");
        assert_eq!(kernel.cursor(), 2);
    }

    #[test]
    fn commit_text_session_range_mismatch_returns_invalid_range() {
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let begin = kernel.apply(EditorCommand::BeginComposition {
            replace_start: 3,
            replace_end_exclusive: 6,
            expected_revision: 0,
        }).into_result();

        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_start: 6,
            byte_end_exclusive: 12,
            replacement_text: "XX".to_string(),
            resulting_selection_anchor: 8,
            resulting_selection_head: 8,
            composition_session_id: session_id,
            composition_base_revision: base_rev,
            composition_generation: gen,
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::InvalidRange(_)));
        assert_eq!(kernel.text(), "你好世界");
    }

    #[test]
    fn commit_text_empty_range_empty_text_no_session_returns_no_change() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let rev_before = kernel.revision();
        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_start: 3,
            byte_end_exclusive: 3,
            replacement_text: String::new(),
            resulting_selection_anchor: 3,
            resulting_selection_head: 3,
            composition_session_id: 0,
            composition_base_revision: 0,
            composition_generation: 0,
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        });
        assert!(matches!(outcome, EditorEditOutcome::NoChange(_)));
        assert_eq!(kernel.revision(), rev_before);
        assert_eq!(kernel.text(), "abc");
    }

    #[test]
    fn finish_composition_cursor_at_committed_text_end() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel.apply(EditorCommand::BeginComposition {
            replace_start: 6,
            replace_end_exclusive: 6,
            expected_revision: 0,
        }).into_result();

        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();

        kernel.apply(EditorCommand::UpdateComposition {
            composition_session_id: session_id,
            composition_generation: gen,
            new_preedit_text: "世界".to_string(),
            new_preedit_cursor_offset: 2,
            expected_revision: begin.new_revision,
        }).into_result();

        let (_, _, new_gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::FinishComposition {
            composition_session_id: session_id,
            composition_generation: new_gen,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);
        assert_eq!(kernel.selection_anchor(), 12);
    }

    #[test]
    fn finish_composition_cursor_exceeds_preedit_length_returns_adjusted() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel.apply(EditorCommand::BeginComposition {
            replace_start: 6,
            replace_end_exclusive: 6,
            expected_revision: 0,
        }).into_result();

        let (session_id, _base_rev, gen) = kernel.composition_session_info().unwrap();

        kernel.apply(EditorCommand::UpdateComposition {
            composition_session_id: session_id,
            composition_generation: gen,
            new_preedit_text: "世界".to_string(),
            new_preedit_cursor_offset: 99,
            expected_revision: begin.new_revision,
        }).into_result();

        let (_, _, new_gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::FinishComposition {
            composition_session_id: session_id,
            composition_generation: new_gen,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::AppliedWithAdjustedSelection(_)));
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn delete_surrounding_large_before_does_not_underflow() {
        let mut kernel = EditorKernel::with_text("ABCDE".to_string(), 3).unwrap();
        kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 3,
            head_byte_offset: 5,
            expected_revision: 0,
        }).into_result();

        let outcome = kernel.apply(EditorCommand::DeleteSurrounding {
            before_byte_start: 0,
            before_byte_end_exclusive: 2,
            after_byte_start: 0,
            after_byte_end_exclusive: 0,
            cause: EditorTransactionCause::Delete,
            expected_revision: 0,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "CDE");
        assert_eq!(kernel.selection_anchor(), 1);
        assert_eq!(kernel.cursor(), 3);
    }
}
