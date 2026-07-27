use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Utf8ByteOffset(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidUtf8OffsetError {
    BeyondEnd { offset: usize, text_len: usize },
    NotCharBoundary { offset: usize },
}

impl fmt::Display for InvalidUtf8OffsetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidUtf8OffsetError::BeyondEnd { offset, text_len } => {
                write!(f, "offset {} beyond text length {}", offset, text_len)
            }
            InvalidUtf8OffsetError::NotCharBoundary { offset } => {
                write!(f, "offset {} is not a UTF-8 char boundary", offset)
            }
        }
    }
}

impl std::error::Error for InvalidUtf8OffsetError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidUtf8RangeError {
    StartAfterEnd { start: usize, end: usize },
    InvalidStart(InvalidUtf8OffsetError),
    InvalidEnd(InvalidUtf8OffsetError),
}

impl fmt::Display for InvalidUtf8RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidUtf8RangeError::StartAfterEnd { start, end } => {
                write!(f, "start {} > end {}", start, end)
            }
            InvalidUtf8RangeError::InvalidStart(e) => write!(f, "invalid start: {}", e),
            InvalidUtf8RangeError::InvalidEnd(e) => write!(f, "invalid end: {}", e),
        }
    }
}

impl std::error::Error for InvalidUtf8RangeError {}

impl Utf8ByteOffset {
    pub fn try_new(text: &str, offset: usize) -> Result<Self, InvalidUtf8OffsetError> {
        if offset > text.len() {
            return Err(InvalidUtf8OffsetError::BeyondEnd {
                offset,
                text_len: text.len(),
            });
        }
        if !text.is_char_boundary(offset) {
            return Err(InvalidUtf8OffsetError::NotCharBoundary { offset });
        }
        Ok(Self(offset))
    }

    pub fn clamp(text: &str, offset: usize) -> Self {
        let clamped = if offset > text.len() {
            text.len()
        } else {
            crate::editor::transaction::clamp_to_char_boundary(text, offset)
        };
        Self(clamped)
    }

    pub(super) fn unchecked(offset: usize) -> Self {
        Self(offset)
    }

    pub fn value(self) -> usize {
        self.0
    }
}

impl fmt::Display for Utf8ByteOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Utf8ByteOffset({})", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Utf8ByteRange {
    start: Utf8ByteOffset,
    end: Utf8ByteOffset,
}

impl Utf8ByteRange {
    pub fn try_new(text: &str, start: usize, end: usize) -> Result<Self, InvalidUtf8RangeError> {
        if start > end {
            return Err(InvalidUtf8RangeError::StartAfterEnd { start, end });
        }
        let s = Utf8ByteOffset::try_new(text, start)
            .map_err(InvalidUtf8RangeError::InvalidStart)?;
        let e = Utf8ByteOffset::try_new(text, end)
            .map_err(InvalidUtf8RangeError::InvalidEnd)?;
        Ok(Self { start: s, end: e })
    }

    pub fn clamp(text: &str, start: usize, end: usize) -> Self {
        let s = Utf8ByteOffset::clamp(text, start);
        let e = Utf8ByteOffset::clamp(text, end);
        if s.value() > e.value() {
            Self { start: e, end: e }
        } else {
            Self { start: s, end: e }
        }
    }

    pub(super) fn from_values(start: usize, end: usize) -> Option<Self> {
        if start > end {
            return None;
        }
        Some(Self {
            start: Utf8ByteOffset::unchecked(start),
            end: Utf8ByteOffset::unchecked(end),
        })
    }

    pub fn start(self) -> Utf8ByteOffset {
        self.start
    }

