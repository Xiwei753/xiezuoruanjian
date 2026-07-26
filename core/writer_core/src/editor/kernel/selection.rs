use super::types::{CoordinatedCursor, EditorOperationKind, EditorVisualIntent};
use super::result::{EditorEditOutcome, EditorEditResult};
use super::EditorKernel;

use crate::editor::strong_types::Utf8ByteOffset;
use crate::editor::transaction::{AnimationMode, EditorTransactionCause};

impl EditorKernel {
    pub(crate) fn apply_set_selection(
        &mut self,
        anchor_byte_offset: usize,
        head_byte_offset: usize,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
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

        let new_selection = (anchor, head);

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Programmatic,
            operation_kind: EditorOperationKind::CursorOnly,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: vec![],
            animation_mode: if self.animation_enabled && old_cursor != head {
                AnimationMode::GlyphAnimation
            } else {
                AnimationMode::SystemSuppressed
            },
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: head,
                should_animate: self.animation_enabled && old_cursor != head,
            },
        };

        EditorEditOutcome::NoChange(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision.value(),
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }
}
