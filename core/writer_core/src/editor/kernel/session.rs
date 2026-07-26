use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::result::{EditorEditOutcome, EditorEditResult};
use super::EditorKernel;

use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset, Utf8ByteRange};
use crate::editor::transaction::{
    EditorChange, EditorTransactionCause, AnimationMode,
};

impl EditorKernel {
    pub fn load_text(&mut self, text: String, cursor: usize) -> EditorEditOutcome {
        let base_revision = self.revision;
        let old_cursor = self.cursor;
        let old_selection = Utf8ByteRange::from_values(self.selection_anchor.value(), self.cursor.value())
            .unwrap_or(Utf8ByteRange::from_values(0, 0).unwrap());

        let needs_clamp = cursor > text.len() || !text.is_char_boundary(cursor);
        let resolved_cursor = if needs_clamp {
            Self::clamp_to_char_boundary(&text, cursor)
        } else {
            cursor
        };

        let old_text = self.text.clone();
        self.text = text;
        self.cursor = Utf8ByteOffset::unchecked(resolved_cursor);
        self.selection_anchor = Utf8ByteOffset::unchecked(resolved_cursor);
        self.revision = self.revision.next();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.composition_session = None;

        let new_selection = Utf8ByteRange::from_values(resolved_cursor, resolved_cursor).unwrap();
        let new_revision = self.revision;

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_values(0, old_text.len()).unwrap(),
            inserted_text: self.text.clone(),
            resulting_selection_byte_range: new_selection,
        }];

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Load,
            operation_kind: EditorOperationKind::Load,
            old_affected_byte_ranges: if old_text.is_empty() { vec![] } else { vec![Utf8ByteRange::from_values(0, old_text.len()).unwrap()] },
            new_affected_byte_ranges: if self.text.is_empty() { vec![] } else { vec![Utf8ByteRange::from_values(0, self.text.len()).unwrap()] },
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(resolved_cursor),
                should_animate: false,
            },
        };

        let result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        };

        if needs_clamp {
            EditorEditOutcome::AppliedWithAdjustedSelection(result)
        } else {
            EditorEditOutcome::Applied(result)
        }
    }

    pub(crate) fn stale_session_result(&mut self) -> EditorEditResult {
        let current_selection = Utf8ByteRange::from_values(self.selection_anchor.value(), self.cursor.value())
            .unwrap_or(Utf8ByteRange::from_values(0, 0).unwrap());
        EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision: self.revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: current_selection,
            new_selection_byte_range: current_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::Programmatic,
                operation_kind: EditorOperationKind::CursorOnly,
                old_affected_byte_ranges: vec![],
                new_affected_byte_ranges: vec![],
                animation_mode: AnimationMode::SystemSuppressed,
                duration_ms: 0,
                coordinated_cursor: CoordinatedCursor {
                    old_offset: self.cursor,
                    new_offset: self.cursor,
                    should_animate: false,
                },
            },
        }
    }

    pub(crate) fn noop_result(
        &mut self,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
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
                    old_offset: old_cursor,
                    new_offset: old_cursor,
                    should_animate: false,
                },
            },
        }
    }

    pub(crate) fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
        if offset > text.len() {
            return text.len();
        }
        if text.is_char_boundary(offset) {
            return offset;
        }
        let mut clamped = offset;
        while clamped > 0 && !text.is_char_boundary(clamped) {
            clamped -= 1;
        }
        clamped
    }

    pub(crate) fn normalize_range(start: usize, end: usize) -> (usize, usize) {
        if start > end {
            (end, start)
        } else {
            (start, end)
        }
    }

    pub(crate) fn compute_single_patch(old_text: &str, new_text: &str) -> (Utf8ByteRange, String) {
        if old_text == new_text {
            return (Utf8ByteRange::from_values(0, 0).unwrap(), String::new());
        }

        let mut prefix_len = 0;
        for (ob, nb) in old_text.bytes().zip(new_text.bytes()) {
            if ob != nb { break; }
            prefix_len += 1;
        }
        while prefix_len > 0 && !old_text.is_char_boundary(prefix_len) {
            prefix_len -= 1;
        }
        while prefix_len > 0 && !new_text.is_char_boundary(prefix_len) {
            prefix_len -= 1;
        }

        let mut old_suffix_len = 0;
        let mut new_suffix_len = 0;
        {
            let old_remaining = &old_text[prefix_len..];
            let new_remaining = &new_text[prefix_len..];
            let old_rev = old_remaining.bytes().rev();
            let new_rev = new_remaining.bytes().rev();
            for (ob, nb) in old_rev.zip(new_rev) {
                if ob != nb { break; }
                old_suffix_len += 1;
                new_suffix_len += 1;
            }
        }

        while old_suffix_len > 0 && !old_text.is_char_boundary(old_text.len() - old_suffix_len) {
            old_suffix_len -= 1;
        }
        while new_suffix_len > 0 && !new_text.is_char_boundary(new_text.len() - new_suffix_len) {
            new_suffix_len -= 1;
        }

        let old_remaining_after_prefix = old_text.len() - prefix_len;
        let new_remaining_after_prefix = new_text.len() - prefix_len;
        if old_suffix_len > old_remaining_after_prefix {
            old_suffix_len = old_remaining_after_prefix;
            while old_suffix_len > 0 && !old_text.is_char_boundary(old_text.len() - old_suffix_len) {
                old_suffix_len -= 1;
            }
        }
        if new_suffix_len > new_remaining_after_prefix {
            new_suffix_len = new_remaining_after_prefix;
            while new_suffix_len > 0 && !new_text.is_char_boundary(new_text.len() - new_suffix_len) {
                new_suffix_len -= 1;
            }
        }

        let replace_start = prefix_len;
        let replace_end = old_text.len() - old_suffix_len;
        let inserted_end = new_text.len() - new_suffix_len;

        if replace_start > replace_end && inserted_end <= prefix_len {
            return (Utf8ByteRange::from_values(replace_start, replace_start).unwrap(), String::new());
        }

        let inserted_text = if prefix_len < inserted_end {
            new_text[prefix_len..inserted_end].to_string()
        } else {
            String::new()
        };

        (Utf8ByteRange::from_values(replace_start, replace_end).unwrap(), inserted_text)
    }

    pub(crate) fn affected_ranges_from_changes(changes: &[EditorChange]) -> (Vec<Utf8ByteRange>, Vec<Utf8ByteRange>) {
        let mut old_ranges = Vec::new();
        let mut new_ranges = Vec::new();
        for c in changes {
            match c {
                EditorChange::Delete { index, text } => {
                    old_ranges.push(Utf8ByteRange::from_values(*index, *index + text.len()).unwrap());
                }
                EditorChange::Insert { index, text } => {
                    new_ranges.push(Utf8ByteRange::from_values(*index, *index + text.len()).unwrap());
                }
            }
        }
        (old_ranges, new_ranges)
    }

    pub(crate) fn take_transaction_id(&mut self) -> u64 {
        let id = self.next_transaction_id;
        self.next_transaction_id = self.next_transaction_id.saturating_add(1);
        id
    }
}
