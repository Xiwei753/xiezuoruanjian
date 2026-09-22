use serde::{Deserialize, Serialize};

use crate::editor::strong_types::{
    EditorRevision, EditorSessionGeneration, EditorSessionId, Utf16CodeUnitOffset, Utf8ByteOffset,
    Utf8ByteRange,
};
use crate::editor::transaction::EditorTransactionCause;

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
    /// 原子 IME commit — Qt `QInputMethodEvent` 两步语义的原子执行：
    /// 1. 先删除 selection `[selection_byte_range.start, selection_byte_range.end)`；
    /// 2. 再在删完 selection 后的文本（base_text）上删除
    ///    `[replacement_byte_range_after_selection.start,
    ///    replacement_byte_range_after_selection.end)` 并在
    ///    `replacement_byte_range_after_selection.start` 插入 `inserted_text`。
    ///
    /// 整个操作只产生一个 revision 推进和一个 UndoEntry。
    ///
    /// Qt 对 `QInputMethodEvent` 的定义：先删除当前 selection，再做
    /// replacement/commit，整个 operation 加入 undo stack。
    /// 不要把 selection 删除和 replacement 拆成两次 pipeline command。
    ///
    /// 坐标空间（全部 UTF-8 byte offset，半开区间）：
    /// - `selection_byte_range`：第一步删除的 committed text byte range。
    ///   零长度（start == end）表示无 selection 删除。
    /// - `replacement_byte_range_after_selection`：第二步在删完 selection 后的
    ///   文本（base_text）上做 replacement/commit 的 byte range。这是 base_text
    ///   坐标，不是原始 committed text 坐标。
    /// - `inserted_text`：第二步插入的 commit 文本。可以为空（纯删除场景）。
    ImeCommit {
        selection_byte_range: Utf8ByteRange,
        replacement_byte_range_after_selection: Utf8ByteRange,
        inserted_text: String,
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
    //  R8: composition 专用 grapheme 语义操作。
    // 只改 composition session 的 preeditText / preeditCursorUtf16 / generation；
    // 不修改 committed 正文，不把 raw platform event 带入 Core。
    CompositionMoveGraphemeLeft {
        composition_session_id: EditorSessionId,
        composition_generation: EditorSessionGeneration,
        expected_revision: EditorRevision,
    },
    CompositionMoveGraphemeRight {
        composition_session_id: EditorSessionId,
        composition_generation: EditorSessionGeneration,
        expected_revision: EditorRevision,
    },
    CompositionDeleteGraphemeBackward {
        composition_session_id: EditorSessionId,
        composition_generation: EditorSessionGeneration,
        expected_revision: EditorRevision,
    },
    CompositionDeleteGraphemeForward {
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
