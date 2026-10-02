//! Issue #819 评论 5956495850 第 3 节：协同动画唯一的采样入口。
//!
//! [`sample_transaction_visual_state`] 在一个 `now` 上一次性算出一笔事务的完整
//! `SampledEditVisualState`——caret 帧只采一次，文字 slice 帧全部出自这一帧
//! 的几何，不再各自从 `current_visible_fraction()` / `track.progress(now)` 推一遍。
//!
//! 规则（评论第 3 节）：
//! - cursor track 一帧只采一次；
//! - `VisualUnitTiming::CaretTrack` 的 InsertReveal/DeleteConceal 必须用这次采出的
//!   caret frame 调 `compute_frame_by_caret_ingest()`；
//! - `VisualUnitTiming::Timed` 才允许 `current_visible_fraction() + compute_frame()`；
//! - ReflowMove/ReflowCrossFade 保持 Timed；
//! - DeleteForwardBoundary 也在这里按实际本帧边界算完，产出最终 slice frame。
//!
//! `render_plan_builder` 和 `rebase` 都消费本函数的输出，这样「屏幕画的帧」和
//! 「rebase 交棒的帧」天然是同一份算法。

use std::time::Instant;

use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::animation::cursor_motion::sample_caret_track_frame;
use crate::sujian_editor_item::animation::frame_state::{
    SampledCaretFrame, SampledEditVisualState, SampledSliceFrame,
};
use crate::sujian_editor_item::animation::transaction::types::IngestSnapshotSide;
use crate::sujian_editor_item::animation::PreparedTextVisualTransaction;

/// Issue #819 评论 5956495850 第 3 节：唯一采样入口。
///
/// 在同一个 `now` 上一次性算出一笔事务的完整 `SampledEditVisualState`：
/// 1. cursor track 只采样一次（若存在），得到 `SampledCaretFrame`；
/// 2. 遍历 units：
///    - `VisualUnitTiming::CaretTrack`（协同 InsertReveal/DeleteConceal）：用本帧
///      caret 采样调 `AnimatedSlice::compute_frame_by_caret_ingest`，吞吐边界直接
///      来自 caret.x / ingest_progress，不经过任何 0..1 visible fraction；
///    - `VisualUnitTiming::Timed`（非协同吞吐字 + ReflowMove/ReflowCrossFade）：
///      用 `current_visible_fraction(now)` + `compute_frame(visible)`。
/// 3. 每个 unit 的 `AnimatedSliceFrame` 转成 `SampledSliceFrame`，带上 kind /
///    byte range / shaping identity / visual line id / snapshot side。
///
/// `caret` 为 `None` 时，CaretTrack unit 不产出 slice frame（它们的逐帧边界
/// 来自那条 track，track 不存在就不能停在半路——调用方负责 retire）。
pub(crate) fn sample_transaction_visual_state(
    tx: &PreparedTextVisualTransaction,
    now: Instant,
) -> SampledEditVisualState {
    // 1. cursor track 一帧只采一次。
    let caret: Option<SampledCaretFrame> = tx
        .cursor_visual_track
        .as_ref()
        .map(|track| sample_caret_track_frame(track, now));

    // 2. 逐 unit 采样 slice frame。
    let mut slices: Vec<SampledSliceFrame> = Vec::with_capacity(tx.units.len());
    for unit in &tx.units {
        let frame = sample_unit_slice_frame(unit, caret, now);
        if let Some(frame) = frame {
            slices.push(frame);
        }
    }

    SampledEditVisualState {
        transaction_key: tx.key,
        layout_basis_revision: tx.layout_basis_revision,
        caret,
        slices,
    }
}

/// 采样单个视觉单元的本帧 slice frame。
///
/// - `VisualUnitTiming::CaretTrack`：必须有 `caret` 才能采样，否则返回 `None`
///   （调用方负责 retire，不让协同吞吐字停在半路）。
/// - `VisualUnitTiming::Timed`：按自己的时间线算 visible_fraction + compute_frame。
fn sample_unit_slice_frame(
    unit: &crate::sujian_editor_item::animation::PreparedVisualUnit,
    caret: Option<SampledCaretFrame>,
    now: Instant,
) -> Option<SampledSliceFrame> {
    let slice = &unit.slice;
    let (frame, snapshot_side, visible_fraction) = if unit.timing.is_caret_track() {
        // 协同 InsertReveal/DeleteConceal：逐帧边界来自本帧 caret 采样。
        // DeleteForwardBoundary 的收拢边界也在这里按本帧 ingest_progress 算完。
        let caret = caret?;
        let frame = slice.compute_frame_by_caret_ingest(
            caret.x,
            caret.y,
            caret.ingest_line_ord,
            caret.is_ingest_segment,
            caret.ingest_side,
            caret.ingest_progress,
        );
        let side = slice_side_for_kind(slice.kind);
        // CaretTrack unit 的 visible_fraction 不被 rebase_from_frame 使用
        //（rebase_from_frame 对 CaretTrack 直接 return），设 0.0。
        (frame, side, 0.0)
    } else {
        // Timed unit（非协同吞吐字 + ReflowMove/ReflowCrossFade）：
        // 用自己的时间线算 visible_fraction + compute_frame。
        let visible = unit.current_visible_fraction(now);
        let frame = slice.compute_frame(visible);
        let side = slice_side_for_kind(slice.kind);
        (frame, side, visible)
    };
    // Issue #819 评论 5956495850 第 4 节：算 remaining_duration_ms 供 rebase 交棒。
    // CaretTrack unit 设 0（连续性由 RebaseCaretHandoff 承担）。
    let (remaining_duration_ms, progress) = match &unit.timing {
        crate::sujian_editor_item::animation::VisualUnitTiming::Timed {
            started_at,
            duration_ms,
            ..
        } => {
            let elapsed = match started_at {
                Some(start) => now.duration_since(*start).as_millis() as u64,
                None => 0,
            };
            let prog = unit.timing.progress(now);
            (duration_ms.saturating_sub(elapsed), prog)
        }
        crate::sujian_editor_item::animation::VisualUnitTiming::CaretTrack { .. } => (0, 0.0),
    };
    Some(SampledSliceFrame {
        kind: slice.kind,
        byte_start: slice.byte_start,
        byte_end: slice.byte_end,
        shaping_identity: slice.shaping_identity.clone(),
        dest_rect: crate::sujian_editor_item::layout_snapshot::SourceRect {
            x: frame.x,
            y: frame.y,
            w: frame.w,
            h: frame.h,
        },
        source_rect: frame.source_rect.clone(),
        opacity: frame.opacity,
        visual_line_id: slice.visual_line_id,
        snapshot_side,
        snapshot_id: frame.snapshot_id,
        visible_fraction,
        remaining_duration_ms,
        progress,
    })
}

/// 按 slice kind 决定它属于哪一侧 canonical。
///
/// - `DeleteConceal` → `Some(Old)`：吞的是 old snapshot 的字。
/// - `InsertReveal` → `Some(New)`：吐的是 new snapshot 的字。
/// - `ReflowMove` / `ReflowCrossFade` → `None`：不参与吞吐，side 无意义。
fn slice_side_for_kind(kind: AnimatedSliceKind) -> Option<IngestSnapshotSide> {
    match kind {
        AnimatedSliceKind::DeleteConceal => Some(IngestSnapshotSide::Old),
        AnimatedSliceKind::InsertReveal => Some(IngestSnapshotSide::New),
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => None,
    }
}
