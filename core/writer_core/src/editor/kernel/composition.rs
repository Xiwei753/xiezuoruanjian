use super::result::{EditorContentDelta, EditorEditOutcome, EditorEditResult};
use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::{CompositionSessionState, EditorKernel, TextEditDelta, UndoEntry};

use crate::editor::strong_types::{
    EditorRevision, EditorSessionGeneration, EditorSessionId, Utf16CodeUnitOffset, Utf8ByteOffset,
    Utf8ByteRange,
};
use crate::editor::transaction::{
    classify_composition_visual, AnimationMode, CompositionOperationKind, EditorTransactionCause,
    OffsetMap,
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
        // Session-divergence recovery: if a composition session already exists, the
        // platform has lost track of it (e.g. an aborted IME interaction, or a soft reset
        // that cleared the adapter's composition state without cancelling the kernel
        // session). A stale session would reject every subsequent plain commit
        // (StaleRevision) and block all future compositions, so a new begin is
        // authoritative: drop the stale session and start fresh with the new range.
        self.composition_session = None;
        if replace_start > replace_end_exclusive {
            return EditorEditOutcome::InvalidRange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }
        if replace_start > self.text.byte_len() || replace_end_exclusive > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }
        if !self.text.is_char_boundary(replace_start)
            || !self.text.is_char_boundary(replace_end_exclusive)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }

        let session_id = self.next_composition_session_id;
        self.next_composition_session_id =
            EditorSessionId::new(session_id.value().saturating_add(1));

        self.composition_session = Some(CompositionSessionState {
            session_id,
            base_revision,
            generation: EditorSessionGeneration::initial(),
            replace_start: Utf8ByteOffset::unchecked(replace_start),
            replace_end_exclusive: Utf8ByteOffset::unchecked(replace_end_exclusive),
            preedit_text: String::new(),
            preedit_cursor_utf16: Utf16CodeUnitOffset::default(),
        });

        let new_selection =
            Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
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
                offset_map: None,
            },
            content_delta: EditorContentDelta::default(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_update_composition(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        new_preedit_text: &str,
        new_preedit_cursor_utf16: Utf16CodeUnitOffset,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let session = match &mut self.composition_session {
            Some(s)
                if s.session_id.value() == composition_session_id
                    && s.generation.value() == composition_generation
                    && s.base_revision == base_revision =>
            {
                s
            }
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        let old_preedit_text = session.preedit_text.clone();
        let replace_start = session.replace_start.value();

        session.preedit_text = new_preedit_text.to_string();
        session.preedit_cursor_utf16 = new_preedit_cursor_utf16;
        session.generation = session.generation.next();

        let classification = classify_composition_visual(
            &old_preedit_text,
            new_preedit_text,
            replace_start,
            replace_start,
            CompositionOperationKind::Update,
            self.animation_enabled,
        );

        let new_selection =
            Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
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
                old_affected_byte_ranges: classification.old_affected_byte_ranges,
                new_affected_byte_ranges: classification.new_affected_byte_ranges,
                animation_mode: classification.animation_mode,
                duration_ms: self.animation_duration_ms,
                coordinated_cursor: CoordinatedCursor {
                    old_offset: old_cursor,
                    new_offset: self.cursor,
                    should_animate: self.animation_enabled && old_cursor != self.cursor,
                },
                offset_map: None,
            },
            content_delta: EditorContentDelta::default(),
        })
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub(crate) fn apply_finish_composition(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let session = match &self.composition_session {
            Some(s)
                if s.session_id.value() == composition_session_id
                    && s.generation.value() == composition_generation
                    && s.base_revision == base_revision =>
            {
                s.clone()
            }
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        if session.preedit_text.is_empty() {
            self.composition_session = None;
            let new_selection =
                Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
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
                    offset_map: None,
                },
                content_delta: EditorContentDelta::default(),
            });
        }

        let replace_start = session.replace_start.value();
        let replace_end = session.replace_end_exclusive.value();
        let committed_text = session.preedit_text.clone();
        let preedit_cursor_utf16 = session.preedit_cursor_utf16.value();

        if replace_start > self.text.byte_len() || replace_end > self.text.byte_len() {
            self.composition_session = None;
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }

        // #624 评论8：先取局部删除文本，再局部 Rope replace，不 clone 全文。
        let deleted_text = self.text.byte_slice(replace_start..replace_end).to_string();
        self.text
            .replace(replace_start..replace_end, &committed_text);
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
        let resulting_cursor =
            Self::clamp_to_char_boundary(&self.text, resulting_cursor_before_clamp);
        let selection_was_adjusted = resulting_cursor != resulting_cursor_before_clamp
            || preedit_cursor_utf16 != preedit_cursor_utf16_clamped;
        self.cursor = Utf8ByteOffset::unchecked(resulting_cursor);
        self.selection_anchor = Utf8ByteOffset::unchecked(resulting_cursor);
        self.composition_session = None;

        let new_selection = Utf8ByteRange::point(resulting_cursor);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::from_ordered(replace_start, replace_end),
            new_range: Utf8ByteRange::from_start_len(replace_start, committed_text.len()),
            deleted_text: deleted_text.clone(),
            inserted_text: committed_text.clone(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection,
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(replace_start, replace_end),
            inserted_text: committed_text.clone(),
            resulting_selection_byte_range: new_selection,
        }];

        let classification = classify_composition_visual(
            &committed_text,
            &committed_text,
            replace_start,
            replace_start,
            CompositionOperationKind::Commit,
            self.animation_enabled,
        );

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
                old_affected_byte_ranges: classification.old_affected_byte_ranges,
                new_affected_byte_ranges: classification.new_affected_byte_ranges,
                animation_mode: classification.animation_mode,
                duration_ms: self.animation_duration_ms,
                coordinated_cursor: CoordinatedCursor {
                    old_offset: old_cursor,
                    new_offset: Utf8ByteOffset::unchecked(resulting_cursor),
                    should_animate: self.animation_enabled
                        && old_cursor.value() != resulting_cursor,
                },
                // #624 评论8：单次 composition commit 从 delta 直接构造 offset map。
                offset_map: Some(OffsetMap::from_single_edit(
                    self.text.byte_len() - committed_text.len() + (replace_end - replace_start),
                    (replace_start, replace_end),
                    committed_text.len(),
                )),
            },
            content_delta: EditorContentDelta::from_texts(&committed_text, &deleted_text),
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
            Some(s)
                if s.session_id.value() == composition_session_id
                    && s.generation.value() == composition_generation
                    && s.base_revision == base_revision =>
            {
                s.clone()
            }
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        let replace_start = session.replace_start.value();
        let replace_end = session.replace_end_exclusive.value();

        if replace_start != replace_end
            && (replace_start > self.text.byte_len() || replace_end > self.text.byte_len())
        {
            self.composition_session = None;
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }

        self.composition_session = None;

        let classification = classify_composition_visual(
            &session.preedit_text,
            "",
            replace_start,
            replace_end,
            CompositionOperationKind::Cancel,
            self.animation_enabled,
        );

        let new_selection =
            Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
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
                old_affected_byte_ranges: classification.old_affected_byte_ranges,
                new_affected_byte_ranges: classification.new_affected_byte_ranges,
                animation_mode: classification.animation_mode,
                duration_ms: self.animation_duration_ms,
                coordinated_cursor: CoordinatedCursor {
                    old_offset: old_cursor,
                    new_offset: self.cursor,
                    should_animate: self.animation_enabled && old_cursor != self.cursor,
                },
                offset_map: None,
            },
            content_delta: EditorContentDelta::default(),
        })
    }

    pub fn composition_session_info(&self) -> Option<(u64, u64, u64)> {
        self.composition_session.as_ref().map(|s| {
            (
                s.session_id.value(),
                s.base_revision.value(),
                s.generation.value(),
            )
        })
    }

    // #629 R8: composition 专用 grapheme 语义操作。
    // 以下四个方法只改 composition session 的 preeditText / preeditCursorUtf16 / generation；
    // 不修改 committed 正文，不把 raw platform event 带入 Core。
    // grapheme 边界由 Core 的 unicode_segmentation 裁判。

    pub(crate) fn apply_composition_move_grapheme_left(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let session = match &mut self.composition_session {
            Some(s)
                if s.session_id.value() == composition_session_id
                    && s.generation.value() == composition_generation
                    && s.base_revision == base_revision =>
            {
                s
            }
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        let preedit = session.preedit_text.clone();
        let cursor_utf16 = session.preedit_cursor_utf16.value();

        // 将 UTF-16 cursor 转换为 UTF-8 byte offset
        let cursor_byte = Self::utf16_to_byte_offset(&preedit, cursor_utf16);

        if cursor_byte == 0 {
            // 已在 preedit 最左端，no-op（但 generation 不变，直接返回）
            let new_selection =
                Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
            return EditorEditOutcome::Applied(EditorEditResult {
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
                    offset_map: None,
                },
                content_delta: EditorContentDelta::default(),
            });
        }

        // 在 preedit 文本中找到前一个 grapheme cluster 边界
        let prev_boundary_byte = Self::previous_grapheme_boundary_on_str(&preedit, cursor_byte);
        let new_cursor_utf16 = Self::byte_to_utf16_offset(&preedit, prev_boundary_byte);

        session.preedit_cursor_utf16 = Utf16CodeUnitOffset::unchecked(new_cursor_utf16);
        session.generation = session.generation.next();

        let new_selection =
            Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
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
                offset_map: None,
            },
            content_delta: EditorContentDelta::default(),
        })
    }

    pub(crate) fn apply_composition_move_grapheme_right(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let session = match &mut self.composition_session {
            Some(s)
                if s.session_id.value() == composition_session_id
                    && s.generation.value() == composition_generation
                    && s.base_revision == base_revision =>
            {
                s
            }
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        let preedit = session.preedit_text.clone();
        let cursor_utf16 = session.preedit_cursor_utf16.value();
        let preedit_utf16_len: usize = preedit.chars().map(|c| c.len_utf16()).sum();

        if cursor_utf16 >= preedit_utf16_len {
            // 已在 preedit 最右端，no-op
            let new_selection =
                Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
            return EditorEditOutcome::Applied(EditorEditResult {
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
                    offset_map: None,
                },
                content_delta: EditorContentDelta::default(),
            });
        }

        let cursor_byte = Self::utf16_to_byte_offset(&preedit, cursor_utf16);
        let next_boundary_byte = Self::next_grapheme_boundary_on_str(&preedit, cursor_byte);
        let new_cursor_utf16 = Self::byte_to_utf16_offset(&preedit, next_boundary_byte);

        session.preedit_cursor_utf16 = Utf16CodeUnitOffset::unchecked(new_cursor_utf16);
        session.generation = session.generation.next();

        let new_selection =
            Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
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
                offset_map: None,
            },
            content_delta: EditorContentDelta::default(),
        })
    }

    pub(crate) fn apply_composition_delete_grapheme_backward(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let session = match &mut self.composition_session {
            Some(s)
                if s.session_id.value() == composition_session_id
                    && s.generation.value() == composition_generation
                    && s.base_revision == base_revision =>
            {
                s
            }
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        let preedit = session.preedit_text.clone();
        let cursor_utf16 = session.preedit_cursor_utf16.value();
        let cursor_byte = Self::utf16_to_byte_offset(&preedit, cursor_utf16);

        if cursor_byte == 0 {
            // preedit 已空或光标在最左端，no-op
            let new_selection =
                Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
            return EditorEditOutcome::Applied(EditorEditResult {
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
                    offset_map: None,
                },
                content_delta: EditorContentDelta::default(),
            });
        }

        let prev_boundary_byte = Self::previous_grapheme_boundary_on_str(&preedit, cursor_byte);
        // 删除 [prev_boundary_byte, cursor_byte) 区间的文本
        let mut new_preedit =
            String::with_capacity(preedit.len() - (cursor_byte - prev_boundary_byte));
        new_preedit.push_str(&preedit[..prev_boundary_byte]);
        new_preedit.push_str(&preedit[cursor_byte..]);

        let new_cursor_utf16 = Self::byte_to_utf16_offset(&new_preedit, prev_boundary_byte);
        session.preedit_text = new_preedit;
        session.preedit_cursor_utf16 = Utf16CodeUnitOffset::unchecked(new_cursor_utf16);
        session.generation = session.generation.next();

        let new_selection =
            Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
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
                offset_map: None,
            },
            content_delta: EditorContentDelta::default(),
        })
    }

    pub(crate) fn apply_composition_delete_grapheme_forward(
        &mut self,
        composition_session_id: u64,
        composition_generation: u64,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let session = match &mut self.composition_session {
            Some(s)
                if s.session_id.value() == composition_session_id
                    && s.generation.value() == composition_generation
                    && s.base_revision == base_revision =>
            {
                s
            }
            _ => return EditorEditOutcome::StaleRevision(self.stale_session_result()),
        };

        let preedit = session.preedit_text.clone();
        let cursor_utf16 = session.preedit_cursor_utf16.value();
        let preedit_utf16_len: usize = preedit.chars().map(|c| c.len_utf16()).sum();

        if cursor_utf16 >= preedit_utf16_len {
            // preedit 已空或光标在最右端，no-op
            let new_selection =
                Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
            return EditorEditOutcome::Applied(EditorEditResult {
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
                    offset_map: None,
                },
                content_delta: EditorContentDelta::default(),
            });
        }

        let cursor_byte = Self::utf16_to_byte_offset(&preedit, cursor_utf16);
        let next_boundary_byte = Self::next_grapheme_boundary_on_str(&preedit, cursor_byte);
        // 删除 [cursor_byte, next_boundary_byte) 区间的文本
        let mut new_preedit =
            String::with_capacity(preedit.len() - (next_boundary_byte - cursor_byte));
        new_preedit.push_str(&preedit[..cursor_byte]);
        new_preedit.push_str(&preedit[next_boundary_byte..]);

        let new_cursor_utf16 = Self::byte_to_utf16_offset(&new_preedit, cursor_byte);
        session.preedit_text = new_preedit;
        session.preedit_cursor_utf16 = Utf16CodeUnitOffset::unchecked(new_cursor_utf16);
        session.generation = session.generation.next();

        let new_selection =
            Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
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
                offset_map: None,
            },
            content_delta: EditorContentDelta::default(),
        })
    }

    // ── 辅助方法：对 &str 做 grapheme boundary 计算 ──

    /// 对 `s` 的 UTF-8 byte slice `[0, byte_offset)` 求最后一个 grapheme cluster 的起始位置。
    fn previous_grapheme_boundary_on_str(s: &str, byte_offset: usize) -> usize {
        use unicode_segmentation::UnicodeSegmentation;
        if byte_offset == 0 {
            return 0;
        }
        if byte_offset > s.len() {
            return s.len();
        }
        let prefix = &s[..byte_offset];
        match prefix.graphemes(true).next_back() {
            Some(g) => byte_offset - g.len(),
            None => 0,
        }
    }

    /// 对 `s` 从 `byte_offset` 开始求第一个 grapheme cluster 的结束位置。
    fn next_grapheme_boundary_on_str(s: &str, byte_offset: usize) -> usize {
        use unicode_segmentation::UnicodeSegmentation;
        let len = s.len();
        if byte_offset >= len {
            return len;
        }
        let suffix = &s[byte_offset..];
        match suffix.graphemes(true).next() {
            Some(g) => byte_offset + g.len(),
            None => len,
        }
    }

    /// UTF-16 code unit offset → UTF-8 byte offset。
    fn utf16_to_byte_offset(s: &str, utf16_offset: usize) -> usize {
        let mut utf16_count = 0usize;
        let mut byte_offset = 0usize;
        for ch in s.chars() {
            let ch_len_utf16 = ch.len_utf16();
            if utf16_count + ch_len_utf16 > utf16_offset {
                break;
            }
            utf16_count += ch_len_utf16;
            byte_offset += ch.len_utf8();
        }
        byte_offset.min(s.len())
    }

    /// UTF-8 byte offset → UTF-16 code unit offset。
    fn byte_to_utf16_offset(s: &str, byte_offset: usize) -> usize {
        let mut utf16_count = 0usize;
        let mut current_byte = 0usize;
        for ch in s.chars() {
            if current_byte >= byte_offset {
                break;
            }
            current_byte += ch.len_utf8();
            utf16_count += ch.len_utf16();
        }
        utf16_count
    }

    /// #629 评论6 Part B：返回当前 composition 完整状态（preedit 文本 + replace range + cursor）。
    ///
    /// 返回元组字段顺序：
    /// `(session_id, base_revision, generation, replace_byte_start, replace_byte_end_exclusive,
    ///   preedit_text, preedit_cursor_utf16)`
    ///
    /// 无活跃 composition session 时返回 None。调用方（app_service）据此构造
    /// [crate::api::types::EditorCompositionStateDto] 暴露给平台端。
    /// 平台端不复制 Core 的 composition 状态机，只消费此快照构造临时显示文本和下划线。
    #[allow(clippy::type_complexity, clippy::cast_possible_truncation)]
    pub fn composition_state(&self) -> Option<(u64, u64, u64, u32, u32, String, u32)> {
        self.composition_session.as_ref().map(|s| {
            (
                s.session_id.value(),
                s.base_revision.value(),
                s.generation.value(),
                s.replace_start.value() as u32,
                s.replace_end_exclusive.value() as u32,
                s.preedit_text.clone(),
                s.preedit_cursor_utf16.value() as u32,
            )
        })
    }
}
