//! Rust EditorKernel — 正文和业务唯一真相。

pub mod types;
pub mod result;
mod tests;



use super::transaction::{
    choose_animation_mode, count_grapheme_clusters,
    diff_plain_text, text_contains_complex_grapheme,
    AnimationMode, CompositionUpdateTransaction,
    CompositionCommitOrCancelTransaction, CompositionVisualRevision,
    EditorChange, EditorTransactionCause,
};
use self::types::{EditorCommand, DisplayPatch, EditorVisualIntent, EditorOperationKind, CoordinatedCursor};
use self::result::{EditorEditOutcome, EditorEditResult, EditorInputError};

#[derive(Debug, Clone)]
pub struct EditorKernel {
    /// 正文纯文本。始终是合法 UTF-8，不含格式标记。
    text: String,
    /// 修订号——每次成功编辑 `saturating_add(1)`，用于乐观并发校验。
    /// 平台端通过 `expected_revision` 匹配，不匹配时返回 `StaleRevision`。
    revision: u64,
    /// 主光标位置（UTF-8 byte offset，保证 char boundary）。
    /// 与 `selection_anchor` 不同时表示有选中范围。
    cursor: usize,
    /// 选区固定端（UTF-8 byte offset）。拖选时不随光标移动。
    selection_anchor: usize,
    /// 下一个事务 ID（单调递增），每次 `take_transaction_id()` 消费。
    next_transaction_id: u64,
    /// 动画时长（毫秒），默认 160。仅影响后续事务，不改变已生成的事务。
    animation_duration_ms: u64,
    /// 单次动画最大 glyph 数，超过时降级为 RunAnimation 或 SnapshotAnimation。
    max_animated_chars: usize,
    /// 动画总开关。false 时所有编辑返回 `AnimationMode::SystemSuppressed`。
    animation_enabled: bool,
    /// 已执行命令栈。undo 从栈顶弹出并应用 inverse，同时 push 到 redo_stack。
    undo_stack: Vec<UndoEntry>,
    /// 已撤销命令栈。redo 从栈顶弹出并重新应用 forward，同时 push 回 undo_stack。
    /// 新编辑（push UndoEntry）清空 redo_stack（标准线性 undo 语义）。
    redo_stack: Vec<UndoEntry>,
    /// 当前活跃的 Composition 会话。同一时刻最多一个。
    /// 非活跃时为 None；BeginComposition 时创建，Finish/Cancel 时销毁。
    composition_session: Option<CompositionSessionState>,
    /// 下一个 Composition 会话 ID（单调递增）。
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
///
/// 坐标空间约定：
/// - `replace_start` / `replace_end_exclusive`：committed 正文坐标（UTF-8 byte offset，半开区间）
/// - `preedit_cursor_utf16`：preedit 内部坐标（UTF-16 code unit offset）
/// - 两者不可混用：committed 坐标不能用于定位 preedit 内部位置，反之亦然
#[derive(Debug, Clone)]
struct CompositionSessionState {
    /// 会话唯一 ID，由 `next_composition_session_id` 分配
    session_id: u64,
    /// 会话开始时的 committed revision，用于过期检测
    base_revision: u64,
    /// 会话 generation，每次 TextEditSession reset 递增。
    /// 过期 generation 的 UpdateComposition/FinishComposition/CancelComposition 被内核拒绝
    generation: u64,
    /// committed 正文替换范围起始（UTF-8 byte offset）
    replace_start: usize,
    /// committed 正文替换范围结束（不含，UTF-8 byte offset，半开区间 [replace_start, replace_end_exclusive)）
    replace_end_exclusive: usize,
    /// 当前预输入文本（UTF-8 纯文本）
    preedit_text: String,
    /// 预输入光标偏移（UTF-16 code unit 单位，preedit 内部坐标）。
    /// IME 协议以 UTF-16 报告光标位置，内核其余字段统一使用 UTF-8 byte offset。
    preedit_cursor_utf16: usize,
}

/// Undo 快照 — 存储编辑前后的全文快照与光标位置。
///
/// 每次编辑操作 push 一条 UndoEntry 到 undo_stack 并清空 redo_stack，
/// 保证 undo/redo 栈的互斥性：新编辑使 redo 历史作废。
///
/// 与 `history::command::TextEditCommand` 的区别：
///
/// - UndoEntry 存储全文快照（old_text/new_text），适用于 EditorKernel 的全文模型
/// - TextEditCommand 存储增量变更（forward/inverse EditorChange），适用于 HistoryManager 的增量模型
///
/// 两者独立维护，EditorKernel 使用 UndoEntry，HistoryManager 使用 TextEditCommand。
#[derive(Debug, Clone)]
struct UndoEntry {
    /// 编辑前的正文全文快照
    old_text: String,
    /// 编辑后的正文全文快照
    new_text: String,
    /// 编辑前的光标位置（UTF-8 byte offset，保证 char boundary）
    old_cursor: usize,
    /// 编辑后的光标位置（UTF-8 byte offset，保证 char boundary）
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

