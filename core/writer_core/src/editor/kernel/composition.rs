use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::result::{EditorEditOutcome, EditorEditResult};
use super::{EditorKernel, CompositionSessionState, UndoEntry};

use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
use crate::editor::transaction::{
    choose_animation_mode, count_grapheme_clusters,
    text_contains_complex_grapheme, CompositionVisualRevision,
    CompositionUpdateTransaction, CompositionCommitOrCancelTransaction,
    AnimationMode, EditorTransactionCause,
};

impl EditorKernel {
    pub(crate) fn apply_begin_composition(
        &mut self,
        replace_start: usize,
        replace_end_exclusive: usize,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        if self.composition_session.is_some() {
            return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if replace_start > replace_end_exclusive {
            return EditorEditOutcome::InvalidRange(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if replace_start > self.text.len() || replace_end_exclusive > self.text.len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }
        if !self.text.is_char_boundary(replace_start) || !self.text.is_char_boundary(replace_end_exclusive) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let session_id = self.next_composition_session_id;
        self.next_composition_session_id = EditorSessionId::new(session_id.value().saturating_add(1));

        self.composition_session = Some(CompositionSessionState {
            session_id,
            base_revision,
            generation: EditorSessionGeneration::initial(),
            replace_start: Utf8ByteOffset::unchecked(replace_start),
            replace_end_exclusive: Utf8ByteOffset::unchecked(replace_end_exclusive),
            preedit_text: String::new(),
            preedit_cursor_utf16: 0,
        });

        let new_selection = Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::ImeComposition,
                operation_kind: EditorOperationKind::CompositionUpdate,
                old_affected_byte_ranges: vec![],
                new_affected_byte_ranges: vec![],
                animation_mode: AnimationMode::SystemSuppressed,
                duration_ms: 0,
                coordinated_cursor: CoordinatedCursor {
                    old_offset: old_cursor,
                    new_offset: self.cursor,
                    should_animate: false,
                },
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_update_composition(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        new_preedit_text: &str,
        new_preedit_cursor_offset: usize,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let session = match &mut self.composition_session {
            Some(s) if s.session_id.value() == composition_session_id
                && s.generation.value() == composition_generation
                && s.base_revision == base_revision => s,
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        let old_preedit_text = session.preedit_text.clone();
        let replace_start = session.replace_start.value();

        session.preedit_text = new_preedit_text.to_string();
        session.preedit_cursor_utf16 = new_preedit_cursor_offset;
        session.generation = session.generation.next();

        let old_affected = if old_preedit_text.is_empty() {
            vec![]
        } else {
                vec![Utf8ByteRange::from_start_len(replace_start, old_preedit_text.len())]
        };
        let new_affected = if new_preedit_text.is_empty() {
            vec![]
        } else {
            vec![Utf8ByteRange::from_start_len(replace_start, new_preedit_text.len())]
        };

        let changed_text: &str = if new_preedit_text.len() >= old_preedit_text.len() {
            new_preedit_text
        } else {
            &old_preedit_text
        };
        let cluster_count = count_grapheme_clusters(changed_text);
        let contains_newline = changed_text.contains('\n');
        let contains_complex = text_contains_complex_grapheme(changed_text);
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

        let new_selection = Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::ImeComposition,
                operation_kind: EditorOperationKind::CompositionUpdate,
                old_affected_byte_ranges: old_affected,
                new_affected_byte_ranges: new_affected,
                animation_mode,
                duration_ms: self.animation_duration_ms,
                coordinated_cursor: CoordinatedCursor {
                    old_offset: old_cursor,
                    new_offset: self.cursor,
                    should_animate: self.animation_enabled && old_cursor != self.cursor,
                },
            },
        })
    }

    pub(crate) fn apply_finish_composition(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let session = match &self.composition_session {
            Some(s) if s.session_id.value() == composition_session_id
                && s.generation.value() == composition_generation
                && s.base_revision == base_revision => s.clone(),
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        if session.preedit_text.is_empty() {
            self.composition_session = None;
            let new_selection = Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
            return EditorEditOutcome::Applied(EditorEditResult {
                transaction_id: self.take_transaction_id(),
                base_revision,
                new_revision: self.revision,
                display_patches: vec![],
                old_selection_byte_range: old_selection,
                new_selection_byte_range: new_selection,
                visual_intent: EditorVisualIntent {
                    cause: EditorTransactionCause::TypingCommit,
                    operation_kind: EditorOperationKind::CompositionCommit,
                    old_affected_byte_ranges: vec![],
                    new_affected_byte_ranges: vec![],
                    animation_mode: AnimationMode::SystemSuppressed,
                    duration_ms: 0,
                    coordinated_cursor: CoordinatedCursor {
                        old_offset: old_cursor,
                        new_offset: self.cursor,
                        should_animate: false,
                    },
                },
            });
        }

        let replace_start = session.replace_start.value();
        let replace_end = session.replace_end_exclusive.value();
        let committed_text = session.preedit_text.clone();
        let preedit_cursor_utf16 = session.preedit_cursor_utf16;

        if replace_start > self.text.len() || replace_end > self.text.len() {
            self.composition_session = None;
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        let old_text = self.text.clone();
        self.text.replace_range(replace_start..replace_end, &committed_text);
        self.revision = self.revision.next();

        let committed_utf16_len: usize = committed_text.chars().map(|c| c.len_utf16()).sum();
        let preedit_cursor_utf16_clamped = preedit_cursor_utf16.min(committed_utf16_len);

        let resulting_cursor_before_clamp = if preedit_cursor_utf16_clamped > 0 {
            let mut utf16_count = 0usize;
            let mut byte_offset = 0usize;
            for ch in committed_text.chars() {
                if utf16_count >= preedit_cursor_utf16_clamped {
                    break;
                }
                let ch_len_utf16 = ch.len_utf16();
                utf16_count += ch_len_utf16;
                byte_offset += ch.len_utf8();
            }
            replace_start + byte_offset
        } else {
            replace_start
        };
        let resulting_cursor = Self::clamp_to_char_boundary(&self.text, resulting_cursor_before_clamp);
        let selection_was_adjusted = resulting_cursor != resulting_cursor_before_clamp
            || preedit_cursor_utf16 != preedit_cursor_utf16_clamped;
        self.cursor = Utf8ByteOffset::unchecked(resulting_cursor);
        self.selection_anchor = Utf8ByteOffset::unchecked(resulting_cursor);
        self.composition_session = None;

        self.undo_stack.push(UndoEntry {
            old_text: old_text.clone(),
            new_text: self.text.clone(),
            old_cursor,
            new_cursor: Utf8ByteOffset::unchecked(resulting_cursor),
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = Utf8ByteRange::point(resulting_cursor);

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(replace_start, replace_end),
            inserted_text: committed_text.clone(),
            resulting_selection_byte_range: new_selection,
        }];

        let cluster_count = count_grapheme_clusters(&committed_text);
        let contains_newline = committed_text.contains('\n');
        let contains_complex = text_contains_complex_grapheme(&committed_text);
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

        let edit_result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::TypingCommit,
                operation_kind: EditorOperationKind::CompositionCommit,
                old_affected_byte_ranges: vec![Utf8ByteRange::from_start_len(replace_start, committed_text.len())],
                new_affected_byte_ranges: vec![Utf8ByteRange::from_start_len(replace_start, committed_text.len())],
                animation_mode,
                duration_ms: self.animation_duration_ms,
                coordinated_cursor: CoordinatedCursor {
                    old_offset: old_cursor,
                    new_offset: Utf8ByteOffset::unchecked(resulting_cursor),
                    should_animate: self.animation_enabled && old_cursor.value() != resulting_cursor,
                },
            },
        };

        if selection_was_adjusted {
            EditorEditOutcome::AppliedWithAdjustedSelection(edit_result)
        } else {
            EditorEditOutcome::Applied(edit_result)
        }
    }

    pub(crate) fn apply_cancel_composition(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let session = match &self.composition_session {
            Some(s) if s.session_id.value() == composition_session_id
                && s.generation.value() == composition_generation
                && s.base_revision == base_revision => s.clone(),
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        let replace_start = session.replace_start.value();
        let replace_end = session.replace_end_exclusive.value();

        if replace_start != replace_end && (replace_start > self.text.len() || replace_end > self.text.len()) {
            self.composition_session = None;
            return EditorEditOutcome::InvalidOffset(self.noop_result(base_revision, old_cursor, old_selection));
        }

        self.composition_session = None;

        let preedit_byte_len = session.preedit_text.len();
        let old_affected = if preedit_byte_len > 0 {
            vec![Utf8ByteRange::from_start_len(replace_start, preedit_byte_len)]
        } else if replace_start != replace_end {
            vec![Utf8ByteRange::from_ordered(replace_start, replace_end)]
        } else {
            vec![]
        };

        let animation_mode = if !self.animation_enabled || old_affected.is_empty() {
            AnimationMode::SystemSuppressed
        } else {
            AnimationMode::ClusterAnimation
        };

        let new_selection = Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent: EditorVisualIntent {
                cause: EditorTransactionCause::ImeComposition,
                operation_kind: EditorOperationKind::CompositionCancel,
                old_affected_byte_ranges: old_affected,
                new_affected_byte_ranges: vec![],
                animation_mode,
                duration_ms: self.animation_duration_ms,
                coordinated_cursor: CoordinatedCursor {
                    old_offset: old_cursor,
                    new_offset: self.cursor,
                    should_animate: self.animation_enabled && old_cursor != self.cursor,
                },
            },
        })
    }

