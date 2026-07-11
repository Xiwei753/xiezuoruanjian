use std::time::Instant;

use super::transaction_key::VisualTransactionKey;
use super::visual_payload::VisualPayload;
use super::animation_mode::AnimationMode;
use crate::sujian_editor_item::editor_animation_debug_log;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VisualTransactionState {
    Pending,
    Prepared,
    Rendering,
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VisualOperationKind {
    Insert,
    Delete,
    Reflow,
    Cursor,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveVisualTransaction {
    pub key: VisualTransactionKey,
    pub state: VisualTransactionState,
    pub operation_kind: VisualOperationKind,
    pub animation_mode: AnimationMode,
    pub duration_ms: u64,
    pub start_time: Instant,
    pub payload: VisualPayload,
    pub cancel_reason: Option<String>,
    pub texture_prepared: bool,
}

impl ActiveVisualTransaction {
    pub fn transaction_id(&self) -> u64 {
        self.key.transaction_id
    }

    pub fn generation(&self) -> u64 {
        self.key.generation
    }

    pub fn is_insert(&self) -> bool {
        self.operation_kind == VisualOperationKind::Insert
    }

    pub fn is_delete(&self) -> bool {
        self.operation_kind == VisualOperationKind::Delete
    }

    pub fn is_reflow(&self) -> bool {
        self.operation_kind == VisualOperationKind::Reflow
    }

    pub fn is_cursor(&self) -> bool {
        self.operation_kind == VisualOperationKind::Cursor
    }

    pub fn inserted_byte_range(&self) -> Option<(usize, usize)> {
        if self.operation_kind == VisualOperationKind::Insert {
            self.payload.inserted_range()
        } else {
            None
        }
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        if self.operation_kind == VisualOperationKind::Reflow || self.operation_kind == VisualOperationKind::Insert {
            self.payload.reflow_byte_ranges()
        } else {
            Vec::new()
        }
    }
}

pub(crate) struct ActiveVisualTransactionQueue {
    transactions: Vec<ActiveVisualTransaction>,
}

impl ActiveVisualTransactionQueue {
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
        }
    }

    pub fn enqueue(&mut self, key: VisualTransactionKey, operation_kind: VisualOperationKind, payload: VisualPayload, animation_mode: AnimationMode, duration_ms: u64) {
        let kind_str = match operation_kind {
            VisualOperationKind::Insert => "Insert",
            VisualOperationKind::Delete => "Delete",
            VisualOperationKind::Reflow => "Reflow",
            VisualOperationKind::Cursor => "Cursor",
        };
        self.transactions.push(ActiveVisualTransaction {
            key,
            state: VisualTransactionState::Pending,
            operation_kind,
            animation_mode,
            duration_ms,
            start_time: Instant::now(),
            payload,
            cancel_reason: None,
            texture_prepared: false,
        });

        editor_animation_debug_log(&format!(
            "VTQueue::enqueue: tid={}, gen={}, kind={}, mode={:?}, duration_ms={}",
            key.transaction_id, key.generation, kind_str, animation_mode, duration_ms
        ));
    }

    pub fn mark_prepared(&mut self, key: VisualTransactionKey) -> bool {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            if tx.state == VisualTransactionState::Pending {
                tx.state = VisualTransactionState::Prepared;
                editor_animation_debug_log(&format!(
                    "VTQueue::mark_prepared: tid={}, gen={}",
                    key.transaction_id, key.generation
                ));
                return true;
            }
        }
        false
    }

    pub fn mark_rendering(&mut self, key: VisualTransactionKey) {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.state = VisualTransactionState::Rendering;
        }
    }

    pub fn mark_texture_prepared(&mut self, key: VisualTransactionKey) {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.texture_prepared = true;
        }
    }

    pub fn mark_texture_failed(&mut self, key: VisualTransactionKey) -> bool {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.cancel_reason = Some("texture_failed".into());
            true
        } else {
            false
        }
    }

    pub fn complete(&mut self, key: VisualTransactionKey) -> bool {
        let before = self.transactions.len();
        self.transactions.retain(|t| t.key != key);
        let removed = self.transactions.len() < before;
        if removed {
            editor_animation_debug_log(&format!(
                "VTQueue::complete: tid={}, gen={}",
                key.transaction_id, key.generation
            ));
        }
        removed
    }

    pub fn cancel(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        let before = self.transactions.len();
        self.transactions.retain(|t| t.key != key);
        let removed = self.transactions.len() < before;
        if removed {
            editor_animation_debug_log(&format!(
                "VTQueue::cancel: tid={}, gen={}, reason={}",
                key.transaction_id, key.generation, reason
            ));
        }
        removed
    }

    pub fn cancel_all(&mut self, reason: &str) {
        if !self.transactions.is_empty() {
            editor_animation_debug_log(&format!(
                "VTQueue::cancel_all: count={}, reason={}",
                self.transactions.len(),
                reason,
            ));
        }
        self.transactions.clear();
    }

    pub fn active_transactions(&self) -> &[ActiveVisualTransaction] {
        &self.transactions
    }

    pub fn has_active(&self) -> bool {
        !self.transactions.is_empty()
    }

    pub fn has_active_insert(&self) -> bool {
        self.transactions.iter().any(|t| t.is_insert() || t.is_reflow() || t.is_delete())
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        if self.transactions.is_empty() {
            return false;
        }
        let before = self.transactions.len();
        self.transactions.retain(|t| {
            let elapsed = now.duration_since(t.start_time).as_millis() as u64;
            let timeout = t.duration_ms * 3 + 500;
            if elapsed > timeout {
                editor_animation_debug_log(&format!(
                    "VTQueue::tick: expired tid={}, gen={}, elapsed={}ms, timeout={}ms",
                    t.key.transaction_id, t.key.generation, elapsed, timeout
                ));
                false
            } else {
                true
            }
        });
        self.transactions.len() != before
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    pub fn insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.transactions
            .iter()
            .filter_map(|t| t.inserted_byte_range())
            .collect()
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.transactions
            .iter()
            .flat_map(|t| t.reflow_byte_ranges())
            .collect()
    }
}
