//! Rust EditorKernel — 正文和业务唯一真相。

mod apply;
mod composition;
mod history;
mod replace;
pub mod result;
mod selection;
mod session;
mod tests;
pub mod types;

use self::result::EditorInputError;
use crate::editor::strong_types::{
    EditorRevision, EditorSessionGeneration, EditorSessionId, Utf8ByteOffset,
};

#[derive(Debug, Clone)]
pub struct EditorKernel {
    text: String,
    revision: EditorRevision,
    cursor: Utf8ByteOffset,
    selection_anchor: Utf8ByteOffset,
    next_transaction_id: u64,
    animation_duration_ms: u64,
    animation_enabled: bool,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    composition_session: Option<CompositionSessionState>,
    next_composition_session_id: EditorSessionId,
}

#[derive(Debug, Clone)]
pub(crate) struct CompositionSessionState {
    pub(crate) session_id: EditorSessionId,
    pub(crate) base_revision: EditorRevision,
    pub(crate) generation: EditorSessionGeneration,
    pub(crate) replace_start: Utf8ByteOffset,
    pub(crate) replace_end_exclusive: Utf8ByteOffset,
    pub(crate) preedit_text: String,
    pub(crate) preedit_cursor_utf16: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct UndoEntry {
    pub(crate) old_text: String,
    pub(crate) new_text: String,
    pub(crate) old_cursor: Utf8ByteOffset,
    pub(crate) new_cursor: Utf8ByteOffset,
}

impl Default for EditorKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorKernel {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            revision: EditorRevision::initial(),
            cursor: Utf8ByteOffset::unchecked(0),
            selection_anchor: Utf8ByteOffset::unchecked(0),
            next_transaction_id: 1,
            animation_duration_ms: 80,
            animation_enabled: true,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            composition_session: None,
            next_composition_session_id: EditorSessionId::new(1),
        }
    }

    pub fn with_text(text: String, cursor: usize) -> Result<Self, EditorInputError> {
        if cursor > text.len() || !text.is_char_boundary(cursor) {
            return Err(EditorInputError::InvalidCursorOffset {
                offset: cursor,
                text_len: text.len(),
            });
        }
        Ok(Self {
            text,
            revision: EditorRevision::initial(),
            cursor: Utf8ByteOffset::unchecked(cursor),
            selection_anchor: Utf8ByteOffset::unchecked(cursor),
            next_transaction_id: 1,
            animation_duration_ms: 80,
            animation_enabled: true,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            composition_session: None,
            next_composition_session_id: EditorSessionId::new(1),
        })
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
        self.revision.value()
    }

    pub fn cursor(&self) -> usize {
        self.cursor.value()
    }

    pub fn selection_anchor(&self) -> usize {
        self.selection_anchor.value()
    }

    pub fn selection(&self) -> (usize, usize) {
        (self.selection_anchor.value(), self.cursor.value())
    }

    /// #606: 返回严格在 `byte_offset` 之前的最近 grapheme cluster 边界（UTF-8 byte offset）。
    ///
    /// 平台端 Backspace/Delete 的 grapheme 边界计算由 Core 唯一决定，
    /// 不再依赖 ICU BreakIterator。
    #[allow(clippy::cast_possible_truncation)]
    pub fn previous_grapheme_boundary(&self, byte_offset: u32) -> u32 {
        use unicode_segmentation::UnicodeSegmentation;
        let text = self.text();
        let offset = byte_offset as usize;
        if offset == 0 {
            return 0;
        }
        if offset > text.len() {
            return text.len() as u32;
        }
        let mut prev: usize = 0;
        for (start, _) in text.grapheme_indices(true) {
            if start >= offset {
                break;
            }
            prev = start;
        }
        prev as u32
    }

    /// #606: 返回严格在 `byte_offset` 之后的最近 grapheme cluster 边界（UTF-8 byte offset）。
    ///
    /// 平台端 Backspace/Delete 的 grapheme 边界计算由 Core 唯一决定，
    /// 不再依赖 ICU BreakIterator。
    #[allow(clippy::cast_possible_truncation)]
    pub fn next_grapheme_boundary(&self, byte_offset: u32) -> u32 {
        use unicode_segmentation::UnicodeSegmentation;
        let text = self.text();
        let offset = byte_offset as usize;
        if offset >= text.len() {
            return text.len() as u32;
        }
        for (start, g) in text.grapheme_indices(true) {
            let end = start + g.len();
            if end > offset {
                return end as u32;
            }
        }
        text.len() as u32
    }
}
