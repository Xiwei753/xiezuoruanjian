use super::types::{
    AnimationMode, EditorAnimationKind, EditorChange, EditorSelection, EditorTransaction,
    EditorTransactionCause,
};
#[cfg(test)]
use super::visual::EditorAnimationEvent;
use super::visual::{EditorVisualTransaction, HiddenVisualRange, VisualCoordinateMode};
use super::visual_classification::{
    choose_animation_mode, classify_visual_diff, common_prefix_byte_len, common_suffix_byte_len,
    count_grapheme_clusters, diff_plain_text, should_animate_changes, split_text_into_clusters,
    split_text_into_runs, text_contains_complex_grapheme,
};
use crate::editor::strong_types::{Utf8ByteOffset, Utf8ByteRange};

/// 编辑引擎 — 创建 EditorTransaction 和 EditorVisualTransaction 的工厂。
#[derive(Debug, Clone)]
pub struct EditorEngine {
    next_animation_id: u64,
    max_animated_chars: usize,
    animation_duration_ms: u64,
}

impl Default for EditorEngine {
    fn default() -> Self {
        Self {
            next_animation_id: 1,
            max_animated_chars: 8,
            animation_duration_ms: 160,
        }
    }
}

impl EditorEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_animation_limits(max_animated_chars: usize, animation_duration_ms: u64) -> Self {
        Self {
            next_animation_id: 1,
            max_animated_chars,
            animation_duration_ms,
        }
    }

    pub fn set_animation_duration_ms(&mut self, duration_ms: u64) {
        self.animation_duration_ms = duration_ms;
    }

    pub fn create_transaction(
        &self,
        old_text: impl Into<String>,
        new_text: impl Into<String>,
        old_selection: EditorSelection,
        new_selection: EditorSelection,
        cause: EditorTransactionCause,
    ) -> EditorTransaction {
        let old_text = old_text.into();
        let new_text = new_text.into();
        let changes = diff_plain_text(&old_text, &new_text);
        let should_animate = should_animate_changes(&changes, cause, self.max_animated_chars);
        EditorTransaction {
            old_text, new_text, changes, old_selection, new_selection, cause, should_animate,
        }
    }

    #[cfg(test)]
    #[deprecated(
        since = "0.12.0",
        note = "Use visual_transaction() instead. This will be removed in a future version."
    )]
    #[allow(deprecated)]
    pub(crate) fn animation_events(
        &mut self,
        transaction: &EditorTransaction,
    ) -> Vec<EditorAnimationEvent> {
        let mut events = Vec::new();
        if transaction.should_animate {
            for change in &transaction.changes {
                let kind = match change {
                    EditorChange::Insert { .. } => EditorAnimationKind::Insert,
                    EditorChange::Delete { .. } => EditorAnimationKind::Delete,
                };
                events.push(EditorAnimationEvent {
                    id: self.take_animation_id(),
                    kind,
                    range_start: change.index(),
                    range_len: Utf8ByteOffset::unchecked(change.text().len()),
                    text: change.text().to_string(),
                    old_cursor: transaction.old_selection.head,
                    new_cursor: transaction.new_selection.head,
                    duration_ms: self.animation_duration_ms,
                    glyph_rects: Vec::new(),
                    old_cursor_rect: None,
                    new_cursor_rect: None,
                });
            }
        }
        if transaction.cause != EditorTransactionCause::Load
            && transaction.old_selection.head != transaction.new_selection.head
        {
            events.push(EditorAnimationEvent {
                id: self.take_animation_id(),
                kind: EditorAnimationKind::Cursor,
                range_start: transaction.new_selection.head.index,
                range_len: Utf8ByteOffset::unchecked(0),
                text: String::new(),
                old_cursor: transaction.old_selection.head,
                new_cursor: transaction.new_selection.head,
                duration_ms: self.animation_duration_ms,
                glyph_rects: Vec::new(),
                old_cursor_rect: None,
                new_cursor_rect: None,
            });
        }
        events
    }

    fn take_animation_id(&mut self) -> u64 {
        let id = self.next_animation_id;
        self.next_animation_id = self.next_animation_id.saturating_add(1);
        id
    }

    pub fn visual_transaction(
        &mut self,
        transaction: &EditorTransaction,
    ) -> Option<EditorVisualTransaction> {
        if !transaction.should_animate || transaction.changes.len() != 1 {
            return None;
        }
        let change = &transaction.changes[0];
        let kind = match change {
            EditorChange::Insert { .. } => EditorAnimationKind::Insert,
            EditorChange::Delete { .. } => EditorAnimationKind::Delete,
        };
        let inserted_range = match change {
            EditorChange::Insert { index, text } => {
                Utf8ByteRange::from_values(index.value(), index.value() + text.len())
            }
            EditorChange::Delete { .. } => None,
        };
        let deleted_range = match change {
            EditorChange::Insert { .. } => None,
            EditorChange::Delete { index, text } => {
                Utf8ByteRange::from_values(index.value(), index.value() + text.len())
            }
        };
        let text = change.text();
        let cluster_count = count_grapheme_clusters(text);
        let contains_newline = text.contains('\n');
        let contains_complex_grapheme = text_contains_complex_grapheme(text);
        let is_loading = transaction.cause == EditorTransactionCause::Load;
        let is_applying_format = transaction.cause == EditorTransactionCause::Format;
        let animation_mode = choose_animation_mode(
            cluster_count, contains_newline, contains_complex_grapheme,
            false, is_loading, is_applying_format, false, true,
        );
        let (cluster_rects, cluster_runs) = match change {
            EditorChange::Insert { index, text: _ } => {
                (Some(split_text_into_clusters(text, index.value())),
                 Some(split_text_into_runs(text, index.value())))
            }
            EditorChange::Delete { .. } => (None, None),
        };
        let hidden_visual_ranges = match inserted_range {
            Some(range) => vec![HiddenVisualRange {
                id: self.take_animation_id(),
                kind: animation_mode, range, old_rect: None, new_rect: None,
                line_index: 0, payload_ref: None,
            }],
            None => Vec::new(),
        };
        Some(EditorVisualTransaction {
            id: self.take_animation_id(),
            kind,
            cause: transaction.cause,
            old_text: transaction.old_text.clone(),
            new_text: transaction.new_text.clone(),
            old_selection: transaction.old_selection,
            new_selection: transaction.new_selection,
            inserted_range,
            deleted_range,
            deleted_glyph_rects: None,
            insert_glyph_rects: None,
            reflow_glyph_rects: None,
            animation_mode,
            cluster_rects,
            cluster_runs,
            hidden_visual_ranges,
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: self.animation_duration_ms,
            coordinate_mode: VisualCoordinateMode::Baseline,
        })
    }

    pub fn cursor_only_transaction(
        &mut self,
        text: &str,
        old_cursor_index: usize,
        new_cursor_index: usize,
    ) -> Option<EditorVisualTransaction> {
        if old_cursor_index == new_cursor_index {
            return None;
        }
        let old_sel = EditorSelection::collapsed(text, old_cursor_index);
        let new_sel = EditorSelection::collapsed(text, new_cursor_index);
        Some(EditorVisualTransaction {
            id: self.take_animation_id(),
            kind: EditorAnimationKind::Cursor,
            cause: EditorTransactionCause::Programmatic,
            old_text: text.to_string(),
            new_text: text.to_string(),
            old_selection: old_sel,
            new_selection: new_sel,
            inserted_range: None,
            deleted_range: None,
            deleted_glyph_rects: None,
            insert_glyph_rects: None,
            reflow_glyph_rects: None,
            animation_mode: AnimationMode::GlyphAnimation,
            cluster_rects: None,
            cluster_runs: None,
            hidden_visual_ranges: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: self.animation_duration_ms,
            coordinate_mode: VisualCoordinateMode::Baseline,
        })
    }
}

