use std::time::Instant;

use writer_core::editor::CursorRect;

use super::animated_slice::AnimatedSlice;
use super::animation_mode::AnimationMode;
use super::cursor_animation::CursorTransition;
use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId};
use super::static_line_patch::StaticLinePatch;
use super::transaction_key::VisualTransactionKey;

/// 平台视觉事务状态机：
///
/// ```text
/// Pending → Prepared → Rendering ↔ Paused → Completed
///    └──── 任一未终态 ────→ Cancelled
/// ```
///
/// 动画计时只能在首次进入 Rendering 时开始（`first_render_frame`），
/// 暂停时累计暂停时长，恢复后 progress 连续。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextVisualTransactionState {
    Pending,
    Prepared,
    Rendering,
    Paused,
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextVisualOperationKind {
    Insert,
    Delete,
    Cursor,
}

/// 一次平台视觉事务持有的全部资源。
///
/// 它拥有一次动画所需的 slices、patches、cursor transition 和 old/new snapshots。
/// `texture_prepared` 为 true 后，静态层才允许隐藏对应范围，否则会出现一帧空洞。
/// 事务完成、取消或超时移除后，对应快照资源才可以释放。
#[derive(Clone, Debug)]
pub(crate) struct PreparedTextVisualTransaction {
    pub key: VisualTransactionKey,
    pub state: TextVisualTransactionState,
    pub operation_kind: TextVisualOperationKind,
    pub animation_mode: AnimationMode,
    pub duration_ms: u64,
    pub start_time: Instant,
    pub old_revision: LayoutRevision,
    pub new_revision: LayoutRevision,
    pub slices: Vec<AnimatedSlice>,
    pub static_patches: Vec<StaticLinePatch>,
    pub cursor_transition: CursorTransition,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
    pub cancel_reason: Option<String>,
    pub texture_prepared: bool,
    pub first_render_frame: Option<Instant>,
    pub rendering_started_at: Option<Instant>,
    pub accumulated_paused_duration_ms: u64,
    pub pause_start: Option<Instant>,
    pub old_snapshot: Option<EditorLayoutSnapshot>,
    pub new_snapshot: Option<EditorLayoutSnapshot>,
}

impl PreparedTextVisualTransaction {
    pub fn is_insert(&self) -> bool {
        self.operation_kind == TextVisualOperationKind::Insert
    }

    pub fn is_delete(&self) -> bool {
        self.operation_kind == TextVisualOperationKind::Delete
    }

    pub fn is_cursor(&self) -> bool {
        self.operation_kind == TextVisualOperationKind::Cursor
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        let effective_start = self.rendering_started_at.unwrap_or(self.first_render_frame.unwrap_or(self.start_time));
        let elapsed = now.duration_since(effective_start).as_millis() as u64;
        let effective_duration = self.duration_ms + self.accumulated_paused_duration_ms;
        let timeout = effective_duration * 3 + 500;
        elapsed > timeout
    }

    pub fn progress(&self, now: Instant) -> f64 {
        let effective_start = self.rendering_started_at.unwrap_or(self.first_render_frame.unwrap_or(self.start_time));
        let elapsed_ms = now.duration_since(effective_start).as_millis() as f64;
        let effective_duration_ms = self.duration_ms as f64;
        if effective_duration_ms <= 0.0 {
            return 1.0;
        }
        let adjusted_elapsed = elapsed_ms - self.accumulated_paused_duration_ms as f64;
        (adjusted_elapsed / effective_duration_ms).min(1.0).max(0.0)
    }

    pub fn pause(&mut self) {
        if self.state == TextVisualTransactionState::Rendering {
            self.state = TextVisualTransactionState::Paused;
            self.pause_start = Some(Instant::now());
        }
    }

    pub fn resume(&mut self) {
        if self.state == TextVisualTransactionState::Paused {
            if let Some(pause_start) = self.pause_start.take() {
                self.accumulated_paused_duration_ms += Instant::now().duration_since(pause_start).as_millis() as u64;
            }
            self.state = TextVisualTransactionState::Rendering;
        }
    }

    pub fn mark_first_render(&mut self) {
        if self.first_render_frame.is_none() {
            self.first_render_frame = Some(Instant::now());
        }
        if self.rendering_started_at.is_none() {
            self.rendering_started_at = Some(Instant::now());
        }
    }

