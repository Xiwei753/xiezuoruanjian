//! Editor animation DTOs for cross-platform FFI.
//!
//! These DTOs mirror the Core `editor::transaction` types but are
//! stable API boundary types used by UniFFI and C-ABI FFI layers.
//! Platform clients (Android, Desktop, HarmonyOS) consume these
//! to decide whether and how to animate text changes.

/// Kind of animation event emitted by the editor engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum EditorAnimationKindDto {
    #[default]
    Insert,
    Delete,
    Cursor,
}

impl From<crate::editor::EditorAnimationKind> for EditorAnimationKindDto {
    fn from(k: crate::editor::EditorAnimationKind) -> Self {
        match k {
            crate::editor::EditorAnimationKind::Insert => Self::Insert,
            crate::editor::EditorAnimationKind::Delete => Self::Delete,
            crate::editor::EditorAnimationKind::Cursor => Self::Cursor,
        }
    }
}

/// Cause of an editor transaction, used by Core to decide `should_animate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum EditorTransactionCauseDto {
    #[default]
    Typing,
    Delete,
    ImeComposition,
    Paste,
    Undo,
    Redo,
    Load,
    Format,
    Programmatic,
}

impl From<EditorTransactionCauseDto> for crate::editor::EditorTransactionCause {
    fn from(c: EditorTransactionCauseDto) -> Self {
        match c {
            EditorTransactionCauseDto::Typing => Self::Typing,
            EditorTransactionCauseDto::Delete => Self::Delete,
            EditorTransactionCauseDto::ImeComposition => Self::ImeComposition,
            EditorTransactionCauseDto::Paste => Self::Paste,
            EditorTransactionCauseDto::Undo => Self::Undo,
            EditorTransactionCauseDto::Redo => Self::Redo,
            EditorTransactionCauseDto::Load => Self::Load,
            EditorTransactionCauseDto::Format => Self::Format,
            EditorTransactionCauseDto::Programmatic => Self::Programmatic,
        }
    }
}

/// A single animation event produced by the Core EditorEngine.
///
/// The platform side uses `range_start`, `range_len`, `text` to locate
/// the affected text in its own Layout, then computes glyph coordinates
/// and submits them to its renderer (e.g., Android `EditorRenderLayer`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorAnimationEventDto {
    pub id: u64,
    pub kind: EditorAnimationKindDto,
    pub range_start: u32,
    pub range_len: u32,
    pub text: String,
    pub old_cursor_index: u32,
    pub new_cursor_index: u32,
    pub duration_ms: u64,
}

impl From<crate::editor::EditorAnimationEvent> for EditorAnimationEventDto {
    fn from(e: crate::editor::EditorAnimationEvent) -> Self {
        Self {
            id: e.id,
            kind: e.kind.into(),
            range_start: e.range_start as u32,
            range_len: e.range_len as u32,
            text: e.text,
            old_cursor_index: e.old_cursor.index as u32,
            new_cursor_index: e.new_cursor.index as u32,
            duration_ms: e.duration_ms,
        }
    }
}
