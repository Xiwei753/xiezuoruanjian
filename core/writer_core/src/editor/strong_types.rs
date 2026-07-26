use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Utf8ByteOffset(pub usize);

impl Utf8ByteOffset {
    pub fn new(text: &str, offset: usize) -> Self {
        let clamped = if offset > text.len() {
            text.len()
        } else {
            let mut safe = offset;
            while safe > 0 && !text.is_char_boundary(safe) {
                safe -= 1;
            }
            safe
        };
        Self(clamped)
    }

    pub fn unchecked(offset: usize) -> Self {
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
    pub start: Utf8ByteOffset,
    pub end: Utf8ByteOffset,
}

impl Utf8ByteRange {
    pub fn new(start: usize, end: usize) -> Option<Self> {
        if start > end {
            return None;
        }
        Some(Self {
            start: Utf8ByteOffset::unchecked(start),
            end: Utf8ByteOffset::unchecked(end),
        })
    }

    pub fn new_checked(text: &str, start: usize, end: usize) -> Option<Self> {
        if start > end {
            return None;
        }
        let s = Utf8ByteOffset::new(text, start);
        let e = Utf8ByteOffset::new(text, end);
        Some(Self { start: s, end: e })
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn len(self) -> usize {
        self.end.0.saturating_sub(self.start.0)
    }

    pub fn to_std_range(self) -> Range<usize> {
        self.start.0..self.end.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EditorRevision(pub u64);

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
pub struct EditorSessionId(pub u64);

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
pub struct EditorSessionGeneration(pub u64);

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
    fn utf8_byte_offset_clamps_to_char_boundary() {
        let text = "你好";
        assert_eq!(Utf8ByteOffset::new(text, 0).value(), 0);
        assert_eq!(Utf8ByteOffset::new(text, 1).value(), 0);
        assert_eq!(Utf8ByteOffset::new(text, 3).value(), 3);
        assert_eq!(Utf8ByteOffset::new(text, 6).value(), 6);
        assert_eq!(Utf8ByteOffset::new(text, 100).value(), 6);
    }

    #[test]
    fn utf8_byte_range_validates_order() {
        assert!(Utf8ByteRange::new(0, 3).is_some());
        assert!(Utf8ByteRange::new(3, 0).is_none());
        assert!(Utf8ByteRange::new(0, 0).is_some());
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
    fn utf8_byte_range_checked_clamps() {
        let text = "abc";
        let range = Utf8ByteRange::new_checked(text, 0, 5);
        assert!(range.is_some());
        let r = range.unwrap();
        assert_eq!(r.start.value(), 0);
        assert_eq!(r.end.value(), 3);
    }
}