    pub fn inserted_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.static_patches
            .iter()
            .filter(|p| p.is_insert)
            .map(|p| (p.byte_start, p.byte_end))
            .collect()
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.static_patches
            .iter()
            .filter(|p| !p.is_insert)
            .map(|p| (p.byte_start, p.byte_end))
            .collect()
    }

    pub fn all_hidden_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.static_patches
            .iter()
            .map(|p| (p.byte_start, p.byte_end))
            .collect()
    }

    pub fn overlaps_byte_range(&self, byte_start: usize, byte_end: usize) -> bool {
        self.slices
            .iter()
            .any(|s| s.byte_end > byte_start && s.byte_start < byte_end)
            || self
                .static_patches
                .iter()
                .any(|p| p.intersects(byte_start, byte_end))
    }

    pub fn snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = self.slices.iter().map(|s| s.snapshot_id).collect();
        for patch in &self.static_patches {
            ids.push(patch.snapshot_id);
        }
        ids.sort_by_key(|id| (id.layout_revision, id.paragraph_id, id.visual_line_ordinal));
        ids.dedup();
        ids
    }
}

/// Linux 当前唯一事务队列。
///
/// 冲突判断基于 byte range 与仍活跃的视觉资源，不是简单"新输入清空旧动画"：
/// 新事务的 byte range 与现有事务的 slices/patches 有重叠时，先从旧事务的当前视觉帧
/// rebase，再取消旧事务，保证连续输入无跳变。
#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedTransactionQueue {
    transactions: Vec<PreparedTextVisualTransaction>,
}

impl PreparedTransactionQueue {
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
        }
    }

    pub fn enqueue(&mut self, tx: PreparedTextVisualTransaction) {
        self.transactions.push(tx);
    }

    pub fn mark_prepared(&mut self, key: VisualTransactionKey) -> bool {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            if tx.state == TextVisualTransactionState::Pending {
                tx.state = TextVisualTransactionState::Prepared;
                return true;
            }
        }
        false
    }

    pub fn mark_rendering(&mut self, key: VisualTransactionKey) {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            if tx.state == TextVisualTransactionState::Prepared {
                tx.state = TextVisualTransactionState::Rendering;
                tx.mark_first_render();
            }
        }
    }

    pub fn mark_texture_prepared(&mut self, key: VisualTransactionKey) {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.texture_prepared = true;
        }
    }

    pub fn complete(&mut self, key: VisualTransactionKey) -> bool {
        let before = self.transactions.len();
        self.transactions.retain(|t| t.key != key);
        self.transactions.len() < before
    }

    pub fn cancel(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        let before = self.transactions.len();
        self.transactions.retain(|t| t.key != key);
        self.transactions.len() < before
    }

    pub fn cancel_all(&mut self, reason: &str) {
        self.transactions.clear();
    }

    pub fn tick(&mut self, now: Instant) -> Vec<VisualTransactionKey> {
        let mut expired = Vec::new();
        self.transactions.retain(|t| {
            if t.is_expired(now) {
                expired.push(t.key);
                false
            } else {
                true
            }
        });
        expired
    }

    pub fn find_conflicting_insert(
        &self,
        byte_start: usize,
        byte_end: usize,
    ) -> Option<VisualTransactionKey> {
        self.transactions
            .iter()
            .filter(|t| t.state != TextVisualTransactionState::Cancelled && t.state != TextVisualTransactionState::Completed)
            .find(|t| t.overlaps_byte_range(byte_start, byte_end))
            .map(|t| t.key)
    }

    pub fn active_transactions(&self) -> &[PreparedTextVisualTransaction] {
        &self.transactions
    }

    pub fn active_transactions_mut(&mut self) -> &mut [PreparedTextVisualTransaction] {
        &mut self.transactions
    }

    pub fn has_active(&self) -> bool {
        !self.transactions.is_empty()
    }

    pub fn has_active_insert(&self) -> bool {
        self.transactions
            .iter()
            .any(|t| t.state != TextVisualTransactionState::Cancelled && t.state != TextVisualTransactionState::Completed)
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    pub fn insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.transactions
            .iter()
            .filter(|t| t.state != TextVisualTransactionState::Cancelled && t.state != TextVisualTransactionState::Completed)
            .flat_map(|t| t.inserted_byte_ranges())
            .collect()
    }

    pub fn all_hidden_ranges(&self) -> Vec<(usize, usize)> {
        self.transactions
            .iter()
            .filter(|t| t.state != TextVisualTransactionState::Cancelled && t.state != TextVisualTransactionState::Completed && t.texture_prepared)
            .flat_map(|t| t.all_hidden_byte_ranges())
            .collect()
    }
}
