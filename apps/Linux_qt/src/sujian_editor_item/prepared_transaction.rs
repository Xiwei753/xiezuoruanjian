use std::time::Instant;

use writer_core::editor::CursorRect;

use super::layout_revision::CanonicalLayoutRevision;
use super::line_snapshot::PreparedLineSnapshot;
use super::animated_slice::AnimatedSlice;
use super::static_line_patch::StaticLinePatch;
use super::transaction_key::VisualTransactionKey;
use super::animation_mode::AnimationMode;
use super::transaction_queue::VisualOperationKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransactionState {
    Pending,
    Prepared,
    Rendering,
    Paused,
    Completed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedTextVisualTransaction {
    pub key: VisualTransactionKey,
    pub old_revision: CanonicalLayoutRevision,
    pub new_revision: CanonicalLayoutRevision,
    pub operation_kind: VisualOperationKind,
    pub animation_mode: AnimationMode,

    pub old_line_snapshots: Vec<PreparedLineSnapshot>,
    pub new_line_snapshots: Vec<PreparedLineSnapshot>,
    pub static_line_patches: Vec<StaticLinePatch>,
    pub animated_slices: Vec<AnimatedSlice>,

    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
    pub duration_ms: u64,

    pub state: TransactionState,
    pub prepared_at: Option<Instant>,
    pub rendering_started_at: Option<Instant>,
    pub pause_started_at: Option<Instant>,
    pub accumulated_paused_duration_ms: u64,
}

impl PreparedTextVisualTransaction {
    pub fn new(
        key: VisualTransactionKey,
        old_revision: CanonicalLayoutRevision,
        new_revision: CanonicalLayoutRevision,
        operation_kind: VisualOperationKind,
        animation_mode: AnimationMode,
        duration_ms: u64,
    ) -> Self {
        Self {
            key,
            old_revision,
            new_revision,
            operation_kind,
            animation_mode,
            old_line_snapshots: Vec::new(),
            new_line_snapshots: Vec::new(),
            static_line_patches: Vec::new(),
            animated_slices: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms,
            state: TransactionState::Pending,
            prepared_at: None,
            rendering_started_at: None,
            pause_started_at: None,
            accumulated_paused_duration_ms: 0,
        }
    }

    pub fn mark_prepared(&mut self) {
        if self.state == TransactionState::Pending {
            self.state = TransactionState::Prepared;
            self.prepared_at = Some(Instant::now());
        }
    }

    pub fn mark_rendering(&mut self) {
        if self.state == TransactionState::Prepared || self.state == TransactionState::Paused {
            self.state = TransactionState::Rendering;
            self.rendering_started_at = Some(Instant::now());
        }
    }

    pub fn mark_paused(&mut self) {
        if self.state == TransactionState::Rendering {
            self.state = TransactionState::Paused;
            self.pause_started_at = Some(Instant::now());
        }
    }

    pub fn mark_resumed(&mut self) {
        if self.state == TransactionState::Paused {
            if let Some(pause_start) = self.pause_started_at {
                self.accumulated_paused_duration_ms +=
                    Instant::now().duration_since(pause_start).as_millis() as u64;
            }
            self.pause_started_at = None;
            self.state = TransactionState::Rendering;
        }
    }

    pub fn mark_completed(&mut self) {
        if self.state == TransactionState::Rendering {
            self.state = TransactionState::Completed;
        }
    }

    pub fn mark_cancelled(&mut self) {
        self.state = TransactionState::Cancelled;
    }

    pub fn effective_elapsed_ms(&self, now: Instant) -> u64 {
        match self.rendering_started_at {
            Some(start) => {
                let total = now.duration_since(start).as_millis() as u64;
                total.saturating_sub(self.accumulated_paused_duration_ms)
            }
            None => 0,
        }
    }

    pub fn progress(&self, now: Instant) -> f64 {
        if self.duration_ms == 0 {
            return 1.0;
        }
        let elapsed = self.effective_elapsed_ms(now);
        (elapsed as f64 / self.duration_ms as f64).min(1.0)
    }

    pub fn is_insert(&self) -> bool {
        self.operation_kind == VisualOperationKind::Insert
    }

    pub fn is_delete(&self) -> bool {
        self.operation_kind == VisualOperationKind::Delete
    }

    pub fn insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.animated_slices
            .iter()
            .filter(|s| s.is_insert())
            .map(|s| s.document_byte_range)
            .collect()
    }

    pub fn delete_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.animated_slices
            .iter()
            .filter(|s| s.is_delete())
            .map(|s| s.document_byte_range)
            .collect()
    }

    pub fn all_hidden_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.animated_slices
            .iter()
            .map(|s| s.document_byte_range)
            .collect()
    }

    pub fn snapshot_for_id(&self, id: &super::snapshot_id::LineSnapshotId) -> Option<&PreparedLineSnapshot> {
        self.old_line_snapshots
            .iter()
            .chain(self.new_line_snapshots.iter())
            .find(|s| &s.snapshot_id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_transaction() -> PreparedTextVisualTransaction {
        let key = VisualTransactionKey::new(1, 1);
        let old_rev = CanonicalLayoutRevision::new(1, 1, 800.0, "Serif", 16.0, 50, 1.5, 32.0, 16.0, 1.0);
        let new_rev = CanonicalLayoutRevision::new(2, 2, 800.0, "Serif", 16.0, 50, 1.5, 32.0, 16.0, 1.0);
        PreparedTextVisualTransaction::new(
            key,
            old_rev,
            new_rev,
            VisualOperationKind::Insert,
            AnimationMode::GlyphAnimation,
            200,
        )
    }

    #[test]
    fn test_state_transitions() {
        let mut tx = make_transaction();
        assert_eq!(tx.state, TransactionState::Pending);

        tx.mark_prepared();
        assert_eq!(tx.state, TransactionState::Prepared);

        tx.mark_rendering();
        assert_eq!(tx.state, TransactionState::Rendering);

        tx.mark_completed();
        assert_eq!(tx.state, TransactionState::Completed);
    }

    #[test]
    fn test_pause_resume() {
        let mut tx = make_transaction();
        tx.mark_prepared();
        tx.mark_rendering();
        tx.mark_paused();
        assert_eq!(tx.state, TransactionState::Paused);

        tx.mark_resumed();
        assert_eq!(tx.state, TransactionState::Rendering);
    }

    #[test]
    fn test_cancel() {
        let mut tx = make_transaction();
        tx.mark_cancelled();
        assert_eq!(tx.state, TransactionState::Cancelled);
    }

    #[test]
    fn test_invalid_transitions_ignored() {
        let mut tx = make_transaction();
        tx.mark_rendering();
        assert_eq!(tx.state, TransactionState::Pending);

        tx.mark_prepared();
        tx.mark_completed();
        assert_eq!(tx.state, TransactionState::Prepared);
    }
}
