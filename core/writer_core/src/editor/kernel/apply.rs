use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::result::{EditorEditOutcome, EditorEditResult};
use super::types::EditorCommand;
use super::{EditorKernel, UndoEntry};

use crate::editor::strong_types::Utf8ByteOffset;
use crate::editor::transaction::{
    choose_animation_mode, count_grapheme_clusters,
    text_contains_complex_grapheme,
    AnimationMode, EditorTransactionCause,
};

impl EditorKernel {
    pub fn apply(&mut self, command: EditorCommand) -> EditorEditOutcome {
        let base_revision = self.revision.value();

        match &command {
            EditorCommand::Insert { expected_revision, .. }
            | EditorCommand::Delete { expected_revision, .. }
            | EditorCommand::Replace { expected_revision, .. }
            | EditorCommand::SetSelection { expected_revision, .. }
            | EditorCommand::ReplaceAll { expected_revision, .. }
            | EditorCommand::InsertLineBreak { expected_revision, .. }
            | EditorCommand::Undo { expected_revision }
            | EditorCommand::Redo { expected_revision }
            | EditorCommand::CommitText { expected_revision, .. }
            | EditorCommand::DeleteSurrounding { expected_revision, .. }
            | EditorCommand::BeginComposition { expected_revision, .. }
            | EditorCommand::UpdateComposition { expected_revision, .. }
            | EditorCommand::FinishComposition { expected_revision, .. }
            | EditorCommand::CancelComposition { expected_revision, .. } => {
                if *expected_revision != base_revision {
                    return EditorEditOutcome::StaleRevision(self.stale_session_result());
                }
            }
        }

        let old_cursor = self.cursor.value();
        let old_selection = (self.selection_anchor.value(), self.cursor.value());

        match command {
            EditorCommand::Insert { byte_offset, text, cause, .. } => {
                self.apply_insert(byte_offset, &text, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::Delete { byte_start, byte_end_exclusive, deleted_text: _, cause, .. } => {
                self.apply_delete(byte_start, byte_end_exclusive, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::Replace { byte_start, byte_end_exclusive, replacement_text, original_text: _, cause, .. } => {
                self.apply_replace(byte_start, byte_end_exclusive, &replacement_text, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::SetSelection { anchor_byte_offset, head_byte_offset, .. } => {
                self.apply_set_selection(anchor_byte_offset, head_byte_offset, base_revision, old_cursor, old_selection)
            }
            EditorCommand::Undo { .. } => {
                self.apply_undo(base_revision, old_cursor, old_selection)
            }
            EditorCommand::Redo { .. } => {
                self.apply_redo(base_revision, old_cursor, old_selection)
            }
            EditorCommand::ReplaceAll { search, replacement, .. } => {
                self.apply_replace_all(&search, &replacement, base_revision, old_cursor, old_selection)
            }
            EditorCommand::InsertLineBreak { byte_offset, auto_indent_prefix, cause, .. } => {
                self.apply_insert_line_break(byte_offset, auto_indent_prefix, cause, base_revision, old_cursor, old_selection)
            }
            EditorCommand::CommitText {
                byte_start,
                byte_end_exclusive,
                replacement_text,
                resulting_selection_anchor,
                resulting_selection_head,
                composition_session_id,
                composition_base_revision,
                composition_generation,
                cause,
                ..
            } => {
                self.apply_commit_text(
                    byte_start,
                    byte_end_exclusive,
                    &replacement_text,
                    resulting_selection_anchor,
                    resulting_selection_head,
                    composition_session_id,
                    composition_base_revision,
                    composition_generation,
                    cause,
                    base_revision,
                    old_cursor,
                    old_selection,
                )
            }
            EditorCommand::DeleteSurrounding {
                before_byte_start,
                before_byte_end_exclusive,
                after_byte_start,
                after_byte_end_exclusive,
                cause,
                ..
            } => {
                self.apply_delete_surrounding(
                    before_byte_start,
                    before_byte_end_exclusive,
                    after_byte_start,
                    after_byte_end_exclusive,
                    cause,
                    base_revision,
                    old_cursor,
                    old_selection,
                )
            }
            EditorCommand::BeginComposition {
                replace_start,
                replace_end_exclusive,
                ..
            } => {
                self.apply_begin_composition(replace_start, replace_end_exclusive, base_revision, old_cursor, old_selection)
            }
            EditorCommand::UpdateComposition {
                composition_session_id,
                composition_generation,
                new_preedit_text,
                new_preedit_cursor_offset,
                ..
            } => {
                self.apply_update_composition(
                    composition_session_id,
                    composition_generation,
                    &new_preedit_text,
                    new_preedit_cursor_offset,
                    base_revision,
                    old_cursor,
                    old_selection,
                )
            }
            EditorCommand::FinishComposition {
                composition_session_id,
                composition_generation,
                ..
            } => {
                self.apply_finish_composition(composition_session_id, composition_generation, base_revision, old_cursor, old_selection)
            }
            EditorCommand::CancelComposition {
                composition_session_id,
                composition_generation,
                ..
            } => {
                self.apply_cancel_composition(composition_session_id, composition_generation, base_revision, old_cursor, old_selection)
            }
        }
    }

    fn apply_insert(
        &mut self,
        byte_offset: usize,
        text: &str,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        if byte_offset > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(byte_offset) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        self.composition_session = None;

        self.text.insert_str(byte_offset, text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_offset + text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        self.undo_stack.push(UndoEntry {
            old_text: self.text[..byte_offset].to_string() + &self.text[byte_offset + text.len()..],
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: new_cursor_val,
        });
        self.redo_stack.clear();

        let new_revision = self.revision.value();
        let new_selection = (new_cursor_val, new_cursor_val);
        let new_affected = vec![(byte_offset, byte_offset + text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (byte_offset, byte_offset),
            inserted_text: text.to_string(),
            resulting_selection_byte_range: new_selection,
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let cluster_count = count_grapheme_clusters(text);
            let contains_newline = text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                is_loading,
                is_format,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Insert,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: new_cursor_val,
                should_animate: self.animation_enabled && old_cursor != new_cursor_val && !is_loading && !is_format,
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

    fn apply_delete(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let (byte_start, byte_end_exclusive) = Self::normalize_range(byte_start, byte_end_exclusive);
        if byte_start > self.text.len() || byte_end_exclusive > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(byte_start) || !self.text.is_char_boundary(byte_end_exclusive) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if byte_start >= byte_end_exclusive {
            return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let old_text = self.text.clone();

        self.composition_session = None;

        self.text.replace_range(byte_start..byte_end_exclusive, "");
        self.revision = self.revision.next();
        self.cursor = Utf8ByteOffset::unchecked(byte_start);
        self.selection_anchor = Utf8ByteOffset::unchecked(byte_start);

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: byte_start,
        });
        self.redo_stack.clear();

        let new_revision = self.revision.value();
        let new_selection = (byte_start, byte_start);
        let old_affected = vec![(byte_start, byte_end_exclusive)];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (byte_start, byte_end_exclusive),
            inserted_text: String::new(),
            resulting_selection_byte_range: new_selection,
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let deleted_text = &old_text[byte_start..byte_end_exclusive];
            let cluster_count = count_grapheme_clusters(deleted_text);
            let contains_newline = deleted_text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(deleted_text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                is_loading,
                is_format,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Delete,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: vec![],
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: byte_start,
                should_animate: self.animation_enabled && old_cursor != byte_start && !is_loading && !is_format,
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

    #[allow(clippy::too_many_arguments)]
    fn apply_replace(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement_text: &str,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let (byte_start, byte_end_exclusive) = Self::normalize_range(byte_start, byte_end_exclusive);
        if byte_start > self.text.len() || byte_end_exclusive > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(byte_start) || !self.text.is_char_boundary(byte_end_exclusive) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let old_text = self.text.clone();

        self.composition_session = None;

        self.text.replace_range(byte_start..byte_end_exclusive, replacement_text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_start + replacement_text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: new_cursor_val,
        });
        self.redo_stack.clear();

        let new_revision = self.revision.value();
        let new_selection = (new_cursor_val, new_cursor_val);
        let old_affected = vec![(byte_start, byte_end_exclusive)];
        let new_affected = vec![(byte_start, byte_start + replacement_text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (byte_start, byte_end_exclusive),
            inserted_text: replacement_text.to_string(),
            resulting_selection_byte_range: new_selection,
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let diff_text = if !replacement_text.is_empty() {
                replacement_text
            } else {
                &old_text[byte_start..byte_end_exclusive]
            };
            let cluster_count = count_grapheme_clusters(diff_text);
            let contains_newline = diff_text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(diff_text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                is_loading,
                is_format,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Replace,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: new_cursor_val,
                should_animate: self.animation_enabled && old_cursor != new_cursor_val && !is_loading && !is_format,
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

    fn apply_insert_line_break(
        &mut self,
        byte_offset: usize,
        auto_indent_prefix: String,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        if byte_offset > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(byte_offset) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        let text = format!("\n{}", auto_indent_prefix);

        self.composition_session = None;

        self.text.insert_str(byte_offset, &text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_offset + text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        self.undo_stack.push(UndoEntry {
            old_text: self.text[..byte_offset].to_string() + &self.text[byte_offset + text.len()..],
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: new_cursor_val,
        });
        self.redo_stack.clear();

        let new_revision = self.revision.value();
        let new_selection = (new_cursor_val, new_cursor_val);
        let new_affected = vec![(byte_offset, byte_offset + text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (byte_offset, byte_offset),
            inserted_text: text.clone(),
            resulting_selection_byte_range: new_selection,
        }];

        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            let cluster_count = count_grapheme_clusters(&text);
            let contains_newline = text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(&text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                false,
                false,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Insert,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: new_cursor_val,
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

    #[allow(clippy::too_many_arguments)]
    fn apply_commit_text(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement_text: &str,
        resulting_selection_anchor: usize,
        resulting_selection_head: usize,
        composition_session_id: u64,
        composition_base_revision: u64,
        composition_generation: u64,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        if let Some(ref session) = self.composition_session {
            if session.session_id.value() != composition_session_id
                || session.base_revision.value() != composition_base_revision
                || session.generation.value() != composition_generation
            {
                return EditorEditOutcome::StaleRevision(self.stale_session_result());
            }
        } else if composition_session_id != 0 {
            return EditorEditOutcome::StaleRevision(self.stale_session_result());
        }

        let (byte_start, byte_end_exclusive) = Self::normalize_range(byte_start, byte_end_exclusive);

        if let Some(ref session) = self.composition_session {
            if byte_start != session.replace_start || byte_end_exclusive != session.replace_end_exclusive {
                return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
            }
        }

        if byte_start == byte_end_exclusive && replacement_text.is_empty() && self.composition_session.is_none() {
            return EditorEditOutcome::NoChange(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if byte_start > self.text.len() || byte_end_exclusive > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(byte_start) || !self.text.is_char_boundary(byte_end_exclusive) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let old_text = self.text.clone();

        self.text.replace_range(byte_start..byte_end_exclusive, replacement_text);
        self.revision = self.revision.next();

        let sel_anchor = Self::clamp_to_char_boundary(&self.text, resulting_selection_anchor);
        let sel_head = Self::clamp_to_char_boundary(&self.text, resulting_selection_head);
        let selection_was_adjusted = sel_anchor != resulting_selection_anchor || sel_head != resulting_selection_head;
        self.selection_anchor = Utf8ByteOffset::unchecked(sel_anchor);
        self.cursor = Utf8ByteOffset::unchecked(sel_head);

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: sel_head,
        });
        self.redo_stack.clear();
        let preedit_byte_len = self.composition_session.as_ref().map(|s| s.preedit_text.len()).unwrap_or(0);
        self.composition_session = None;

        let new_revision = self.revision.value();
        let new_selection = (sel_anchor, sel_head);
        let old_affected = if preedit_byte_len > 0 {
            vec![(byte_start, byte_start + preedit_byte_len)]
        } else {
            vec![(byte_start, byte_end_exclusive)]
        };
        let new_affected = vec![(byte_start, byte_start + replacement_text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: (byte_start, byte_end_exclusive),
            inserted_text: replacement_text.to_string(),
            resulting_selection_byte_range: new_selection,
        }];

        let cluster_count = count_grapheme_clusters(replacement_text);
        let contains_newline = replacement_text.contains('\n');
        let contains_complex = text_contains_complex_grapheme(replacement_text);
        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false, false, false, false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::CompositionCommit,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: sel_head,
                should_animate: self.animation_enabled && old_cursor != sel_head,
            },
        };

        let edit_result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
        };

        if selection_was_adjusted {
            EditorEditOutcome::AppliedWithAdjustedSelection(edit_result)
        } else {
            EditorEditOutcome::Applied(edit_result)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_delete_surrounding(
        &mut self,
        before_byte_start: usize,
        before_byte_end_exclusive: usize,
        after_byte_start: usize,
        after_byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: u64,
        old_cursor: usize,
        old_selection: (usize, usize),
    ) -> EditorEditOutcome {
        let sel_anchor = self.selection_anchor.value();
        let sel_head = self.cursor.value();
        let (sel_min, sel_max) = if sel_anchor <= sel_head { (sel_anchor, sel_head) } else { (sel_head, sel_anchor) };

        let mut patches = Vec::new();
        let mut text = self.text.clone();

        let after_range = if after_byte_start < after_byte_end_exclusive {
            Some((after_byte_start, after_byte_end_exclusive))
        } else {
            None
        };
        let before_range = if before_byte_start < before_byte_end_exclusive {
            Some((before_byte_start, before_byte_end_exclusive))
        } else {
            None
        };

        if let Some((as_, ae)) = after_range {
            if as_ > text.len() || ae > text.len() || !text.is_char_boundary(as_) || !text.is_char_boundary(ae) {
                return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
            }
            if as_ >= ae || as_ < sel_max {
                return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
            }
            text.replace_range(as_..ae, "");
            patches.push((as_, ae, String::new()));
        }

        if let Some((bs, be)) = before_range {
            if bs > text.len() || be > text.len() || !text.is_char_boundary(bs) || !text.is_char_boundary(be) {
                return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
            }
            if bs >= be || be > sel_min {
                return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
            }
            text.replace_range(bs..be, "");
            patches.push((bs, be, String::new()));
        }

        if patches.is_empty() {
            return EditorEditOutcome::NoChange(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let old_text = self.text.clone();
        self.text = text;
        self.revision = self.revision.next();
        self.composition_session = None;

        let before_deleted_len: usize = if let Some((bs, be)) = before_range {
            be.saturating_sub(bs)
        } else {
            0
        };

        let new_sel_anchor = if sel_anchor == sel_min {
            sel_min.saturating_sub(before_deleted_len)
        } else {
            sel_max.saturating_sub(before_deleted_len)
        };
        let new_sel_head = if sel_head == sel_min {
            sel_min.saturating_sub(before_deleted_len)
        } else {
            sel_max.saturating_sub(before_deleted_len)
        };
        self.selection_anchor = Utf8ByteOffset::unchecked(new_sel_anchor);
        self.cursor = Utf8ByteOffset::unchecked(new_sel_head);

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: new_sel_head,
        });
        self.redo_stack.clear();

        let new_revision = self.revision.value();
        let new_selection = (new_sel_anchor, new_sel_head);

        let (replace_range, inserted_text) = Self::compute_single_patch(&old_text, &self.text);
        let display_patches = if replace_range.0 < replace_range.1 || !inserted_text.is_empty() {
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

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Delete,
            old_affected_byte_ranges: patches.iter().map(|(s, e, _)| (*s, *e)).collect(),
            new_affected_byte_ranges: vec![],
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_byte_offset: old_cursor,
                new_byte_offset: new_sel_head,
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
