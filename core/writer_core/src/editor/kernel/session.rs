use super::result::{EditorContentDelta, EditorEditOutcome, EditorEditResult};
use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::EditorKernel;

use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset, Utf8ByteRange};
use crate::editor::transaction::{AnimationMode, EditorTransactionCause, OffsetMap};

impl EditorKernel {
    #[allow(clippy::cast_possible_truncation)]
    pub fn load_text(&mut self, text: String, cursor: usize) -> EditorEditOutcome {
        let base_revision = self.revision;
        let old_cursor = self.cursor;
        let old_selection =
            Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());

        let needs_clamp = cursor > text.len() || !text.is_char_boundary(cursor);
        let resolved_cursor = if needs_clamp {
            crate::editor::transaction::clamp_to_char_boundary(&text, cursor)
        } else {
            cursor
        };

        // #624 评论8：load 是冷路径，允许全文统计与 materialize；
        // offset map 用 from_single_edit（无静态区域）构造。
        let old_len = self.text.byte_len();
        let old_chars: u32 = self.text.chars().count() as u32;
        let old_non_ws: u32 = self.text.chars().filter(|c| !c.is_whitespace()).count() as u32;
        self.text = crop::Rope::from(text);
        self.cursor = Utf8ByteOffset::unchecked(resolved_cursor);
        self.selection_anchor = Utf8ByteOffset::unchecked(resolved_cursor);
        self.revision = self.revision.next();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.composition_session = None;

        let new_selection = Utf8ByteRange::point(resolved_cursor);
        let new_revision = self.revision;
        let new_text = self.snapshot_text();
        let new_len = new_text.len();
        let new_chars: u32 = new_text.chars().count() as u32;
        let new_non_ws: u32 = new_text.chars().filter(|c| !c.is_whitespace()).count() as u32;

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_start_len(0, old_len),
            inserted_text: new_text,
            resulting_selection_byte_range: new_selection,
        }];

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Load,
            operation_kind: EditorOperationKind::Load,
            old_affected_byte_ranges: if old_len == 0 {
                vec![]
            } else {
                vec![Utf8ByteRange::from_start_len(0, old_len)]
            },
            new_affected_byte_ranges: if new_len == 0 {
                vec![]
            } else {
                vec![Utf8ByteRange::from_start_len(0, new_len)]
            },
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(resolved_cursor),
                should_animate: false,
            },
            offset_map: Some(OffsetMap::from_single_edit(old_len, (0, old_len), new_len)),
        };

        let result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
            content_delta: EditorContentDelta {
                inserted_chars: new_chars,
                deleted_chars: old_chars,
                inserted_non_whitespace_chars: new_non_ws,
                deleted_non_whitespace_chars: old_non_ws,
            },
        };

        if needs_clamp {
            EditorEditOutcome::AppliedWithAdjustedSelection(result)
        } else {
            EditorEditOutcome::Applied(result)
        }
    }

    pub(crate) fn stale_session_result(&mut self) -> EditorEditResult {
        let current_selection =
            Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());
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
                offset_map: None,
            },
            content_delta: EditorContentDelta::default(),
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
                offset_map: None,
            },
            content_delta: EditorContentDelta::default(),
        }
    }
    pub(crate) fn take_transaction_id(&mut self) -> u64 {
        let id = self.next_transaction_id;
        self.next_transaction_id = self.next_transaction_id.saturating_add(1);
        id
    }

    /// #624 评论8：Rope 版 char boundary clamp（不 materialize 全文）。
    pub(crate) fn clamp_to_char_boundary(rope: &crop::Rope, offset: usize) -> usize {
        if offset > rope.byte_len() {
            return rope.byte_len();
        }
        if rope.is_char_boundary(offset) {
            return offset;
        }
        let mut clamped = offset;
        while clamped > 0 && !rope.is_char_boundary(clamped) {
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
}
