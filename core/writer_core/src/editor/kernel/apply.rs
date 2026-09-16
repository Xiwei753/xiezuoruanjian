use super::result::{make_selection, EditorContentDelta, EditorEditOutcome, EditorEditResult};
use super::types::EditorCommand;
use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::{EditorKernel, TextEditDelta, UndoEntry};

use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset, Utf8ByteRange};
use crate::editor::transaction::{
    choose_animation_mode, compute_animation_units_from_slices, count_grapheme_clusters,
    text_contains_complex_grapheme, AnimationMode, AnimationTextSlice, EditorTransactionCause,
    OffsetMap,
};

impl EditorKernel {
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn apply(&mut self, command: EditorCommand) -> EditorEditOutcome {
        let base_revision = self.revision;

        match &command {
            EditorCommand::Insert {
                expected_revision, ..
            }
            | EditorCommand::Delete {
                expected_revision, ..
            }
            | EditorCommand::Replace {
                expected_revision, ..
            }
            | EditorCommand::SetSelection {
                expected_revision, ..
            }
            | EditorCommand::ReplaceAll {
                expected_revision, ..
            }
            | EditorCommand::InsertLineBreak {
                expected_revision, ..
            }
            | EditorCommand::Undo { expected_revision }
            | EditorCommand::Redo { expected_revision }
            | EditorCommand::CommitText {
                expected_revision, ..
            }
            | EditorCommand::DeleteSurrounding {
                expected_revision, ..
            }
            | EditorCommand::ImeCommit {
                expected_revision, ..
            }
            | EditorCommand::BeginComposition {
                expected_revision, ..
            }
            | EditorCommand::UpdateComposition {
                expected_revision, ..
            }
            | EditorCommand::FinishComposition {
                expected_revision, ..
            }
            | EditorCommand::CancelComposition {
                expected_revision, ..
            }
            | EditorCommand::CompositionMoveGraphemeLeft {
                expected_revision, ..
            }
            | EditorCommand::CompositionMoveGraphemeRight {
                expected_revision, ..
            }
            | EditorCommand::CompositionDeleteGraphemeBackward {
                expected_revision, ..
            }
            | EditorCommand::CompositionDeleteGraphemeForward {
                expected_revision, ..
            } => {
                if *expected_revision != base_revision {
                    return EditorEditOutcome::StaleRevision(self.stale_session_result());
                }
            }
        }

        let old_cursor = self.cursor;
        let old_selection_anchor = self.selection_anchor.value();
        let old_selection_head = self.cursor.value();

        match command {
            EditorCommand::Insert {
                byte_offset,
                text,
                cause,
                ..
            } => self.apply_insert(
                byte_offset.value(),
                &text,
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::Delete {
                byte_range,
                deleted_text: _,
                cause,
                ..
            } => self.apply_delete(
                byte_range.start().value(),
                byte_range.end().value(),
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::Replace {
                byte_range,
                replacement_text,
                original_text: _,
                cause,
                ..
            } => self.apply_replace(
                byte_range.start().value(),
                byte_range.end().value(),
                &replacement_text,
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::SetSelection { anchor, head, .. } => self.apply_set_selection(
                anchor.value(),
                head.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::Undo { .. } => self.apply_undo(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::Redo { .. } => self.apply_redo(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::ReplaceAll {
                search,
                replacement,
                ..
            } => self.apply_replace_all(
                &search,
                &replacement,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::InsertLineBreak {
                byte_offset,
                auto_indent_enabled,
                cause,
                ..
            } => self.apply_insert_line_break(
                byte_offset.value(),
                auto_indent_enabled,
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::CommitText {
                byte_range,
                replacement_text,
                resulting_selection_anchor,
                resulting_selection_head,
                composition_session_id,
                composition_base_revision,
                composition_generation,
                cause,
                ..
            } => self.apply_commit_text(
                byte_range.start().value(),
                byte_range.end().value(),
                &replacement_text,
                resulting_selection_anchor.value(),
                resulting_selection_head.value(),
                composition_session_id.value(),
                composition_base_revision.value(),
                composition_generation.value(),
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::DeleteSurrounding {
                before_byte_range,
                after_byte_range,
                cause,
                ..
            } => self.apply_delete_surrounding(
                before_byte_range.start().value(),
                before_byte_range.end().value(),
                after_byte_range.start().value(),
                after_byte_range.end().value(),
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::ImeCommit {
                selection_byte_range,
                inserted_text,
                cause,
                ..
            } => self.apply_ime_commit(
                selection_byte_range.start().value(),
                selection_byte_range.end().value(),
                &inserted_text,
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::BeginComposition { replace_range, .. } => self.apply_begin_composition(
                replace_range.start().value(),
                replace_range.end().value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::UpdateComposition {
                composition_session_id,
                composition_generation,
                new_preedit_text,
                new_preedit_cursor_utf16,
                ..
            } => self.apply_update_composition(
                composition_session_id.value(),
                composition_generation.value(),
                &new_preedit_text,
                new_preedit_cursor_utf16,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::FinishComposition {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_finish_composition(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::CancelComposition {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_cancel_composition(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            //  R8: composition 专用 grapheme 语义操作
            EditorCommand::CompositionMoveGraphemeLeft {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_composition_move_grapheme_left(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::CompositionMoveGraphemeRight {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_composition_move_grapheme_right(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::CompositionDeleteGraphemeBackward {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_composition_delete_grapheme_backward(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::CompositionDeleteGraphemeForward {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_composition_delete_grapheme_forward(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn apply_insert(
        &mut self,
        byte_offset: usize,
        text: &str,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        if byte_offset > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(byte_offset) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        self.composition_session = None;

        // 局部 Rope edit，不 clone 全文。
        self.text.insert(byte_offset, text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_offset + text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        let new_selection = make_selection(new_cursor_val, new_cursor_val);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::point(byte_offset),
            new_range: Utf8ByteRange::from_start_len(byte_offset, text.len()),
            deleted_text: String::new(),
            inserted_text: text.to_string(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_affected = vec![Utf8ByteRange::from_start_len(byte_offset, text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::point(byte_offset),
            inserted_text: text.to_string(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
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

        let (old_animation_units, new_animation_units) = compute_animation_units_from_slices(
            animation_mode,
            &[],
            &[AnimationTextSlice {
                absolute_start: byte_offset,
                text,
            }],
            &[],
            &new_affected,
        );

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Insert,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(new_cursor_val),
                should_animate: self.animation_enabled
                    && old_cursor.value() != new_cursor_val
                    && !is_loading
                    && !is_format,
            },
            // 单次编辑从 delta 直接构造 offset map，不再扫全文。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - text.len(),
                (byte_offset, byte_offset),
                text.len(),
            )),
            old_animation_units,
            new_animation_units,
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            visual_intent,
            content_delta: EditorContentDelta::from_inserted_text(text),
        })
    }

    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    fn apply_delete(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        let (byte_start, byte_end_exclusive) =
            Self::normalize_range(byte_start, byte_end_exclusive);
        if byte_start > self.text.byte_len() || byte_end_exclusive > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(byte_start)
            || !self.text.is_char_boundary(byte_end_exclusive)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if byte_start >= byte_end_exclusive {
            return EditorEditOutcome::InvalidRange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // 先取局部删除文本，再局部 Rope delete，不 clone 全文。
        let deleted_text = self
            .text
            .byte_slice(byte_start..byte_end_exclusive)
            .to_string();

        self.composition_session = None;

        self.text.delete(byte_start..byte_end_exclusive);
        self.revision = self.revision.next();
        self.cursor = Utf8ByteOffset::unchecked(byte_start);
        self.selection_anchor = Utf8ByteOffset::unchecked(byte_start);

        let new_selection = make_selection(byte_start, byte_start);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            new_range: Utf8ByteRange::point(byte_start),
            deleted_text: deleted_text.clone(),
            inserted_text: String::new(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let old_affected = vec![Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive)];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            inserted_text: String::new(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let cluster_count = count_grapheme_clusters(&deleted_text);
            let contains_newline = deleted_text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(&deleted_text);
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

        let (old_animation_units, new_animation_units) = compute_animation_units_from_slices(
            animation_mode,
            &[AnimationTextSlice {
                absolute_start: byte_start,
                text: &deleted_text,
            }],
            &[],
            &old_affected,
            &[],
        );

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Delete,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: vec![],
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(byte_start),
                should_animate: self.animation_enabled
                    && old_cursor.value() != byte_start
                    && !is_loading
                    && !is_format,
            },
            // 单次删除从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() + (byte_end_exclusive - byte_start),
                (byte_start, byte_end_exclusive),
                0,
            )),
            old_animation_units,
            new_animation_units,
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            visual_intent,
            content_delta: EditorContentDelta::from_deleted_text(&deleted_text),
        })
    }

    #[allow(clippy::too_many_arguments)]
    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting
    )]
    fn apply_replace(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement_text: &str,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        let (byte_start, byte_end_exclusive) =
            Self::normalize_range(byte_start, byte_end_exclusive);
        if byte_start > self.text.byte_len() || byte_end_exclusive > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(byte_start)
            || !self.text.is_char_boundary(byte_end_exclusive)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // 先取局部删除文本，再局部 Rope replace，不 clone 全文。
        let deleted_text = self
            .text
            .byte_slice(byte_start..byte_end_exclusive)
            .to_string();

        self.composition_session = None;

        self.text
            .replace(byte_start..byte_end_exclusive, replacement_text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_start + replacement_text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        let new_selection = make_selection(new_cursor_val, new_cursor_val);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            new_range: Utf8ByteRange::from_start_len(byte_start, replacement_text.len()),
            deleted_text: deleted_text.clone(),
            inserted_text: replacement_text.to_string(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = make_selection(new_cursor_val, new_cursor_val);
        let old_affected = vec![Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive)];
        let new_affected = vec![Utf8ByteRange::from_start_len(
            byte_start,
            replacement_text.len(),
        )];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            inserted_text: replacement_text.to_string(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let diff_text = if !replacement_text.is_empty() {
                replacement_text
            } else {
                &deleted_text
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

        let operation_kind = if byte_start == byte_end_exclusive {
            EditorOperationKind::Insert
        } else if replacement_text.is_empty() {
            EditorOperationKind::Delete
        } else {
            EditorOperationKind::Replace
        };

        let (old_animation_units, new_animation_units) = compute_animation_units_from_slices(
            animation_mode,
            &[AnimationTextSlice {
                absolute_start: byte_start,
                text: &deleted_text,
            }],
            &[AnimationTextSlice {
                absolute_start: byte_start,
                text: replacement_text,
            }],
            &old_affected,
            &new_affected,
        );

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(new_cursor_val),
                should_animate: self.animation_enabled
                    && old_cursor.value() != new_cursor_val
                    && !is_loading
                    && !is_format,
            },
            // 单次替换从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - replacement_text.len() + (byte_end_exclusive - byte_start),
                (byte_start, byte_end_exclusive),
                replacement_text.len(),
            )),
            old_animation_units,
            new_animation_units,
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            visual_intent,
            content_delta: EditorContentDelta::from_texts(replacement_text, &deleted_text),
        })
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn apply_insert_line_break(
        &mut self,
        byte_offset: usize,
        auto_indent_enabled: bool,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        if byte_offset > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(byte_offset) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        // #606: Core 端 auto-indent — 从正文按 UTF-8 安全边界找到当前逻辑行开头，
        // 读取已有前导空白（空格/Tab），构造插入文本为 \n + prefix。
        // auto_indent_enabled 为 false 时只插入 \n。
        // 行首定位与前导空白读取都基于光标附近 RopeSlice，不 materialize 全文。
        let text = if auto_indent_enabled {
            let prefix = Self::compute_auto_indent_prefix(&self.text, byte_offset);
            format!("\n{}", prefix)
        } else {
            "\n".to_string()
        };

        self.composition_session = None;

        // 局部 Rope insert，不 clone 全文。
        self.text.insert(byte_offset, &text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_offset + text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        let new_selection = make_selection(new_cursor_val, new_cursor_val);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::point(byte_offset),
            new_range: Utf8ByteRange::from_start_len(byte_offset, text.len()),
            deleted_text: String::new(),
            inserted_text: text.clone(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = make_selection(new_cursor_val, new_cursor_val);
        let new_affected = vec![Utf8ByteRange::from_start_len(byte_offset, text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::point(byte_offset),
            inserted_text: text.clone(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
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

        let (old_animation_units, new_animation_units) = compute_animation_units_from_slices(
            animation_mode,
            &[],
            &[AnimationTextSlice {
                absolute_start: byte_offset,
                text: &text,
            }],
            &[],
            &new_affected,
        );

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Insert,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(new_cursor_val),
                should_animate: self.animation_enabled && old_cursor.value() != new_cursor_val,
            },
            // 单次换行插入从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - text.len(),
                (byte_offset, byte_offset),
                text.len(),
            )),
            old_animation_units,
            new_animation_units,
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            visual_intent,
            content_delta: EditorContentDelta::from_inserted_text(&text),
        })
    }

    /// #606: Core 端 auto-indent 前导空白计算。
    ///
    /// 从正文按 UTF-8 安全边界找到  所在逻辑行的开头，
    /// 读取该行已有的前导空白（空格和 Tab），返回前导空白字符串。
    ///
    /// 规则：
    /// - 找到  之前最后一个换行符的位置，下一字节即为行首
    /// - 从行首开始逐字节检查，只收集连续的空格和 Tab
    /// - 遇到其他字符（包括多字节字符的首字节）立即停止
    /// - UTF-8 安全：空格和 Tab 都是单字节 ASCII，不会出现在多字节字符的续字节中
    ///
    /// 返回的前导空白会被追加到新行之后，实现自动缩进。
    /// Rope 局部版本 — 只在光标前 `[0, byte_offset)` slice 上迭代，
    /// 不 materialize 全文。`bytes().rev()` 从光标向前找行首，再从行首收集前导空白。
    fn compute_auto_indent_prefix(rope: &crop::Rope, byte_offset: usize) -> String {
        // 找到 byte_offset 所在行的行首
        let prefix_slice = rope.byte_slice(0..byte_offset);
        let line_start = prefix_slice
            .bytes()
            .rev()
            .position(|b| b == b'\n')
            .map_or(0, |from_end| byte_offset - from_end);

        // 从行首开始收集前导空白（空格和 Tab）
        let line = rope.byte_slice(line_start..byte_offset);
        let mut prefix = String::new();
        for byte in line.bytes() {
            if byte == b' ' || byte == b'\t' {
                prefix.push(byte as char);
            } else {
                break;
            }
        }
        prefix
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
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
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
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

        let (byte_start, byte_end_exclusive) =
            Self::normalize_range(byte_start, byte_end_exclusive);

        if let Some(ref session) = self.composition_session {
            if byte_start != session.replace_start.value()
                || byte_end_exclusive != session.replace_end_exclusive.value()
            {
                return EditorEditOutcome::InvalidRange(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection_anchor,
                    old_selection_head,
                ));
            }
        }

        if byte_start == byte_end_exclusive
            && replacement_text.is_empty()
            && self.composition_session.is_none()
        {
            return EditorEditOutcome::NoChange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if byte_start > self.text.byte_len() || byte_end_exclusive > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(byte_start)
            || !self.text.is_char_boundary(byte_end_exclusive)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // 先取局部删除文本，再局部 Rope replace，不 clone 全文。
        let deleted_text = self
            .text
            .byte_slice(byte_start..byte_end_exclusive)
            .to_string();

        self.text
            .replace(byte_start..byte_end_exclusive, replacement_text);
        self.revision = self.revision.next();

        let sel_anchor = Self::clamp_to_char_boundary(&self.text, resulting_selection_anchor);
        let sel_head = Self::clamp_to_char_boundary(&self.text, resulting_selection_head);
        let selection_was_adjusted =
            sel_anchor != resulting_selection_anchor || sel_head != resulting_selection_head;
        self.selection_anchor = Utf8ByteOffset::unchecked(sel_anchor);
        self.cursor = Utf8ByteOffset::unchecked(sel_head);

        let new_selection = make_selection(sel_anchor, sel_head);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            new_range: Utf8ByteRange::from_start_len(byte_start, replacement_text.len()),
            deleted_text: deleted_text.clone(),
            inserted_text: replacement_text.to_string(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();
        // #684 评论 5668108597：保存 preedit_text 用于生成 old_animation_units。
        // composition commit 时 old_affected 是 preedit_text 的范围，old_text 应为 preedit_text。
        let preedit_text: String = self
            .composition_session
            .as_ref()
            .map(|s| s.preedit_text.clone())
            .unwrap_or_default();
        let preedit_byte_len = preedit_text.len();
        let is_composition_commit = self.composition_session.is_some();
        self.composition_session = None;

        let new_revision = self.revision;
        let old_affected = if preedit_byte_len > 0 {
            vec![Utf8ByteRange::from_start_len(byte_start, preedit_byte_len)]
        } else {
            vec![Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive)]
        };
        let new_affected = vec![Utf8ByteRange::from_start_len(
            byte_start,
            replacement_text.len(),
        )];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            inserted_text: replacement_text.to_string(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
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
                false,
                false,
                false,
                false,
                self.animation_enabled,
            )
        };

        // #684: composition commit 时 old 侧视觉文本是 preedit_text，
        // 普通 commit 时 old 侧是 deleted_text。
        let old_text_for_units = if preedit_byte_len > 0 {
            &preedit_text
        } else {
            &deleted_text
        };
        let (old_animation_units, new_animation_units) = compute_animation_units_from_slices(
            animation_mode,
            &[AnimationTextSlice {
                absolute_start: byte_start,
                text: old_text_for_units,
            }],
            &[AnimationTextSlice {
                absolute_start: byte_start,
                text: replacement_text,
            }],
            &old_affected,
            &new_affected,
        );

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: if is_composition_commit {
                EditorOperationKind::CompositionCommit
            } else if byte_start == byte_end_exclusive {
                EditorOperationKind::Insert
            } else if replacement_text.is_empty() {
                EditorOperationKind::Delete
            } else {
                EditorOperationKind::Replace
            },
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(sel_head),
                should_animate: self.animation_enabled && old_cursor.value() != sel_head,
            },
            // 单次 commit 从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - replacement_text.len() + (byte_end_exclusive - byte_start),
                (byte_start, byte_end_exclusive),
                replacement_text.len(),
            )),
            old_animation_units,
            new_animation_units,
        };

        let edit_result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            visual_intent,
            content_delta: EditorContentDelta::from_texts(replacement_text, &deleted_text),
        };

        if selection_was_adjusted {
            EditorEditOutcome::AppliedWithAdjustedSelection(edit_result)
        } else {
            EditorEditOutcome::Applied(edit_result)
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    fn apply_delete_surrounding(
        &mut self,
        before_byte_start: usize,
        before_byte_end_exclusive: usize,
        after_byte_start: usize,
        after_byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        let sel_anchor = self.selection_anchor.value();
        let sel_head = self.cursor.value();
        let (sel_min, sel_max) = if sel_anchor <= sel_head {
            (sel_anchor, sel_head)
        } else {
            (sel_head, sel_anchor)
        };

        let mut edits: Vec<TextEditDelta> = Vec::new();
        let old_len = self.text.byte_len();

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
        // before 删除长度用于计算 after delta 的最终文本坐标。
        // 纯几何计算，不依赖正文状态，可提前求值。
        let before_deleted_len: usize = before_range.map_or(0, |(bs, be)| be.saturating_sub(bs));

        if let Some((as_, ae)) = after_range {
            if as_ > self.text.byte_len()
                || ae > self.text.byte_len()
                || !self.text.is_char_boundary(as_)
                || !self.text.is_char_boundary(ae)
            {
                return EditorEditOutcome::InvalidOffset(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection_anchor,
                    old_selection_head,
                ));
            }
            if as_ >= ae || as_ < sel_max {
                return EditorEditOutcome::InvalidRange(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection_anchor,
                    old_selection_head,
                ));
            }
            // 局部 Rope delete 记录 delta。
            let deleted = self.text.byte_slice(as_..ae).to_string();
            self.text.delete(as_..ae);
            edits.push(TextEditDelta {
                old_range: Utf8ByteRange::from_ordered(as_, ae),
                // new_range 必须使用两次删除都完成后的最终文本坐标。
                // after 先于 before 删除，删除 after 瞬间正文仍含 before
                // 区间，point(as_) 是「仅删除 after」时的坐标；随后 before 删除会把该点
                // 左移 before_deleted_len。若这里保留 point(as_)，undo 的 DisplayPatch
                // 以 base 坐标降序应用时 after patch 会插到错误位置（"abXYcd" undo 后
                // Android 得到 "abXYdc"），Core/Android mirror 分裂。as_ >= be 保证
                // as_ - before_deleted_len >= bs >= 0，不会下溢。
                new_range: Utf8ByteRange::point(as_.saturating_sub(before_deleted_len)),
                deleted_text: deleted,
                inserted_text: String::new(),
            });
        }

        if let Some((bs, be)) = before_range {
            if bs > self.text.byte_len()
                || be > self.text.byte_len()
                || !self.text.is_char_boundary(bs)
                || !self.text.is_char_boundary(be)
            {
                return EditorEditOutcome::InvalidOffset(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection_anchor,
                    old_selection_head,
                ));
            }
            if bs >= be || be > sel_min {
                return EditorEditOutcome::InvalidRange(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection_anchor,
                    old_selection_head,
                ));
            }
            // 局部 Rope delete 记录 delta。
            let deleted = self.text.byte_slice(bs..be).to_string();
            self.text.delete(bs..be);
            edits.push(TextEditDelta {
                old_range: Utf8ByteRange::from_ordered(bs, be),
                new_range: Utf8ByteRange::point(bs),
                deleted_text: deleted,
                inserted_text: String::new(),
            });
        }

        if edits.is_empty() {
            return EditorEditOutcome::NoChange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        self.revision = self.revision.next();
        self.composition_session = None;

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

        let new_selection = make_selection(new_sel_anchor, new_sel_head);

        // content delta / offset map / affected ranges 全部从 delta 构造，
        // 计算完成后才把 edits 移入 Undo 栈。
        let mut content_delta = EditorContentDelta::default();
        let mut offset_pairs: Vec<(usize, usize, usize, usize)> = Vec::with_capacity(edits.len());
        for delta in &edits {
            content_delta.accumulate(&EditorContentDelta::from_texts(
                &delta.inserted_text,
                &delta.deleted_text,
            ));
            offset_pairs.push((
                delta.old_range.start().value(),
                delta.old_range.end().value(),
                delta.new_range.start().value(),
                delta.new_range.end().value(),
            ));
        }
        let old_affected: Vec<Utf8ByteRange> = edits.iter().map(|e| e.old_range).collect();
        let new_revision = self.revision;

        // 原子 patch batch — 每条 delta 一条局部 DisplayPatch
        // （base 文档坐标，删除的 inserted_text 为空）。不再合成最外层单条 patch
        // （把 before/after 之间的保留段 middle 重新拼接进 inserted_text）：
        // 两个相距很远的删除会复制中间整段正文，且与 batch 协议不一致。
        let display_patches: Vec<DisplayPatch> = edits
            .iter()
            .map(|d| DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: d.old_range,
                inserted_text: d.inserted_text.clone(),
                resulting_selection_byte_range: EditorEditResult::selection_byte_range(
                    new_selection,
                ),
            })
            .collect();

        self.undo_stack.push(UndoEntry {
            edits,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Delete,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: vec![],
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(new_sel_head),
                should_animate: false,
            },
            offset_map: Some(OffsetMap::from_edits(old_len, &offset_pairs)),
            // delete-surrounding 的 animation_mode 永远是 SystemSuppressed，无动画单元。
            old_animation_units: vec![],
            new_animation_units: vec![],
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            visual_intent,
            content_delta,
        })
    }

    /// 原子 IME commit：先删除 selection，再插入 inserted_text。
    ///
    /// Qt 对 `QInputMethodEvent` 的定义：先删除当前 selection，再做
    /// replacement/commit，整个 operation 加入 undo stack。
    /// 本方法在一个 `apply()` 调用内完成两步正文修改：
    /// - revision 只推进一次；
    /// - 只 push 一个 `UndoEntry`（可包含两条 `TextEditDelta`）；
    /// - selection/cursor 一次算完。
    ///
    /// 多段 affected range / offset map 按现有 `DeleteSurrounding` 的
    /// 多编辑事务写法保留，不退化成把中间没改的正文当成一个大 replace。
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn apply_ime_commit(
        &mut self,
        sel_start: usize,
        sel_end: usize,
        inserted_text: &str,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        let (sel_min, sel_max) = if sel_start <= sel_end {
            (sel_start, sel_end)
        } else {
            (sel_end, sel_start)
        };

        if sel_min > self.text.byte_len() || sel_max > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(sel_min) || !self.text.is_char_boundary(sel_max) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // Step 1: 删除 selection。
        let deleted_text = if sel_min < sel_max {
            self.text.byte_slice(sel_min..sel_max).to_string()
        } else {
            String::new()
        };
        if !deleted_text.is_empty() {
            self.text.delete(sel_min..sel_max);
        }

        self.composition_session = None;

        // Step 2: 在 selection 起点插入 inserted_text。
        if !inserted_text.is_empty() {
            self.text.insert(sel_min, inserted_text);
        }
        self.revision = self.revision.next();

        // selection 起点作为 anchor，插入后 cursor 在 anchor + inserted_text.len()。
        let new_cursor_val = sel_min + inserted_text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        let new_selection = make_selection(new_cursor_val, new_cursor_val);

        // 两条 delta：一条删除 selection，一条插入 text。
        let mut edits: Vec<TextEditDelta> = Vec::with_capacity(2);
        if sel_min < sel_max {
            edits.push(TextEditDelta {
                old_range: Utf8ByteRange::from_ordered(sel_min, sel_max),
                new_range: Utf8ByteRange::point(sel_min),
                deleted_text: deleted_text.clone(),
                inserted_text: String::new(),
            });
        }
        if !inserted_text.is_empty() {
            edits.push(TextEditDelta {
                old_range: Utf8ByteRange::point(sel_min),
                new_range: Utf8ByteRange::from_start_len(sel_min, inserted_text.len()),
                deleted_text: String::new(),
                inserted_text: inserted_text.to_string(),
            });
        }
        // 空操作（无选区 + 空插入）已在上游过滤。
        if edits.is_empty() {
            return EditorEditOutcome::NoChange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        self.undo_stack.push(UndoEntry {
            edits: edits.clone(),
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let old_affected = if sel_min < sel_max {
            vec![Utf8ByteRange::from_ordered(sel_min, sel_max)]
        } else {
            vec![]
        };
        let new_affected = if !inserted_text.is_empty() {
            vec![Utf8ByteRange::from_start_len(sel_min, inserted_text.len())]
        } else {
            vec![]
        };

        // DisplayPatch：合并为一条，deleted_text + inserted_text 反映完整变更。
        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(sel_min, sel_max),
            inserted_text: inserted_text.to_string(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        // animation_mode：IME commit 与普通 typing 相同逻辑。
        let diff_text = if !inserted_text.is_empty() {
            inserted_text
        } else {
            &deleted_text
        };
        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
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

        let (old_animation_units, new_animation_units) = {
            let old_slice = if sel_min < sel_max {
                vec![AnimationTextSlice {
                    absolute_start: sel_min,
                    text: &deleted_text,
                }]
            } else {
                vec![]
            };
            let new_slice = if !inserted_text.is_empty() {
                vec![AnimationTextSlice {
                    absolute_start: sel_min,
                    text: inserted_text,
                }]
            } else {
                vec![]
            };
            compute_animation_units_from_slices(
                animation_mode,
                &old_slice,
                &new_slice,
                &old_affected,
                &new_affected,
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
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(new_cursor_val),
                should_animate: self.animation_enabled && old_cursor.value() != new_cursor_val,
            },
            // 原子 IME commit 从 delta 构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - inserted_text.len() + (sel_max - sel_min),
                (sel_min, sel_min),
                inserted_text.len(),
            )),
            old_animation_units,
            new_animation_units,
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            visual_intent,
            content_delta: EditorContentDelta::from_texts(inserted_text, &deleted_text),
        })
    }
}
