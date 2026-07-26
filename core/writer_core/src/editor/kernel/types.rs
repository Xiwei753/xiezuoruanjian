use serde::{Deserialize, Serialize};

use crate::editor::transaction::{AnimationMode, EditorTransactionCause};

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