    pub fn end(self) -> Utf8ByteOffset {
        self.end
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn len(self) -> usize {
        self.end.value().saturating_sub(self.start.value())
    }

    pub fn to_std_range(self) -> Range<usize> {
        self.start.value()..self.end.value()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EditorRevision(u64);

impl EditorRevision {
    pub fn new(revision: u64) -> Self {
        Self(revision)
    }

    pub fn initial() -> Self {
        Self(0)
    }

    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EditorRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EditorRevision({})", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EditorSessionId(u64);

impl EditorSessionId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EditorSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EditorSessionId({})", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EditorSessionGeneration(u64);

impl EditorSessionGeneration {
    pub fn new(gen: u64) -> Self {
        Self(gen)
    }

    pub fn initial() -> Self {
        Self(0)
    }

    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EditorSessionGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EditorSessionGeneration({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_byte_offset_try_new_valid() {
        let text = "你好";
        assert!(Utf8ByteOffset::try_new(text, 0).is_ok());
        assert!(Utf8ByteOffset::try_new(text, 3).is_ok());
        assert!(Utf8ByteOffset::try_new(text, 6).is_ok());
    }

    #[test]
    fn utf8_byte_offset_try_new_rejects_mid_char() {
        let text = "你好";
        assert!(Utf8ByteOffset::try_new(text, 1).is_err());
        assert!(Utf8ByteOffset::try_new(text, 2).is_err());
        assert!(Utf8ByteOffset::try_new(text, 4).is_err());
        assert!(Utf8ByteOffset::try_new(text, 5).is_err());
    }

    #[test]
    fn utf8_byte_offset_try_new_rejects_beyond_end() {
        let text = "abc";
        let result = Utf8ByteOffset::try_new(text, 100);
        assert!(result.is_err());
        if let Err(InvalidUtf8OffsetError::BeyondEnd { offset, text_len }) = result {
            assert_eq!(offset, 100);
            assert_eq!(text_len, 3);
        } else {
            panic!("expected BeyondEnd error");
        }
    }

    #[test]
    fn utf8_byte_range_try_new_valid() {
        let text = "你好世界";
        assert!(Utf8ByteRange::try_new(text, 0, 6).is_ok());
        assert!(Utf8ByteRange::try_new(text, 0, 0).is_ok());
    }

    #[test]
    fn utf8_byte_range_try_new_rejects_start_after_end() {
        let text = "abc";
        assert!(Utf8ByteRange::try_new(text, 3, 0).is_err());
    }

    #[test]
    fn utf8_byte_range_try_new_rejects_mid_char() {
        let text = "你好";
        assert!(Utf8ByteRange::try_new(text, 1, 3).is_err());
        assert!(Utf8ByteRange::try_new(text, 0, 1).is_err());
    }

    #[test]
    fn utf8_byte_range_try_new_rejects_beyond_end() {
        let text = "abc";
        assert!(Utf8ByteRange::try_new(text, 0, 5).is_err());
    }

    #[test]
    fn editor_revision_next() {
        let rev = EditorRevision::initial();
        assert_eq!(rev.value(), 0);
        assert_eq!(rev.next().value(), 1);
    }

    #[test]
    fn editor_session_generation_next() {
        let gen = EditorSessionGeneration::initial();
        assert_eq!(gen.value(), 0);
        assert_eq!(gen.next().value(), 1);
    }

    #[test]
    fn utf8_byte_offset_clamp_valid() {
        let text = "你好世界";
        assert_eq!(Utf8ByteOffset::clamp(text, 0).value(), 0);
        assert_eq!(Utf8ByteOffset::clamp(text, 3).value(), 3);
        assert_eq!(Utf8ByteOffset::clamp(text, 6).value(), 6);
    }

    #[test]
    fn utf8_byte_offset_clamp_mid_char() {
        let text = "你好";
        assert_eq!(Utf8ByteOffset::clamp(text, 1).value(), 0);
        assert_eq!(Utf8ByteOffset::clamp(text, 2).value(), 0);
        assert_eq!(Utf8ByteOffset::clamp(text, 4).value(), 3);
    }

    #[test]
    fn utf8_byte_offset_clamp_beyond_end() {
        let text = "abc";
        assert_eq!(Utf8ByteOffset::clamp(text, 100).value(), 3);
    }

    #[test]
    fn utf8_byte_range_clamp_valid() {
        let text = "abc";
        let range = Utf8ByteRange::clamp(text, 0, 2);
        assert_eq!(range.start().value(), 0);
        assert_eq!(range.end().value(), 2);
    }

    #[test]
    fn utf8_byte_range_clamp_mid_char() {
        let text = "你好";
        let range = Utf8ByteRange::clamp(text, 1, 4);
        assert_eq!(range.start().value(), 0);
        assert_eq!(range.end().value(), 3);
    }

    #[test]
    fn utf8_byte_range_clamp_start_after_end() {
        let text = "abc";
        let range = Utf8ByteRange::clamp(text, 2, 1);
        assert_eq!(range.start().value(), 1);
        assert_eq!(range.end().value(), 1);
    }

    #[test]
    fn undo_entry_uses_strong_cursor_types() {
        use crate::editor::kernel::UndoEntry;
        let entry = UndoEntry {
            old_text: "hello".to_string(),
            new_text: "world".to_string(),
            old_cursor: Utf8ByteOffset::unchecked(0),
            new_cursor: Utf8ByteOffset::unchecked(5),
        };
        assert_eq!(entry.old_cursor.value(), 0);
        assert_eq!(entry.new_cursor.value(), 5);
    }

    #[test]
    fn composition_session_state_uses_strong_range_types() {
        use crate::editor::kernel::CompositionSessionState;
        let state = CompositionSessionState {
            session_id: EditorSessionId::new(1),
            base_revision: EditorRevision::initial(),
            generation: EditorSessionGeneration::initial(),
            replace_start: Utf8ByteOffset::unchecked(3),
            replace_end_exclusive: Utf8ByteOffset::unchecked(6),
            preedit_text: "你好".to_string(),
            preedit_cursor_utf16: 1,
        };
        assert_eq!(state.replace_start.value(), 3);
        assert_eq!(state.replace_end_exclusive.value(), 6);
        assert_ne!(state.replace_start, state.replace_end_exclusive);
    }

    #[test]
    fn strong_types_prevent_cross_assignment() {
        let offset = Utf8ByteOffset::unchecked(42);
        let revision = EditorRevision::new(42);
        let session_id = EditorSessionId::new(42);
        let generation = EditorSessionGeneration::new(42);
        assert_eq!(offset.value() as u64, revision.value());
        assert_eq!(revision.value(), session_id.value());
        assert_eq!(session_id.value(), generation.value());
    }
}
