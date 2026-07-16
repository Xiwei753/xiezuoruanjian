//! #535: Rust EditorKernel — 正文和业务唯一真相。
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

/// #535: 编辑命令 — 平台输入适配器翻译系统事件后的标准化命令。
///
/// 所有平台输入（IME、键盘、触摸选择）都翻译成 EditorCommand，
/// 交给 EditorKernel.apply() 处理。平台不能再维护第二份可独立编辑的正文真相。
///
/// range 单位统一为 UTF-8 byte boundary。
/// Android/Windows 的 UTF-16、Qt 的 QChar index 只允许存在于平台 TextIndexMap 内。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum EditorCommand {
    /// 在指定位置插入文本
    Insert {
        /// 插入位置（UTF-8 byte offset）
        byte_offset: usize,
        /// 插入的文本
        text: String,
        /// 编辑原因
        cause: EditorTransactionCause,
    },
    /// 删除指定范围的文本
    Delete {
        /// 删除范围起始（UTF-8 byte offset）
        byte_start: usize,
        /// 删除范围结束（UTF-8 byte offset，不含）
        byte_end_exclusive: usize,
        /// 被删除的文本（用于 undo）
        deleted_text: String,
        /// 编辑原因
        cause: EditorTransactionCause,
    },
    /// 替换指定范围的文本
    Replace {
        /// 替换范围起始（UTF-8 byte offset）
        byte_start: usize,
        /// 替换范围结束（UTF-8 byte offset，不含）
        byte_end_exclusive: usize,
        /// 替换后的文本
        replacement_text: String,
        /// 被替换的原文（用于 undo）
        original_text: String,
        /// 编辑原因
        cause: EditorTransactionCause,
    },
    /// 设置选区
    SetSelection {
        /// 选区锚点（UTF-8 byte offset）
        anchor_byte_offset: usize,
        /// 选区头部（UTF-8 byte offset）
        head_byte_offset: usize,
    },
    /// 撤销
    Undo,
    /// 重做
    Redo,
}

/// #535: 显示补丁 — 平台显示镜像唯一允许消费的正文变化。
///
/// DisplayTextMirror 按 DisplayPatch 增量更新 SpannableStringBuilder，
/// 不得根据 old/new 全文重新 diff，也不得先本地改 Buffer 再通知 Core。
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

/// #535: 视觉意图 — Core 告诉平台层应该做什么动画。
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

/// #535: 操作类型 — 区分不同编辑操作的视觉语义
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

/// #535: 光标协同 — 描述光标移动的视觉语义
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

/// #535: 编辑结果 — EditorKernel.apply() 的返回值。
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

/// #535: EditorKernel — 正文和业务唯一真相。
///
/// 平台不能再维护第二份可独立编辑的正文真相。
/// 平台只持有与 Rust revision 对应的显示镜像。
#[derive(Debug, Clone)]
pub struct EditorKernel {
    /// 当前正文
    text: String,
    /// 当前 revision ID（递增）
    revision: u64,
    /// 当前光标位置（UTF-8 byte offset）
    cursor: usize,
    /// 当前选区锚点（UTF-8 byte offset）
    selection_anchor: usize,
    /// 下一个事务 ID
    next_transaction_id: u64,
    /// 动画时长（毫秒）
    animation_duration_ms: u64,
    /// 最大动画字符数
    max_animated_chars: usize,
    /// 动画是否启用
    animation_enabled: bool,
    /// undo 栈
    undo_stack: Vec<UndoEntry>,
    /// redo 栈
    redo_stack: Vec<UndoEntry>,
}

/// undo 条目
#[derive(Debug, Clone)]
struct UndoEntry {
    old_text: String,
    new_text: String,
    old_cursor: usize,
    new_cursor: usize,
    cause: EditorTransactionCause,
}

