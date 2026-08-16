use serde::{Deserialize, Serialize};

use crate::editor::strong_types::{
    EditorRevision, EditorSessionGeneration, EditorSessionId, Utf16CodeUnitOffset, Utf8ByteOffset,
    Utf8ByteRange,
};
use crate::editor::transaction::{AnimationMode, EditorTransactionCause, OffsetMap};

pub enum EditorCommand {
    Insert {
        byte_offset: Utf8ByteOffset,
        text: String,
        cause: EditorTransactionCause,
        expected_revision: EditorRevision,
    },
    Delete {
        byte_range: Utf8ByteRange,
        deleted_text: String,
        cause: EditorTransactionCause,
        expected_revision: EditorRevision,
    },
    Replace {
        byte_range: Utf8ByteRange,
        replacement_text: String,
        original_text: String,
        cause: EditorTransactionCause,
        expected_revision: EditorRevision,
    },
    SetSelection {
        anchor: Utf8ByteOffset,
        head: Utf8ByteOffset,
        expected_revision: EditorRevision,
    },
    Undo {
        expected_revision: EditorRevision,
    },
    Redo {
        expected_revision: EditorRevision,
    },
    ReplaceAll {
        search: String,
        replacement: String,
        expected_revision: EditorRevision,
    },
    InsertLineBreak {
        byte_offset: Utf8ByteOffset,
        auto_indent_enabled: bool,
        cause: EditorTransactionCause,
        expected_revision: EditorRevision,
    },
    CommitText {
        byte_range: Utf8ByteRange,
        replacement_text: String,
        resulting_selection_anchor: Utf8ByteOffset,
        resulting_selection_head: Utf8ByteOffset,
        composition_session_id: EditorSessionId,
        composition_base_revision: EditorRevision,
        composition_generation: EditorSessionGeneration,
        cause: EditorTransactionCause,
        expected_revision: EditorRevision,
    },
    DeleteSurrounding {
        before_byte_range: Utf8ByteRange,
        after_byte_range: Utf8ByteRange,
        cause: EditorTransactionCause,
        expected_revision: EditorRevision,
    },
    BeginComposition {
        replace_range: Utf8ByteRange,
        expected_revision: EditorRevision,
    },
    UpdateComposition {
        composition_session_id: EditorSessionId,
        composition_generation: EditorSessionGeneration,
        new_preedit_text: String,
        new_preedit_cursor_utf16: Utf16CodeUnitOffset,
        expected_revision: EditorRevision,
    },
    FinishComposition {
        composition_session_id: EditorSessionId,
        composition_generation: EditorSessionGeneration,
        expected_revision: EditorRevision,
    },
    CancelComposition {
        composition_session_id: EditorSessionId,
        composition_generation: EditorSessionGeneration,
        expected_revision: EditorRevision,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayPatch {
    pub base_revision: EditorRevision,
    pub new_revision: EditorRevision,
    pub replace_byte_range: Utf8ByteRange,
    pub inserted_text: String,
    pub resulting_selection_byte_range: Utf8ByteRange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorVisualIntent {
    pub cause: EditorTransactionCause,
    pub operation_kind: EditorOperationKind,
    pub old_affected_byte_ranges: Vec<Utf8ByteRange>,
    pub new_affected_byte_ranges: Vec<Utf8ByteRange>,
    pub animation_mode: AnimationMode,
    pub duration_ms: u64,
    pub coordinated_cursor: CoordinatedCursor,
    /// #606: Core 计算的 old/new 正文偏移映射。
    ///
    /// 正文变更时由 `OffsetMap::build(&old_text, &new_text)` 填充；
    /// 纯选区/光标操作（不修改正文）为 `None`。平台端 AffectedLayoutPlanner
    /// 直接消费此字段，不再在 Kotlin 中独立推导 offset mapping。
    pub offset_map: Option<OffsetMap>,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatedCursor {
    pub old_offset: Utf8ByteOffset,
    pub new_offset: Utf8ByteOffset,
    pub should_animate: bool,
}