/// #516: 检查两个事务是否在视觉区域上冲突。
pub fn transactions_overlap(
    old_kind: super::visual::UnifiedTransactionKind,
    old_affected_range: (usize, usize),
    new_kind: super::visual::UnifiedTransactionKind,
    new_affected_range: (usize, usize),
) -> bool {
    let (old_start, old_end) = old_affected_range;
    let (new_start, new_end) = new_affected_range;
    if matches!(old_kind, super::visual::UnifiedTransactionKind::CursorOnly)
        || matches!(new_kind, super::visual::UnifiedTransactionKind::CursorOnly)
    {
        return true;
    }
    old_start < new_end && new_start < old_end
}

/// #516/#606: 统一 rebase — 新事务与旧事务冲突时的处理。
pub fn compute_rebase(
    cancelled_transaction_id: u64,
    old_progress: f64,
    old_frame_snapshot: Option<super::rebase::RebaseFrameSnapshot>,
    input: super::rebase_mapping::SliceMatchInput,
) -> super::rebase::TransactionRebase {
    let slice_mappings = super::rebase_mapping::compute_rebase_slice_mappings(input);
    super::rebase::TransactionRebase {
        cancelled_transaction_id,
        old_progress,
        old_frame_snapshot,
        slice_mappings,
    }
}
