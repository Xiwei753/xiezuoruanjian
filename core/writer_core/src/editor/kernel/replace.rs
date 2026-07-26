use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::result::{EditorEditOutcome, EditorEditResult};
use super::{EditorKernel, UndoEntry};

use crate::editor::strong_types::Utf8ByteOffset;
use crate::editor::transaction::{
    diff_plain_text,
    AnimationMode, EditorTransactionCause,
};

impl EditorKernel {
    pub(crate) fn apply_replace_all(
        &mut self,
        search: &str,
        replacement: &str,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let old_text = self.text.clone();
        let new_text = old_text.replace(search, replacement);

        if new_text == old_text {
            return EditorEditOutcome::NoChange(self.noop_result(base_revision, old_cursor, old_selection));
        }

        self.text = new_text;
        self.revision = self.revision.next();
        let new_cursor_val = Self::clamp_to_char_boundary(&self.text, self.cursor.value());
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.composition_session = None;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: new_cursor_val,
        });
        self.redo_stack.clear();

        let new_revision = self.revision.value();
        let new_selection = (new_cursor_val, new_cursor_val);

        let (replace_range, inserted_text) = Self::compute_single_patch(&old_text, &self.text);

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: replace_range,
            inserted_text,
            resulting_selection_byte_range: new_selection,
        }];

        let changes = diff_plain_text(&old_text, &self.text);
        let (old_affected, new_affected) = Self::affected_ranges_from_changes(&changes);

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Format,
            operation_kind: EditorOperationKind::Format,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: new_cursor_val,
                should_animate: false,
            },
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        })
    }
}
