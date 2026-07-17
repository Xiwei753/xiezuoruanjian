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

    pub fn selection_anchor(&self) -> usize {
        self.selection_anchor
    }

    pub fn selection(&self) -> (usize, usize) {
        (self.selection_anchor, self.cursor)
    }

    /// #535: 应用编辑命令 — 唯一正文修改入口。
    ///
    /// 平台输入适配器只调用此方法，不能直接修改正文。
    /// 返回 EditorEditResult，包含 display_patches 和 visual_intent。
    pub fn apply(&mut self, command: EditorCommand) -> EditorEditResult {
        let base_revision = self.revision;

        match &command {
            EditorCommand::Insert { expected_revision, .. }
            | EditorCommand::Delete { expected_revision, .. }
            | EditorCommand::Replace { expected_revision, .. }
             | EditorCommand::SetSelection { expected_revision, .. }
             | EditorCommand::ReplaceAll { expected_revision, .. }
             | EditorCommand::InsertLineBreak { expected_revision, .. }
             | EditorCommand::Undo { expected_revision }
             | EditorCommand::Redo { expected_revision } => {
                if *expected_revision != base_revision {
                    return self.stale_session_result(base_revision);
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

        self.text.insert_str(byte_offset, text);
        self.revision = self.revision.saturating_add(1);
        self.cursor = byte_offset + text.len();
        self.selection_anchor = self.cursor;

        self.undo_stack.push(UndoEntry {
            old_text: self.text[..byte_offset].to_string() + &self.text[byte_offset + text.len()..],
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
            cause,
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
        let is_ime = cause == EditorTransactionCause::ImeComposition;

        let animation_mode = if !self.animation_enabled || is_loading || is_format || is_ime {
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

    fn apply_delete(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditResult {
        let (byte_start, byte_end_exclusive) = if byte_start > byte_end_exclusive {
            (byte_end_exclusive, byte_start)
        } else {
            (byte_start, byte_end_exclusive)
        };
        let byte_start = byte_start.min(self.text.len());
        let byte_end_exclusive = byte_end_exclusive.min(self.text.len());
        if byte_start >= byte_end_exclusive {
            return self.noop_result(base_revision, old_cursor, old_selection);
        }

        let old_text = self.text.clone();

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
        let is_ime = cause == EditorTransactionCause::ImeComposition;

        let animation_mode = if !self.animation_enabled || is_loading || is_format || is_ime {
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
        let (byte_start, byte_end_exclusive) = if byte_start > byte_end_exclusive {
            (byte_end_exclusive, byte_start)
        } else {
            (byte_start, byte_end_exclusive)
        };
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
        let is_ime = cause == EditorTransactionCause::ImeComposition;

        let animation_mode = if !self.animation_enabled || is_loading || is_format || is_ime {
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

    fn compute_single_patch(old_text: &str, new_text: &str) -> ((usize, usize), String) {
        if old_text == new_text {
            return ((0, 0), String::new());
        }

        let old_bytes = old_text.as_bytes();
        let new_bytes = new_text.as_bytes();

        let mut prefix_len = 0;
        while prefix_len < old_bytes.len() && prefix_len < new_bytes.len() && old_bytes[prefix_len] == new_bytes[prefix_len] {
            prefix_len += 1;
        }
        while prefix_len > 0 && !old_text.is_char_boundary(prefix_len) {
            prefix_len -= 1;
        }
        while prefix_len > 0 && !new_text.is_char_boundary(prefix_len) {
            prefix_len -= 1;
        }

        let mut suffix_len = 0;
        while suffix_len < old_bytes.len() - prefix_len && suffix_len < new_bytes.len() - prefix_len
            && old_bytes[old_bytes.len() - 1 - suffix_len] == new_bytes[new_bytes.len() - 1 - suffix_len]
        {
            suffix_len += 1;
        }
        while suffix_len > 0 && !old_text.is_char_boundary(old_bytes.len() - suffix_len) {
            suffix_len -= 1;
        }
        while suffix_len > 0 && !new_text.is_char_boundary(new_bytes.len() - suffix_len) {
            suffix_len -= 1;
        }

        let replace_start = prefix_len;
        let replace_end = old_bytes.len() - suffix_len;
        let inserted_text = new_text[prefix_len..new_bytes.len() - suffix_len].to_string();

        ((replace_start, replace_end), inserted_text)
    }

    fn apply_replace_all(
        &mut self,
        search: &str,
        replacement: &str,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditResult {
        let old_text = self.text.clone();
        let new_text = old_text.replace(search, replacement);

        if new_text == old_text {
            return self.noop_result(base_revision, old_cursor, old_selection);
        }

        self.text = new_text;
        self.revision = self.revision.saturating_add(1);
        self.cursor = self.cursor.min(self.text.len());
        self.selection_anchor = self.cursor;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
            cause: EditorTransactionCause::Format,
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

    fn apply_insert_line_break(
        &mut self,
        byte_offset: usize,
        auto_indent_prefix: String,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditResult {
        let byte_offset = byte_offset.min(self.text.len());
        let text = format!("\n{}", auto_indent_prefix);

        self.text.insert_str(byte_offset, &text);
        self.revision = self.revision.saturating_add(1);
        self.cursor = byte_offset + text.len();
        self.selection_anchor = self.cursor;

        self.undo_stack.push(UndoEntry {
            old_text: self.text[..byte_offset].to_string() + &self.text[byte_offset + text.len()..],
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: self.cursor,
            cause: cause.clone(),
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

    fn stale_session_result(
        &mut self,
        base_revision: u64,
    ) -> EditorEditResult {
        EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: (self.selection_anchor, self.cursor),
            new_selection_byte_range: (self.selection_anchor, self.cursor),
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
    ///
    /// 始终生成完整 replacement patch，即使正文相同。
    /// 平台 Mirror 通过 loadFromSessionSnapshot 重建，不依赖增量 patch。
    pub fn load_text(&mut self, text: String, cursor: usize) -> EditorEditResult {
        let base_revision = self.revision;
        let old_cursor = self.cursor;
        let old_selection = (self.selection_anchor, self.cursor);

        let old_text = self.text.clone();
        self.text = text;
        let cursor = cursor.min(self.text.len());
        self.cursor = cursor;
        self.selection_anchor = cursor;
        self.revision = self.revision.saturating_add(1);
        self.undo_stack.clear();
        self.redo_stack.clear();

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

    /// #535: 获取 CompositionUpdate 的 VisualIntent。
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
        let replace_end = composition_replace_range
            .map(|(_, e)| e)
            .unwrap_or(self.cursor);
        let new_end = replace_start + new_preedit_text.len();

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
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: self.cursor,
                new_byte_offset: new_end,
                should_animate: false,
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
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6);
        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 6,
            text: "世界".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
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
            expected_revision: 0,
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
            expected_revision: 0,
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
            expected_revision: 0,
        });

        assert_eq!(kernel.text(), "hello");
        assert_eq!(kernel.cursor(), 3);
        assert_eq!(result.display_patches.len(), 0);
        assert_eq!(result.visual_intent.operation_kind, EditorOperationKind::CursorOnly);
    }

    #[test]
    fn undo_restores_previous_state() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2);
        let result = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        });
        assert_eq!(kernel.text(), "abc");

        let result = kernel.apply(EditorCommand::Undo { expected_revision: result.new_revision });
        assert_eq!(kernel.text(), "ab");
        assert_eq!(kernel.cursor(), 2);
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::Undo);
    }

    #[test]
    fn redo_restores_undone_state() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2);
        let r1 = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        });
        let r2 = kernel.apply(EditorCommand::Undo { expected_revision: r1.new_revision });
        assert_eq!(kernel.text(), "ab");

        let result = kernel.apply(EditorCommand::Redo { expected_revision: r2.new_revision });
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
            expected_revision: 0,
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

        let r1 = kernel.apply(EditorCommand::Insert {
            byte_offset: 0,
            text: "a".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        });
        assert_eq!(kernel.revision(), 1);

        kernel.apply(EditorCommand::Insert {
            byte_offset: 1,
            text: "b".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: r1.new_revision,
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
            expected_revision: 0,
        });

        assert_eq!(result.display_patches.len(), 1);
        assert_eq!(result.display_patches[0].replace_byte_range, (6, 11));
        assert_eq!(result.display_patches[0].inserted_text, "rust");
    }

    #[test]
    fn coordinated_cursor_tracks_movement() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3);
        let result = kernel.apply(EditorCommand::SetSelection {
            anchor_byte_offset: 0,
            head_byte_offset: 0,
            expected_revision: 0,
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
            expected_revision: 0,
        });

        assert_eq!(result.visual_intent.animation_mode, AnimationMode::SystemSuppressed);
        assert!(!result.visual_intent.coordinated_cursor.should_animate);
    }

    #[test]
    fn undo_then_new_edit_clears_redo() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2);
        let r1 = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        });
        let r2 = kernel.apply(EditorCommand::Undo { expected_revision: r1.new_revision });
        assert_eq!(kernel.text(), "ab");

        let r3 = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "d".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: r2.new_revision,
        });

        let result = kernel.apply(EditorCommand::Redo { expected_revision: r3.new_revision });
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
            expected_revision: 0,
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
            expected_revision: 0,
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
            expected_revision: 0,
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
            expected_revision: 0,
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
            expected_revision: 0,
        });
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
        });
        let r2 = kernel.apply(EditorCommand::Insert {
            byte_offset: 1,
            text: "b".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: r1.new_revision,
        });
        let r3 = kernel.apply(EditorCommand::Insert {
            byte_offset: 2,
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: r2.new_revision,
        });
        assert_eq!(kernel.text(), "abc");

        let r4 = kernel.apply(EditorCommand::Undo { expected_revision: r3.new_revision });
        assert_eq!(kernel.text(), "ab");

        let r5 = kernel.apply(EditorCommand::Undo { expected_revision: r4.new_revision });
        assert_eq!(kernel.text(), "a");

        kernel.apply(EditorCommand::Undo { expected_revision: r5.new_revision });
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
        });
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);

        let result = kernel.apply(EditorCommand::Delete {
            byte_start: 6,
            byte_end_exclusive: 12,
            deleted_text: "世界".to_string(),
            cause: EditorTransactionCause::Delete,
            expected_revision: result.new_revision,
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
            expected_revision: 0,
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
            expected_revision: 0,
        });
        kernel.load_text("new".to_string(), 3);
        let result = kernel.apply(EditorCommand::Undo { expected_revision: 0 });
        assert_eq!(kernel.text(), "new");
    }

    #[test]
    fn atomic_display_patch_for_replace() {
        let mut kernel = EditorKernel::with_text("hello world".to_string(), 11);
        let result = kernel.apply(EditorCommand::Replace {
            byte_start: 6,
            byte_end_exclusive: 11,
            replacement_text: "rust".to_string(),
            original_text: "world".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: 0,
        });
        assert_eq!(result.display_patches.len(), 1);
        let patch = &result.display_patches[0];
        assert_eq!(patch.replace_byte_range, (6, 11));
        assert_eq!(patch.inserted_text, "rust");
    }

    #[test]
    fn composition_update_visual_intent_returns_correct_intent() {
        let kernel = EditorKernel::with_text("你好".to_string(), 6);
        let intent = kernel.composition_update_visual_intent(
            None,
            "",
            "nihao",
        );
        assert_eq!(intent.cause, EditorTransactionCause::ImeComposition);
        assert_eq!(intent.operation_kind, EditorOperationKind::CompositionUpdate);
        assert_eq!(intent.animation_mode, AnimationMode::SystemSuppressed);
        assert_eq!(intent.duration_ms, 0);
        assert!(!intent.coordinated_cursor.should_animate);
        assert!(!intent.new_affected_byte_ranges.is_empty());
    }

    #[test]
    fn composition_update_visual_intent_with_replace_range() {
        let kernel = EditorKernel::with_text("你好世界".to_string(), 12);
        let intent = kernel.composition_update_visual_intent(
            Some((6, 12)),
            "世界",
            "朋友",
        );
        assert_eq!(intent.operation_kind, EditorOperationKind::CompositionUpdate);
        assert_eq!(intent.old_affected_byte_ranges, vec![(6, 12)]);
        assert_eq!(intent.new_affected_byte_ranges, vec![(6, 12)]);
    }
}
