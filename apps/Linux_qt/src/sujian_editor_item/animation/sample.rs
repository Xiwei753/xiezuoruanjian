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

/// Issue #819 评论 5967250411 问题 5：接收预先采好的 caret 的采样入口。
///
/// 与 [`sample_transaction_visual_state`] 的区别：本函数不自己调 `sample_caret_track_frame`，
/// 而是接收调用方预先采好的 `caret`。`build_text_animation_plan_with_sample` 先调
/// `sample_coordinated_motion_frame` 采一次 caret（得到 `CoordinatedMotionFrame`），
/// 然后对 owner transaction 调本函数传入那份 caret，非 owner 事务传入 `None`。
/// 这样一帧只采一次 caret，文字和 CoordinatedMotionFrame 消费同一个 `SampledCaretFrame`。
pub(crate) fn sample_transaction_visual_state_with_caret(
    tx: &PreparedTextVisualTransaction,
    now: Instant,
    caret: Option<SampledCaretFrame>,
) -> SampledEditVisualState {
    // 逐 unit 采样 slice frame，使用传入的 caret（不再自己采 track）。
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
    // Issue #819 评论 5967250411 问题 1：本帧本 unit 是否已到达终态。
    // 不再让 rebase 拿 visible_fraction 猜 CaretTrack 终态。
    let (frame, snapshot_side, visible_fraction, is_finished) = if unit.timing.is_caret_track() {
        // 协同 InsertReveal/DeleteConceal：逐帧边界来自本帧 caret 采样。
        // DeleteForwardBoundary 的收拢边界也在这里按本帧 ingest_progress 算完。
        let caret = caret?;
        // Issue #819 评论 5968931455: stage_id 过滤。
        // carried unit（旧 stage_id）和新 unit（新 stage_id）在同一条 track 上，
        // 但 segment 的 stage_id 不同。只有 stage_id 匹配的 unit 才在当前段被吞吐，
        // 其余 unit 保持初态/终态，避免跨事务 carried unit 消费下一笔编辑的 route。
        if let Some(unit_stage_id) = unit.stage_id {
            if unit_stage_id != caret.ingest_stage_id {
                // stage_id 不匹配：本 unit 不该被当前 segment 驱动。
                // unit 的 stage 在前（< caret stage）→ 已完成，保持终态；
                // unit 的 stage 在后（> caret stage）→ 还没开始，保持初态。
                let unit_passed = unit_stage_id < caret.ingest_stage_id;
                let (frame, vis) = stage_mismatch_frame(slice, unit_passed);
                let side = slice_side_for_kind(slice.kind);
                (frame, side, vis, unit_passed)
            } else {
                // stage_id 匹配：正常消费当前 segment。
                let frame = slice.compute_frame_by_caret_ingest(
                    caret.x,
                    caret.y,
                    caret.ingest_line_ord,
                    caret.is_ingest_segment,
                    caret.ingest_side,
                    caret.ingest_progress,
                );
                let side = slice_side_for_kind(slice.kind);
                let caret_finished = caret.progress >= 1.0;
                (frame, side, 0.0, caret_finished)
            }
        } else {
            // unit 没有 stage_id（向后兼容 / 测试构造）：正常消费。
            let frame = slice.compute_frame_by_caret_ingest(
                caret.x,
                caret.y,
                caret.ingest_line_ord,
                caret.is_ingest_segment,
                caret.ingest_side,
                caret.ingest_progress,
            );
            let side = slice_side_for_kind(slice.kind);
            let caret_finished = caret.progress >= 1.0;
            (frame, side, 0.0, caret_finished)
        }
    } else {
        // Timed unit（非协同吞吐字 + ReflowMove/ReflowCrossFade）：
        // 用自己的时间线算 visible_fraction + compute_frame。
        let visible = unit.current_visible_fraction(now);
        let frame = slice.compute_frame(visible);
        let side = slice_side_for_kind(slice.kind);
        // Timed unit 的终态由自己的 timeline progress 决定。
        let timed_finished = unit.timing.progress(now) >= 1.0;
        (frame, side, visible, timed_finished)
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
        unit_stage_id: unit.stage_id,
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
        is_finished,
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

/// Issue #819 评论 5968931455: stage_id 不匹配时返回初态/终态帧。
///
/// - `unit_passed = true`：unit 的 stage 在当前 segment 之前（已完成）→ 终态。
///   - InsertReveal 终态 = fully shown（visible = 1.0）。
///   - DeleteConceal 终态 = fully hidden（visible = 0.0）。
/// - `unit_passed = false`：unit 的 stage 在当前 segment 之后（还没开始）→ 初态。
///   - InsertReveal 初态 = not shown（visible = 0.0）。
///   - DeleteConceal 初态 = fully shown（visible = 1.0）。
///
/// 返回 `(frame, visible_fraction)`，`visible_fraction` 供 rebase 交棒用。
fn stage_mismatch_frame(
    slice: &AnimatedSlice,
    unit_passed: bool,
) -> (
    crate::sujian_editor_item::animated_slice::AnimatedSliceFrame,
    f64,
) {
    let visible = match (slice.kind, unit_passed) {
        (AnimatedSliceKind::InsertReveal, true) => 1.0, // 终态：完全显示
        (AnimatedSliceKind::InsertReveal, false) => 0.0, // 初态：不显示
        (AnimatedSliceKind::DeleteConceal, true) => 0.0, // 终态：完全隐藏
        (AnimatedSliceKind::DeleteConceal, false) => 1.0, // 初态：完全显示
        // Reflow 不参与 stage_id 过滤（始终 Timed），这里防御性返回初态。
        (AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade, _) => 0.0,
    };
    (slice.compute_frame(visible), visible)
}
