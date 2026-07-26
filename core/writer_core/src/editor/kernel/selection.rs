use super::types::{CoordinatedCursor, EditorOperationKind, EditorVisualIntent};
use super::result::{EditorEditOutcome, EditorEditResult};
use super::EditorKernel;

use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset, Utf8ByteRange};
use crate::editor::transaction::{AnimationMode, EditorTransactionCause};

impl EditorKernel {
    pub(crate) fn apply_set_selection(
        &mut self,
        anchor_byte_offset: usize,
        head_byte_offset: usize,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let anchor = anchor_byte_offset;
        let head = head_byte_offset;
        if anchor > self.text.len() || head > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(anchor) || !self.text.is_char_boundary(head) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        self.selection_anchor = Utf8ByteOffset::unchecked(anchor);
        self.cursor = Utf8ByteOffset::unchecked(head);

        let new_selection = Utf8ByteRange::new(anchor, head).unwrap();

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Programmatic,
            operation_kind: EditorOperationKind::CursorOnly,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: vec![],
            animation_mode: if self.animation_enabled && old_cursor.value() != head {
                AnimationMode::GlyphAnimation
            } else {
                AnimationMode::SystemSuppressed
            },
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(head),
                should_animate: self.animation_enabled && old_cursor.value() != head,
            },
        };

        EditorEditOutcome::NoChange(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }
}