    /// 插入文本 — 在 byte_offset 处插入 text。
    ///
    /// 不变量：byte_offset 必须是合法 UTF-8 char boundary 且 ≤ text.len()，
    /// 否则返回 InvalidOffset。插入后光标移动到插入文本末尾，
    /// 同时清除活跃 composition 会话（非 composition 操作中断正在进行的 IME 输入）。
    /// 新编辑入 undo 栈并清空 redo 栈（标准线性 undo 语义）。
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

    /// 删除文本 — 删除半开区间 [byte_start, byte_end_exclusive) 的文本。
    ///
    /// 不变量：byte_start/byte_end_exclusive 必须是合法 char boundary，
    /// byte_start < byte_end_exclusive，且均在文本范围内。
    /// 删除后光标移动到删除起始位置，清除 composition 会话。
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
    /// 替换文本 — 用 replacement_text 替换半开区间 [byte_start, byte_end_exclusive) 的文本。
    ///
    /// 不变量：byte_start/byte_end_exclusive 必须是合法 char boundary 且在文本范围内。
    /// 替换后光标移动到替换文本末尾，清除 composition 会话。
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

    /// 设置选区 — 仅更新 anchor/head，不修改正文和 revision。
    ///
    /// 前置条件：`anchor_byte_offset`/`head_byte_offset` 必须是 char boundary 且 <= text.len()。
    /// 不满足时返回 `InvalidOffset`（含 NoChange result，base_revision 来自参数）。
    /// 后置条件：正文不变、revision 不变、返回 `NoChange` 变体。
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

    /// 撤销 — 从 undo 栈弹出最近一条 UndoEntry，恢复编辑前正文和光标。
    ///
    /// 不变量：undo 和 redo 互为逆操作。undo 弹出 undo 栈顶并 push 到 redo 栈，
    /// 使 redo 可以重新应用。undo 时清除活跃 composition 会话，
    /// 因为 composition 基于的正文已被撤销。
    /// undo/redo 始终使用 SnapshotAnimation（全文快照对比），不拆分细粒度动画。
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

    /// 重做 — 从 redo 栈弹出最近一条 UndoEntry，重新应用编辑后正文和光标。
    ///
    /// 不变量：redo 弹出 redo 栈顶并 push 回 undo 栈，使 undo 可以再次撤销。
    /// redo 时清除活跃 composition 会话。与 undo 一样使用 SnapshotAnimation。
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
    /// inserted_text 是替换后的新文本，基于 new_text 坐标。
    ///
    /// 算法：找最长公共前缀和最长公共后缀，中间部分即为差异区域。
    /// 前缀/后缀对齐到 char boundary（可能需要回退几个字节），
    /// 因为 DisplayPatch 的 replace_byte_range 必须在 char boundary 上。
    ///
    /// 不变量：返回的 replace_byte_range 和 inserted_text 满足：
    /// `old_text[replace_start..replace_end]` 被替换为 `inserted_text`
    /// 后得到 `new_text[prefix_len..new_text.len()-new_suffix_len]`。
    /// 当 old_text == new_text 时返回 `((0, 0), "")`。
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

    /// 全局替换 — 将正文中所有 `search` 替换为 `replacement`。
    ///
    /// 搜索为空或无匹配时返回 `NoChange`。
    /// 替换后光标重定位到 `clamp_to_char_boundary(cursor)`（保证 UTF-8 char boundary），
    /// 选区折叠为光标位置。undo 快照保存替换前的完整正文。
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

    /// 插入换行 — 在 `byte_offset` 处插入 `"\n{auto_indent_prefix}"`。
    ///
    /// 与普通 Insert 命令的区别：换行 + 自动缩进作为一个原子操作，
    /// 保证 undo 时一次性撤销换行和缩进。`auto_indent_prefix` 由平台端
    /// 根据上一行缩进计算，Core 不自行推断缩进。
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

    /// 构造过期会话结果 — expected_revision 不匹配时返回的空操作结果。
    ///
    /// 不修改正文和选区，display_patches 为空。base_revision 和 new_revision
    /// 均为当前 revision（未递增），平台端据此刷新本地状态并重试。
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

    /// 构造无变更结果。与 `stale_session_result` 的区别：
    /// - `noop_result` 的 `base_revision` 来自参数（调用方传入的预期版本）
    /// - `stale_session_result` 的 `base_revision` 来自 `self.revision`（内核当前版本）
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

