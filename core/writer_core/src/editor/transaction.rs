//! 平台无关的编辑事务与动画事件模型。
//!
//! 本模块只描述正文如何变化以及渲染层可以播放什么事件，不处理绘制、输入法、窗口、
//! 鼠标或触摸。平台端必须把输入事件翻译成这里的 transaction，再由 renderer 决定如何画。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorCursor {
    /// UTF-8 byte offset. The value is always clamped to a char boundary.
    pub index: usize,
}

impl EditorCursor {
    pub fn new(text: &str, index: usize) -> Self {
        Self {
            index: clamp_to_char_boundary(text, index),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSelection {
    pub anchor: EditorCursor,
    pub head: EditorCursor,
}

impl EditorSelection {
    pub fn collapsed(text: &str, index: usize) -> Self {
        let cursor = EditorCursor::new(text, index);
        Self {
            anchor: cursor,
            head: cursor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum EditorChange {
    Insert { index: usize, text: String },
    Delete { index: usize, text: String },
}

impl EditorChange {
    pub fn index(&self) -> usize {
        match self {
            Self::Insert { index, .. } | Self::Delete { index, .. } => *index,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Insert { text, .. } | Self::Delete { text, .. } => text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorTransactionCause {
    Typing,
    Delete,
    Paste,
    Undo,
    Redo,
    Load,
    Format,
    ImeComposition,
    Programmatic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorTransaction {
    pub old_text: String,
    pub new_text: String,
    pub changes: Vec<EditorChange>,
    pub old_selection: EditorSelection,
    pub new_selection: EditorSelection,
    pub cause: EditorTransactionCause,
    pub should_animate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorAnimationKind {
    Insert,
    Delete,
    Cursor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorAnimationEvent {
    pub id: u64,
    pub kind: EditorAnimationKind,
    pub range_start: usize,
    pub range_len: usize,
    pub text: String,
    pub old_cursor: EditorCursor,
    pub new_cursor: EditorCursor,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct EditorEngine {
    next_animation_id: u64,
    max_animated_chars: usize,
    animation_duration_ms: u64,
}

impl Default for EditorEngine {
    fn default() -> Self {
        Self {
            next_animation_id: 1,
            max_animated_chars: 8,
            animation_duration_ms: 160,
        }
    }
}

impl EditorEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_animation_limits(max_animated_chars: usize, animation_duration_ms: u64) -> Self {
        Self {
            next_animation_id: 1,
            max_animated_chars,
            animation_duration_ms,
        }
    }

    pub fn create_transaction(
        &self,
        old_text: impl Into<String>,
        new_text: impl Into<String>,
        old_selection: EditorSelection,
        new_selection: EditorSelection,
        cause: EditorTransactionCause,
    ) -> EditorTransaction {
        let old_text = old_text.into();
        let new_text = new_text.into();
        let changes = diff_plain_text(&old_text, &new_text);
        let should_animate = should_animate_changes(&changes, cause, self.max_animated_chars);

        EditorTransaction {
            old_text,
            new_text,
            changes,
            old_selection,
            new_selection,
            cause,
            should_animate,
        }
    }

    pub fn animation_events(
        &mut self,
        transaction: &EditorTransaction,
    ) -> Vec<EditorAnimationEvent> {
        let mut events = Vec::new();
        if transaction.should_animate {
            for change in &transaction.changes {
                let kind = match change {
                    EditorChange::Insert { .. } => EditorAnimationKind::Insert,
                    EditorChange::Delete { .. } => EditorAnimationKind::Delete,
                };
                events.push(EditorAnimationEvent {
                    id: self.take_animation_id(),
                    kind,
                    range_start: change.index(),
                    range_len: change.text().len(),
                    text: change.text().to_string(),
                    old_cursor: transaction.old_selection.head,
                    new_cursor: transaction.new_selection.head,
                    duration_ms: self.animation_duration_ms,
                });
            }
        }

        if transaction.cause != EditorTransactionCause::Load
            && transaction.old_selection.head != transaction.new_selection.head
        {
            events.push(EditorAnimationEvent {
                id: self.take_animation_id(),
                kind: EditorAnimationKind::Cursor,
                range_start: transaction.new_selection.head.index,
                range_len: 0,
                text: String::new(),
                old_cursor: transaction.old_selection.head,
                new_cursor: transaction.new_selection.head,
                duration_ms: self.animation_duration_ms,
            });
        }

        events
    }

    fn take_animation_id(&mut self) -> u64 {
        let id = self.next_animation_id;
        self.next_animation_id = self.next_animation_id.saturating_add(1);
        id
    }
}

pub fn diff_plain_text(old_text: &str, new_text: &str) -> Vec<EditorChange> {
    if old_text == new_text {
        return Vec::new();
    }

    let prefix = common_prefix_byte_len(old_text, new_text);
    let suffix = common_suffix_byte_len(old_text, new_text, prefix);
    let old_end = old_text.len() - suffix;
    let new_end = new_text.len() - suffix;
    let removed = &old_text[prefix..old_end];
    let inserted = &new_text[prefix..new_end];

    let mut changes = Vec::new();
    if !removed.is_empty() {
        changes.push(EditorChange::Delete {
            index: prefix,
            text: removed.to_string(),
        });
    }
    if !inserted.is_empty() {
        changes.push(EditorChange::Insert {
            index: prefix,
            text: inserted.to_string(),
        });
    }
    changes
}

fn should_animate_changes(
    changes: &[EditorChange],
    cause: EditorTransactionCause,
    max_animated_chars: usize,
) -> bool {
    if !matches!(cause, EditorTransactionCause::Typing | EditorTransactionCause::Delete) {
        return false;
    }
    if changes.len() != 1 {
        return false;
    }
    let text = changes[0].text();
    !text.is_empty()
        && !text.contains('\n')
        && text.chars().count() <= max_animated_chars
}

fn common_prefix_byte_len(old_text: &str, new_text: &str) -> usize {
    let mut prefix = 0;
    for ((old_index, old_char), (_, new_char)) in old_text.char_indices().zip(new_text.char_indices()) {
        if old_char != new_char {
            break;
        }
        prefix = old_index + old_char.len_utf8();
    }
    prefix
}

fn common_suffix_byte_len(old_text: &str, new_text: &str, prefix: usize) -> usize {
    let old_tail = &old_text[prefix..];
    let new_tail = &new_text[prefix..];
    let mut suffix = 0;
    for ((_, old_char), (_, new_char)) in old_tail.char_indices().rev().zip(new_tail.char_indices().rev()) {
        if old_char != new_char {
            break;
        }
        suffix += old_char.len_utf8();
    }
    suffix
}

fn clamp_to_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let mut safe = index;
    while safe > 0 && !text.is_char_boundary(safe) {
        safe -= 1;
    }
    safe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_single_insert_on_utf8_boundary() {
        let changes = diff_plain_text("你好世界", "你好新世界");
        assert_eq!(
            changes,
            vec![EditorChange::Insert {
                index: "你好".len(),
                text: "新".to_string(),
            }]
        );
    }

    #[test]
    fn detects_single_delete_on_utf8_boundary() {
        let changes = diff_plain_text("abc月def", "abcdef");
        assert_eq!(
            changes,
            vec![EditorChange::Delete {
                index: "abc".len(),
                text: "月".to_string(),
            }]
        );
    }

    #[test]
    fn replacement_is_delete_then_insert() {
        let changes = diff_plain_text("alpha beta", "alpha gamma");
        assert_eq!(
            changes,
            vec![
                EditorChange::Delete {
                    index: "alpha ".len(),
                    text: "bet".to_string(),
                },
                EditorChange::Insert {
                    index: "alpha ".len(),
                    text: "gamm".to_string(),
                },
            ]
        );
    }

    #[test]
    fn typing_transaction_emits_insert_and_cursor_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );

        assert!(tx.should_animate);
        let events = engine.animation_events(&tx);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
        assert_eq!(events[0].text, "c");
        assert_eq!(events[1].kind, EditorAnimationKind::Cursor);
    }

    #[test]
    fn paste_does_not_emit_text_animation() {
        let mut engine = EditorEngine::new();
        let tx = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );

        assert!(!tx.should_animate);
        let events = engine.animation_events(&tx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EditorAnimationKind::Cursor);
    }

    #[test]
    fn load_does_not_emit_animation_events() {
        let mut engine = EditorEngine::new();
        let tx = engine.create_transaction(
            "",
            "loaded",
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed("loaded", 6),
            EditorTransactionCause::Load,
        );

        assert!(!tx.should_animate);
        assert!(engine.animation_events(&tx).is_empty());
    }
}
