use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::result::{EditorEditOutcome, EditorEditResult};
use super::EditorKernel;

use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset, Utf8ByteRange};
use crate::editor::transaction::{
    diff_plain_text,
    AnimationMode, EditorTransactionCause,
};

impl EditorKernel {
    pub(crate) fn apply_undo(
        &mut self,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let entry = match self.undo_stack.pop() {
            Some(e) => e,
            None => return EditorEditOutcome::NoChange(self.noop_result(base_revision, old_cursor, old_selection)),
        };

        let old_text = self.text.clone();
        self.text = entry.old_text.clone();
        self.cursor = Utf8ByteOffset::unchecked(entry.old_cursor);
        self.selection_anchor = Utf8ByteOffset::unchecked(entry.old_cursor);
        self.revision = self.revision.next();
        self.composition_session = None;

        self.redo_stack.push(entry);

        let new_cursor_val = self.cursor;
        let new_revision = self.revision;
        let new_selection = Utf8ByteRange::new(new_cursor_val.value(), new_cursor_val.value()).unwrap();

        let (replace_range, inserted_text) = Self::compute_single_patch(&old_text, &self.text);

        let display_patches = if replace_range.start.value() < replace_range.end.value() || !inserted_text.is_empty() {
            vec![DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: replace_range,
                inserted_text,
                resulting_selection_byte_range: new_selection,
            }]
        } else {
            vec![]
        };

        let changes = diff_plain_text(&old_text, &self.text);
        let (old_affected, new_affected) = Self::affected_ranges_from_changes(&changes);

        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            AnimationMode::SnapshotAnimation
        };

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Undo,
            operation_kind: EditorOperationKind::Replace,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: new_cursor_val,
                should_animate: self.animation_enabled && old_cursor != new_cursor_val,
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

    pub(crate) fn apply_redo(
        &mut self,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let entry = match self.redo_stack.pop() {
            Some(e) => e,
            None => return EditorEditOutcome::NoChange(self.noop_result(base_revision, old_cursor, old_selection)),
        };

        let old_text = self.text.clone();
        self.text = entry.new_text.clone();
        self.cursor = Utf8ByteOffset::unchecked(entry.new_cursor);
        self.selection_anchor = Utf8ByteOffset::unchecked(entry.new_cursor);
        self.revision = self.revision.next();
        self.composition_session = None;

        self.undo_stack.push(entry);

        let new_cursor_val = self.cursor;
        let new_revision = self.revision;
        let new_selection = Utf8ByteRange::new(new_cursor_val.value(), new_cursor_val.value()).unwrap();

        let (replace_range, inserted_text) = Self::compute_single_patch(&old_text, &self.text);

        let display_patches = if replace_range.start.value() < replace_range.end.value() || !inserted_text.is_empty() {
            vec![DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: replace_range,
                inserted_text,
                resulting_selection_byte_range: new_selection,
            }]
        } else {
            vec![]
        };

        let changes = diff_plain_text(&old_text, &self.text);
        let (old_affected, new_affected) = Self::affected_ranges_from_changes(&changes);

        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            AnimationMode::SnapshotAnimation
        };

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Redo,
            operation_kind: EditorOperationKind::Replace,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: new_cursor_val,
                should_animate: self.animation_enabled && old_cursor != new_cursor_val,
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