    pub fn composition_session_info(&self) -> Option<(u64, u64, u64)> {
        self.composition_session.as_ref().map(|s| (s.session_id.value(), s.base_revision.value(), s.generation.value()))
    }

    pub fn composition_update(
        &mut self,
        composition_replace_range: Option<(usize, usize)>,
        old_preedit_text: &str,
        new_preedit_text: &str,
    ) -> CompositionUpdateTransaction {
        let mut engine = super::super::transaction::EditorEngine::with_animation_limits(
            self.max_animated_chars,
            self.animation_duration_ms,
        );
        engine.composition_update_transaction(
            &self.text,
            composition_replace_range,
            old_preedit_text,
            new_preedit_text,
        )
    }

    pub fn composition_update_visual_intent(
        &self,
        composition_replace_range: Option<(usize, usize)>,
        old_preedit_text: &str,
        new_preedit_text: &str,
    ) -> EditorVisualIntent {
        let replace_start = composition_replace_range
            .map(|(s, _)| s)
            .unwrap_or(self.cursor.value());
        let new_end = replace_start + new_preedit_text.len();

        let changed_text = if new_preedit_text.len() >= old_preedit_text.len() {
            new_preedit_text
        } else {
            old_preedit_text
        };
        let cluster_count = count_grapheme_clusters(changed_text);
        let contains_newline = changed_text.contains('\n');
        let contains_complex = text_contains_complex_grapheme(changed_text);
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

        EditorVisualIntent {
            cause: EditorTransactionCause::ImeComposition,
            operation_kind: EditorOperationKind::CompositionUpdate,
            old_affected_byte_ranges: if old_preedit_text.is_empty() {
                vec![]
            } else {
            vec![Utf8ByteRange::from_start_len(replace_start, old_preedit_text.len())]
            },
            new_affected_byte_ranges: if new_preedit_text.is_empty() {
                vec![]
            } else {
                vec![Utf8ByteRange::from_start_len(replace_start, new_preedit_text.len())]
            },
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: self.cursor,
                new_offset: Utf8ByteOffset::unchecked(new_end),
                should_animate: self.animation_enabled,
            },
        }
    }

    pub fn composition_commit_or_cancel(
        &mut self,
        composition_revision: CompositionVisualRevision,
        committed_text_after: &str,
        is_commit: bool,
    ) -> CompositionCommitOrCancelTransaction {
        let mut engine = super::super::transaction::EditorEngine::with_animation_limits(
            self.max_animated_chars,
            self.animation_duration_ms,
        );
        engine.composition_commit_or_cancel_transaction(
            &self.text,
            committed_text_after,
            composition_revision,
            is_commit,
        )
    }
}