impl EditorKernel {
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
        }
    }

    pub fn with_text(text: String, cursor: usize) -> Self {
        let cursor = cursor.min(text.len());
        Self {
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
        }
    }

    pub fn set_animation_duration_ms(&mut self, duration_ms: u64) {
        self.animation_duration_ms = duration_ms;
    }

    pub fn set_animation_enabled(&mut self, enabled: bool) {
        self.animation_enabled = enabled;
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// #535: 应用编辑命令 — 唯一正文修改入口。
    ///
    /// 平台输入适配器只调用此方法，不能直接修改正文。
    /// 返回 EditorEditResult，包含 display_patches 和 visual_intent。
    pub fn apply(&mut self, command: EditorCommand) -> EditorEditResult {
        let base_revision = self.revision;
        let old_cursor = self.cursor;
        let old_selection = (self.selection_anchor, self.cursor);

        match command {
            EditorCommand::Insert { byte_offset, text, cause } => {
                self.apply_insert(byte_offset, &text, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::Delete { byte_start, byte_end_exclusive, deleted_text: _, cause } => {
                self.apply_delete(byte_start, byte_end_exclusive, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::Replace { byte_start, byte_end_exclusive, replacement_text, original_text: _, cause } => {
                self.apply_replace(byte_start, byte_end_exclusive, &replacement_text, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::SetSelection { anchor_byte_offset, head_byte_offset } => {
                self.apply_set_selection(anchor_byte_offset, head_byte_offset, base_revision, old_cursor, old_selection)
            }
            EditorCommand::Undo => {
                self.apply_undo(base_revision, old_cursor, old_selection)
            }
            EditorCommand::Redo => {
                self.apply_redo(base_revision, old_cursor, old_selection)
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
    ) -> EditorEditResult {
        let byte_offset = byte_offset.min(self.text.len());
        let old_text = self.text.clone();

        self.text.insert_str(byte_offset, text);
        self.revision = self.revision.saturating_add(1);
        self.cursor = byte_offset + text.len();
        self.selection_anchor = self.cursor;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
            cause,
        });
        self.redo_stack.clear();

        let new_selection = (self.cursor, self.cursor);
        let (operation_kind, old_affected, new_affected) = (
            EditorOperationKind::Insert,
            vec![],
            vec![(byte_offset, byte_offset + text.len())],
        );

        self.build_edit_result(
            base_revision, &old_text, cause, old_selection, new_selection,
            operation_kind, old_affected, new_affected, old_cursor,
        )
    }

    fn apply_delete(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditResult {
        let byte_start = byte_start.min(self.text.len());
        let byte_end_exclusive = byte_end_exclusive.min(self.text.len());
        if byte_start >= byte_end_exclusive {
            return self.noop_result(base_revision, old_cursor, old_selection);
        }

        let old_text = self.text.clone();
        let _deleted = self.text[byte_start..byte_end_exclusive].to_string();

        self.text.replace_range(byte_start..byte_end_exclusive, "");
        self.revision = self.revision.saturating_add(1);
        self.cursor = byte_start;
        self.selection_anchor = self.cursor;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
            cause,
        });
        self.redo_stack.clear();

        let new_selection = (self.cursor, self.cursor);
        let (operation_kind, old_affected, new_affected) = (
            EditorOperationKind::Delete,
            vec![(byte_start, byte_end_exclusive)],
            vec![],
        );

        self.build_edit_result(
            base_revision, &old_text, cause, old_selection, new_selection,
            operation_kind, old_affected, new_affected, old_cursor,
        )
    }

    fn apply_replace(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement_text: &str,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditResult {
        let byte_start = byte_start.min(self.text.len());
        let byte_end_exclusive = byte_end_exclusive.min(self.text.len());

        let old_text = self.text.clone();

        self.text.replace_range(byte_start..byte_end_exclusive, replacement_text);
        self.revision = self.revision.saturating_add(1);
        self.cursor = byte_start + replacement_text.len();
        self.selection_anchor = self.cursor;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
            cause,
        });
        self.redo_stack.clear();

        let new_selection = (self.cursor, self.cursor);
        let (operation_kind, old_affected, new_affected) = (
            EditorOperationKind::Replace,
            vec![(byte_start, byte_end_exclusive)],
            vec![(byte_start, byte_start + replacement_text.len())],
        );

        self.build_edit_result(
            base_revision, &old_text, cause, old_selection, new_selection,
            operation_kind, old_affected, new_affected, old_cursor,
        )
    }

    fn apply_set_selection(
        &mut self,
        anchor_byte_offset: usize,
        head_byte_offset: usize,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditResult {
        self.selection_anchor = anchor_byte_offset.min(self.text.len());
        self.cursor = head_byte_offset.min(self.text.len());

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

        EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        }
    }

    fn apply_undo(
        &mut self,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditResult {
        let entry = match self.undo_stack.pop() {
            Some(e) => e,
            None => return self.noop_result(base_revision, old_cursor, old_selection),
        };

        let old_text = self.text.clone();
        self.text = entry.old_text.clone();
        self.cursor = entry.old_cursor;
        self.selection_anchor = self.cursor;
        self.revision = self.revision.saturating_add(1);

        self.redo_stack.push(entry);

        let new_selection = (self.cursor, self.cursor);

        let changes = diff_plain_text(&old_text, &self.text);
        let (old_affected, new_affected) = Self::affected_ranges_from_changes(&changes);

        self.build_edit_result(
            base_revision, &old_text, EditorTransactionCause::Undo,
            old_selection, new_selection, EditorOperationKind::Replace,
            old_affected, new_affected, old_cursor,
        )
    }

    fn apply_redo(
        &mut self,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditResult {
        let entry = match self.redo_stack.pop() {
            Some(e) => e,
            None => return self.noop_result(base_revision, old_cursor, old_selection),
        };

        let old_text = self.text.clone();
        self.text = entry.new_text.clone();
        self.cursor = entry.new_cursor;
        self.selection_anchor = self.cursor;
        self.revision = self.revision.saturating_add(1);

        self.undo_stack.push(entry);

        let new_selection = (self.cursor, self.cursor);

        let changes = diff_plain_text(&old_text, &self.text);
        let (old_affected, new_affected) = Self::affected_ranges_from_changes(&changes);

        self.build_edit_result(
            base_revision, &old_text, EditorTransactionCause::Redo,
            old_selection, new_selection, EditorOperationKind::Replace,
            old_affected, new_affected, old_cursor,
        )
    }

    fn build_edit_result(
        &mut self,
        base_revision: u64,
        old_text: &str,
        cause: EditorTransactionCause,
        old_selection: (usize, usize),
        new_selection: (usize, usize),
        operation_kind: EditorOperationKind,
        old_affected: Vec<(usize, usize)>,
        new_affected: Vec<(usize, usize)>,
        old_cursor: usize,
    ) -> EditorEditResult {
        let new_revision = self.revision;

        let display_patches = if old_text != self.text {
            let changes = diff_plain_text(old_text, &self.text);
            changes.into_iter().map(|c| {
                let (range, inserted) = match &c {
                    EditorChange::Insert { index, text } => ((*index, *index), text.clone()),
                    EditorChange::Delete { index, text } => ((*index, *index + text.len()), String::new()),
                };
                DisplayPatch {
                    base_revision,
                    new_revision,
                    replace_byte_range: range,
                    inserted_text: inserted,
                    resulting_selection_byte_range: new_selection,
                }
            }).collect()
        } else {
            vec![]
        };

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;
        let is_ime = cause == EditorTransactionCause::ImeComposition;

        let animation_mode = if !self.animation_enabled || is_loading || is_format || is_ime {
            AnimationMode::SystemSuppressed
        } else {
            let diff_text = if !new_affected.is_empty() {
                &self.text[new_affected[0].0..new_affected.last().map(|r| r.1).unwrap_or(new_affected[0].1)]
            } else if !old_affected.is_empty() {
                ""
            } else {
                ""
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
            operation_kind,
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

        EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
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

    /// 加载文本（章节打开时调用）
    pub fn load_text(&mut self, text: String, cursor: usize) -> EditorEditResult {
        let base_revision = self.revision;
        let old_cursor = self.cursor;
        let old_selection = (self.selection_anchor, self.cursor);

        let old_text = self.text.clone();
        self.text = text;
        self.cursor = cursor.min(self.text.len());
        self.selection_anchor = self.cursor;
        self.revision = self.revision.saturating_add(1);
        self.undo_stack.clear();
        self.redo_stack.clear();

        let new_selection = (self.cursor, self.cursor);

        let changes = diff_plain_text(&old_text, &self.text);
        let (old_affected, new_affected) = Self::affected_ranges_from_changes(&changes);

        self.build_edit_result(
            base_revision, &old_text, EditorTransactionCause::Load,
            old_selection, new_selection, EditorOperationKind::Load,
            old_affected, new_affected, old_cursor,
        )
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
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6);
        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 6,
            text: "世界".to_string(),
            cause: EditorTransactionCause::Typing,
        });

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
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12);
        let result = kernel.apply(EditorCommand::Delete {
            byte_start: 6,
            byte_end_exclusive: 12,
            deleted_text: "世界".to_string(),
            cause: EditorTransactionCause::Delete,
        });

        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Delete);
    }

    #[test]
    fn replace_command_produces_display_patch() {
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12);
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 6,
            byte_end_exclusive: 12,
            replacement_text: "朋友".to_string(),
            original_text: "世界".to_string(),
            cause: EditorTransactionCause::Typing,
        });

        assert_eq!(kernel.text(), "你好朋友");
        assert_eq!(kernel.cursor(), 12);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Replace);
    }

    #[test]
    fn set_selection_does_not_modify_text() {
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5);
        let result = kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 0,
            head_byte_offset: 3,
        });

        assert_eq!(kernel.text(), "hello");
        assert_eq!(kernel.cursor(), 3);
        assert_eq!(result.display_patches.len(), 0);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::CursorOnly);
    }

    #[test]
    fn undo_restores_previous_state() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2);
        kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        assert_eq!(kernel.text(), "abc");

        let result = kernel.apply(EditorCommand::Undo);
        assert_eq!(kernel.text(), "ab");
        assert_eq!(kernel.cursor(), 2);
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::Undo);
    }

    #[test]
    fn redo_restores_undone_state() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2);
        kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        kernel.apply(EditorCommand::Undo);
        assert_eq!(kernel.text(), "ab");

        let result = kernel.apply(EditorCommand::Redo);
        assert_eq!(kernel.text(), "abc");
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::Redo);
    }

    #[test]
    fn load_text_resets_state() {
        let mut kernel = EditorKernel::with_text("old".to_string(), 3);
        kernel.apply(EditorCommand::Insert {
            byte_offset: 3,
            text: " text".to_string(),
            cause: EditorTransactionCause::Typing,
        });

        let result = kernel.load_text("new content".to_string(), 0);
        assert_eq!(kernel.text(), "new content");
        assert_eq!(kernel.cursor(), 0);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Load);
        assert_eq!(result.visual_intent.animation_mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn revision_increments_on_each_edit() {
        let mut kernel = EditorKernel::new();
        assert_eq!(kernel.revision(), 0);

        kernel.apply(EditorCommand::Insert {
            byte_offset: 0,
            text: "a".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        assert_eq!(kernel.revision(), 1);

        kernel.apply(EditorCommand::Insert {
            byte_offset: 1,
            text: "b".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        assert_eq!(kernel.revision(), 2);
    }

    #[test]
    fn display_patch_contains_correct_ranges() {
        let mut kernel = EditorKernel::with_text("hello world".to_string(), 11);
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 6,
            byte_end_exclusive: 11,
            replacement_text: "rust".to_string(),
            original_text: "world".to_string(),
            cause: EditorTransactionCause::Typing,
        });

        assert_eq!(result.display_patches.len(), 2);
        assert_eq!(result.display_patches[0].replace_byte_range, (6, 11));
        assert_eq!(result.display_patches[0].inserted_text, "");
        assert_eq!(result.display_patches[1].replace_byte_range, (6, 6));
        assert_eq!(result.display_patches[1].inserted_text, "rust");
    }

    #[test]
    fn coordinated_cursor_tracks_movement() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3);
        let result = kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 0,
            head_byte_offset: 0,
        });

        assert_eq!(result.visual_intent.coordinated_cursor.old_byte_offset, 3);
        assert_eq!(result.visual_intent.coordinated_cursor.new_byte_offset, 0);
        assert!(result.visual_intent.coordinated_cursor.should_animate);
    }

    #[test]
    fn animation_disabled_suppresses_animation() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2);
        kernel.set_animation_enabled(false);

        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
        });

        assert_eq!(result.visual_intent.animation_mode, AnimationMode::SystemSuppressed);
        assert!(!result.visual_intent.coordinated_cursor.should_animate);
    }

    #[test]
    fn undo_then_new_edit_clears_redo() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2);
        kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        kernel.apply(EditorCommand::Undo);
        assert_eq!(kernel.text(), "ab");

        kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "d".to_string(),
            cause: EditorTransactionCause::Typing,
        });

        let result = kernel.apply(EditorCommand::Redo);
        assert_eq!(kernel.text(), "abd");
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::CursorOnly);
    }

    #[test]
    fn edit_result_serializes_camel_case() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2);
        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
        });

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
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6);
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
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6);
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 6,
            byte_end_exclusive: 6,
            replacement_text: "你好".to_string(),
            original_text: String::new(),
            cause: EditorTransactionCause::TypingCommit,
        });
        assert_eq!(kernel.text(), "你好你好");
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Replace);
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::TypingCommit);
    }

    #[test]
    fn delete_empty_range_is_noop() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3);
        let result = kernel.apply(EditorCommand::Delete {
            byte_start: 2,
            byte_end_exclusive: 2,
            deleted_text: String::new(),
            cause: EditorTransactionCause::Delete,
        });
        assert_eq!(kernel.text(), "abc");
        assert_eq!(result.display_patches.len(), 0);
    }

    #[test]
    fn insert_at_boundary_clamps() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2);
        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 100,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        assert_eq!(kernel.text(), "abc");
        assert_eq!(kernel.cursor(), 3);
    }

    #[test]
    fn replace_same_text_produces_patch() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3);
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 1,
            byte_end_exclusive: 2,
            replacement_text: "X".to_string(),
            original_text: "b".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        assert_eq!(kernel.text(), "aXc");
        assert!(!result.display_patches.is_empty());
    }

    #[test]
    fn undo_after_multiple_edits_restores_correctly() {
        let mut kernel = EditorKernel::new();
        kernel.apply(EditorCommand::Insert {
            byte_offset: 0,
            text: "a".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        kernel.apply(EditorCommand::Insert {
            byte_offset: 1,
            text: "b".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        assert_eq!(kernel.text(), "abc");

        kernel.apply(EditorCommand::Undo);
        assert_eq!(kernel.text(), "ab");

        kernel.apply(EditorCommand::Undo);
        assert_eq!(kernel.text(), "a");

        kernel.apply(EditorCommand::Undo);
        assert_eq!(kernel.text(), "");
    }

    #[test]
    fn cjk_insert_and_delete() {
        let mut kernel = EditorKernel::new();
        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 0,
            text: "你好世界".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);

        let result = kernel.apply(EditorCommand::Delete {
            byte_start: 6,
            byte_end_exclusive: 12,
            deleted_text: "世界".to_string(),
            cause: EditorTransactionCause::Delete,
        });
        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::Delete);
    }

    #[test]
    fn set_selection_with_same_position_no_cursor_animation() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 1);
        let result = kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 1,
            head_byte_offset: 1,
        });
        assert!(!result.visual_intent.coordinated_cursor.should_animate);
    }

    #[test]
    fn load_text_clears_undo_stack() {
        let mut kernel = EditorKernel::with_text("old".to_string(), 3);
        kernel.apply(EditorCommand::Insert {
            byte_offset: 3,
            text: " text".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        kernel.load_text("new".to_string(), 3);
        let result = kernel.apply(EditorCommand::Undo);
        assert_eq!(kernel.text(), "new");
    }

    #[test]
    fn multiple_display_patches_for_replace() {
        let mut kernel = EditorKernel::with_text("hello world".to_string(), 11);
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 6,
            byte_end_exclusive: 11,
            replacement_text: "rust".to_string(),
            original_text: "world".to_string(),
            cause: EditorTransactionCause::Typing,
        });
        assert!(result.display_patches.len() >= 1);
        let total_deleted: usize = result.display_patches.iter()
            .filter(|p| p.inserted_text.is_empty())
            .map(|p| p.replace_byte_range.1 - p.replace_byte_range.0)
            .sum();
        assert_eq!(total_deleted, 5);
    }
}