    /// 从 EditorChange 列表提取受影响的 UTF-8 byte range 列表。
    ///
    /// Delete 变更的 range [index, index+text.len()) 记入 old_ranges（旧文本中被删除的范围），
    /// Insert 变更的 range [index, index+text.len()) 记入 new_ranges（新文本中插入的范围）。
    /// 所有 range 均为半开区间，基于变更后的文本坐标。
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

    /// 消费并返回下一个事务 ID。ID 单调递增（saturating_add 防溢出），
    /// 即使编辑无实际变更也分配新 ID，保证平台端能区分不同操作。
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
    ///
    /// 不变量：加载时清空 undo/redo 栈（新章节不应保留旧章节的撤销历史），
    /// 清除活跃 composition 会话，光标对齐到最近 char boundary。
    /// revision 递增（load 也是一次正文变更，需要新 revision）。
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

    /// IME composition commit — 将预输入文本写入正文，替换 [byte_start, byte_end_exclusive)。
    ///
    /// 会话验证：必须匹配活跃会话的 session_id、base_revision 和 generation，
    /// 否则返回 StaleRevision（会话可能已被 TextEditSession reset 使过期）。
    /// 范围验证：byte_start/byte_end_exclusive 必须与会话的 replace_start/replace_end_exclusive
    /// 一致，否则返回 InvalidRange（commit 不能改变 composition 的替换范围）。
    ///
    /// 光标坐标转换：resulting_selection_anchor/head 由平台以 UTF-8 byte offset 传入，
    /// 内核自动对齐到 char boundary。若对齐后值与传入值不同，返回 AppliedWithAdjustedSelection。
    ///
    /// 确认后 composition 会话销毁，undo/redo 栈正常记录。
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
    ///
    /// 选区调整逻辑：删除 before 区域后，选区 anchor/head 需要向前移动
    /// before_deleted_len 个字节。删除 after 区域不影响选区 offset
    /// （after 在选区右侧，删除后选区左侧的 offset 不变）。
    /// 最终选区 = (原选区位置 - before_deleted_len)。
    #[allow(clippy::too_many_arguments)]
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

    /// 开始 IME composition 会话 — 在正文 `[replace_start, replace_end_exclusive)` 半开区间
    /// 上建立 IME 预输入区域。
    ///
    /// 前置条件：无活跃会话、range 合法（start <= end、char boundary、<= text.len()）。
    /// 不满足时返回 InvalidRange/InvalidOffset。
    /// 后置条件：创建 CompositionSessionState，正文不变，返回 Applied（含空 patches）。
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
    /// 前置条件：必须匹配活跃会话的 session_id、generation 和 base_revision，
    /// 否则返回 StaleRevision（会话可能已被 TextEditSession reset 使过期）。
    /// 后置条件：更新 preedit_text 和 preedit_cursor_utf16，generation 递增，
    /// 正文不变，display_patches 为空（preedit 是纯视觉层，不修改 committed 正文）。
    /// `new_preedit_cursor_offset` 为 UTF-16 code unit 单位（IME 协议语义），
    /// 内核其余字段统一使用 UTF-8 byte offset。
    #[allow(clippy::too_many_arguments)]
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
    /// 前置条件：必须匹配活跃会话的 session_id、generation 和 base_revision，
    /// 否则返回 StaleRevision。
    /// 后置条件：会话的 preedit 文本替换 [replace_start, replace_end_exclusive) 范围，
    /// 会话销毁，undo/redo 栈正常记录。
    /// 光标位置由 preedit_cursor_utf16（UTF-16 code unit）转换为 UTF-8 byte offset
    /// （见下方 UTF-16→UTF-8 转换逻辑）。空预输入文本时仅关闭会话，不修改正文。
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

        // UTF-16 → UTF-8 光标坐标转换：
        // IME 协议（Android InputConnection / Qt QInputMethodEvent）以 UTF-16 code unit
        // 报告 preedit 光标位置。此处逐字符遍历 committed_text，累加 UTF-16 code unit
        // 直到达到 preedit_cursor_utf16_clamped，同时累加 UTF-8 byte offset。
        // 最终光标 = replace_start + byte_offset（committed 正文坐标）。
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
    /// 前置条件：必须匹配活跃会话的 session_id、generation 和 base_revision，
    /// 否则返回 StaleRevision。
    /// 后置条件：不修改正文（preedit 文本从未写入正文），仅销毁会话。
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

    /// 返回当前 composition 会话的三元组 (session_id, base_revision, generation)。
    /// 无活跃会话时返回 None。平台端用此信息构造 UpdateComposition/FinishComposition
    /// 命令的 composition_session_id / composition_base_revision / composition_generation 参数。
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