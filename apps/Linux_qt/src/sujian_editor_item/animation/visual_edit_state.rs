//! 单一的正文视觉过渡。
//!
//! 每次编辑都丢弃上一份 transition，从最近收到 Qt 提交回执的 VisualFrame 构造新目标。
//! 不保存 burst、stage、travelling distance 或排队中的编辑历史。

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use writer_core::editor::OffsetMap;

use super::super::cursor_animation::CoordinatedCaretProgressLimit;
use super::super::layout_snapshot::{
    EditorLayoutSnapshot, LineSnapshotId, ShapingIdentity, SourceRect,
};
use super::super::render_ownership::RenderOwnershipPlan;
use super::super::render_plan::VisualCaretGeometry;
use super::visual_frame::{VisualCluster, VisualFrame};
use crate::sujian_editor_item::cursor_controller::CursorMoveSource;
use crate::sujian_editor_item::edit_motion::{CursorRect, DeleteEdge, DeletedRangeEdge};
use crate::sujian_editor_item::layout_revision::LayoutRevision;

#[derive(Clone, Debug)]
struct TargetCluster {
    snapshot_id: LineSnapshotId,
    byte_range: (usize, usize),
    shaping_identity: ShapingIdentity,
    rect: SourceRect,
    source_rect: SourceRect,
}

#[derive(Clone, Copy, Debug)]
enum MotionKind {
    Transform,
    /// Continue a same-shaped glyph from the latest Qt-submitted visible slice.
    RevealFromCommittedSlice,
    Reveal,
    Delete(DeleteEdge),
    CrossFade,
}

const MIN_CATCH_UP_SUBMITTED_FRAMES: u8 = 3;
static NEXT_VISUAL_TRANSITION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug)]
enum MotionBoundary {
    /// This cluster can share the current visual caret as its clipping boundary.
    VisualCaret(f64),
    /// Cross-line/shaping transitions and stationary leading-edge deletes retain
    /// this VisualEditState's explicit text-progress geometry.
    TextProgress,
}

#[derive(Clone, Copy, Debug)]
struct CaretPosition {
    x: f64,
    y: f64,
    h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VisualLineIdentity {
    // These fields form the row key across adjacent layout revisions. The revision itself
    // changes on every edit, so comparing complete LineSnapshotIds would reject valid moves.
    paragraph_id: u64,
    visual_line_ordinal: u32,
}

impl From<LineSnapshotId> for VisualLineIdentity {
    fn from(id: LineSnapshotId) -> Self {
        Self {
            paragraph_id: id.paragraph_id,
            visual_line_ordinal: id.visual_line_ordinal,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CaretMotion {
    from: CaretPosition,
    to: CaretPosition,
    to_byte: usize,
    from_line: Option<VisualLineIdentity>,
    to_line: Option<VisualLineIdentity>,
    layout_revision: LayoutRevision,
    document_session: u64,
}

impl CaretMotion {
    fn new(
        from: CursorRect,
        to: CursorRect,
        caret_byte_offsets: (usize, usize),
        base_snapshot: &EditorLayoutSnapshot,
        target_snapshot: &EditorLayoutSnapshot,
        layout_revision: LayoutRevision,
        document_session: u64,
    ) -> Self {
        let from = CaretPosition {
            x: from.x,
            y: from.top,
            h: (from.bottom - from.top).max(0.0),
        };
        let to = CaretPosition {
            x: to.x,
            y: to.top,
            h: (to.bottom - to.top).max(0.0),
        };
        Self {
            from,
            to,
            to_byte: caret_byte_offsets.1,
            from_line: caret_line_identity(base_snapshot, from),
            to_line: caret_line_identity(target_snapshot, to),
            layout_revision,
            document_session,
        }
    }

    fn can_drive_cluster_line(self, line_id: LineSnapshotId) -> bool {
        // Only clusters on the uniquely resolved source and destination row may use the
        // caret boundary. A route crossing rows stays on text progress for its whole life.
        matches!(
            (self.from_line, self.to_line),
            (Some(from), Some(to)) if from == to && from == line_id.into()
        )
    }

    fn drives_committed_slice(
        self,
        source: &VisualCluster,
        target: &TargetCluster,
        offset_map: &OffsetMap,
    ) -> bool {
        if !self.can_drive_cluster_line(target.snapshot_id) {
            return false;
        }
        let Some(mapped_source_range) =
            offset_map.map_new_range_to_old(target.byte_range.0, target.byte_range.1)
        else {
            return false;
        };
        if source.canonical_range != Some(mapped_source_range) {
            return false;
        }

        // The route must reach this cluster's logical trailing boundary and its
        // direction-aware visible frontier. Merely sharing a visual row is insufficient.
        let logical_frontier = target.byte_range.1;
        if self.to_byte < logical_frontier {
            return false;
        }

        let frontier_x = if target.shaping_identity.direction_rtl {
            target.rect.x
        } else {
            target.rect.x + target.rect.w
        };
        route_crosses_cluster_frontier(self.from, self.to, frontier_x, &target.rect)
    }

    fn drives_new_reveal(self, target: &TargetCluster, offset_map: &OffsetMap) -> bool {
        if !self.can_drive_cluster_line(target.snapshot_id)
            || self.to_byte < target.byte_range.1
            || offset_map
                .map_new_range_to_old(target.byte_range.0, target.byte_range.1)
                .is_some()
        {
            return false;
        }

        let frontier_x = cluster_trailing_frontier_x(target);
        route_reaches_cluster_frontier(self.from, self.to, frontier_x, &target.rect)
    }

    fn matches(self, caret: VisualCaretGeometry) -> bool {
        caret.movement_source == CursorMoveSource::TextTransaction
            && caret.layout_revision == Some(self.layout_revision)
            && caret.document_session == self.document_session
            && (caret.target_x - self.to.x).abs() <= 0.5
            && (caret.target_y - self.to.y).abs() <= 0.5
            && point_is_on_route(
                caret.x,
                caret.y,
                caret.path_start_x,
                caret.path_start_y,
                caret.target_x,
                caret.target_y,
            )
    }

    fn matches_committed_slice(self, target: &TargetCluster, caret: VisualCaretGeometry) -> bool {
        if !self.can_drive_cluster_line(target.snapshot_id)
            || self.to_byte < target.byte_range.1
            || !self.matches(caret)
        {
            return false;
        }
        let from = CaretPosition {
            x: caret.path_start_x,
            y: caret.path_start_y,
            h: caret.h.max(0.0),
        };
        let to = CaretPosition {
            x: caret.target_x,
            y: caret.target_y,
            h: caret.h.max(0.0),
        };
        let frontier_x = cluster_trailing_frontier_x(target);
        route_crosses_cluster_frontier(from, to, frontier_x, &target.rect)
    }

    fn matches_new_reveal(self, target: &TargetCluster, caret: VisualCaretGeometry) -> bool {
        if !self.can_drive_cluster_line(target.snapshot_id)
            || self.to_byte < target.byte_range.1
            || !self.matches(caret)
        {
            return false;
        }
        let from = CaretPosition {
            x: caret.path_start_x,
            y: caret.path_start_y,
            h: caret.h.max(0.0),
        };
        let to = CaretPosition {
            x: caret.target_x,
            y: caret.target_y,
            h: caret.h.max(0.0),
        };
        route_reaches_cluster_frontier(from, to, cluster_trailing_frontier_x(target), &target.rect)
    }
}

#[derive(Clone, Debug)]
struct ClusterMotion {
    source: Option<VisualCluster>,
    /// Other visible layers for the same mapped target cluster. They fade out as
    /// contributions to this logical glyph; they are not independent deletions.
    retiring_sources: Vec<VisualCluster>,
    target: Option<TargetCluster>,
    /// Canonical range for a source-only cross-fade whose Core mapping still survives.
    source_canonical_range: Option<(usize, usize)>,
    kind: MotionKind,
    caret_motion: Option<CaretMotion>,
    terminal_geometry_committed: bool,
    /// Width represented by the latest Qt-submitted frame, never a staged plan.
    submitted_visible_width: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct VisualEditState {
    pub transition_id: u64,
    pub source_frame: VisualFrame,
    pub target_snapshot: EditorLayoutSnapshot,
    motions: Vec<ClusterMotion>,
    pub started_at: Instant,
    pub duration_ms: u64,
    pub input_interval_ms: u64,
    pub visual_lag_px: f64,
    /// Fast-input transitions consume this budget only after unique Qt frame submissions.
    submitted_frame_count: u8,
    submitted_frame_ids: HashSet<u64>,
    minimum_submitted_frames: u8,
    /// 终点帧收到 Qt 提交回执；之后才可请求 Static ownership handoff。
    pub terminal_frame_committed: bool,
}

impl VisualEditState {
    pub(crate) fn new(
        submitted_frame: Option<&VisualFrame>,
        base_snapshot: &EditorLayoutSnapshot,
        target_snapshot: EditorLayoutSnapshot,
        frame_to_base_map: &OffsetMap,
        offset_map: &OffsetMap,
        deleted_range_edges: &[DeletedRangeEdge],
        now: Instant,
        base_duration_ms: u64,
        previous_edit_at: Option<Instant>,
        caret_rects: Option<(CursorRect, CursorRect)>,
        caret_byte_offsets: Option<(usize, usize)>,
        document_session: u64,
    ) -> Self {
        let transition_id = NEXT_VISUAL_TRANSITION_ID.fetch_add(1, Ordering::Relaxed);
        let source_frame = submitted_frame
            .cloned()
            .unwrap_or_else(|| VisualFrame::from_static_snapshot(base_snapshot));
        let targets = target_clusters(&target_snapshot);
        let caret_motion = caret_rects
            .zip(caret_byte_offsets)
            .map(|((from, to), byte_offsets)| {
                CaretMotion::new(
                    from,
                    to,
                    byte_offsets,
                    base_snapshot,
                    &target_snapshot,
                    target_snapshot.revision,
                    document_session,
                )
            });
        let mut source_used = vec![false; source_frame.clusters.len()];
        let mut target_used = vec![false; targets.len()];
        let mut motions = Vec::new();

        // 首先按 Core 的精确 offset map 认出仍是同一个逻辑 cluster 的对象。
        for (target_index, target) in targets.iter().enumerate() {
            let Some(old_range) =
                offset_map.map_new_range_to_old(target.byte_range.0, target.byte_range.1)
            else {
                continue;
            };
            let exact: Vec<usize> = source_frame
                .clusters
                .iter()
                .enumerate()
                .filter_map(|(index, source)| {
                    (!source_used[index] && source.canonical_range == Some(old_range))
                        .then_some(index)
                })
                .collect();
            if !exact.is_empty() {
                pair_cluster(
                    &exact,
                    target_index,
                    &source_frame,
                    &targets,
                    offset_map,
                    &mut source_used,
                    &mut target_used,
                    &mut motions,
                );
            }
        }

        // shaping 改变时 range 可能只部分重叠（例如 f 变成 fi）。整块 cluster
        // 一次性交给同一个 CrossFade owner，绝不按字符比例裁切 shaping cluster。
        for (target_index, target) in targets.iter().enumerate() {
            if target_used[target_index] {
                continue;
            }
            let overlapping: Vec<usize> = source_frame
                .clusters
                .iter()
                .enumerate()
                .filter_map(|(index, source)| {
                    let canonical_range = source.canonical_range?;
                    let projected =
                        offset_map.map_old_range_to_new(canonical_range.0, canonical_range.1)?;
                    (!source_used[index] && ranges_overlap(projected, target.byte_range))
                        .then_some(index)
                })
                .collect();
            if !overlapping.is_empty() {
                pair_cluster(
                    &overlapping,
                    target_index,
                    &source_frame,
                    &targets,
                    offset_map,
                    &mut source_used,
                    &mut target_used,
                    &mut motions,
                );
            }
        }

        for (target_index, target) in targets.iter().enumerate() {
            if !target_used[target_index] {
                target_used[target_index] = true;
                motions.push(ClusterMotion {
                    source: None,
                    retiring_sources: Vec::new(),
                    target: Some(target.clone()),
                    source_canonical_range: None,
                    kind: MotionKind::Reveal,
                    caret_motion: None,
                    terminal_geometry_committed: false,
                    submitted_visible_width: 0.0,
                });
            }
        }
        for (source_index, source) in source_frame.clusters.iter().enumerate() {
            if !source_used[source_index] {
                let mapped_range = source
                    .canonical_range
                    .and_then(|range| offset_map.map_old_range_to_new(range.0, range.1));
                if let Some(mapped_range) = mapped_range {
                    // The Core map says this logical text survives. If no render cluster
                    // claimed it (for example, a shaping boundary changed), fade its
                    // committed contribution out without assigning Delete semantics.
                    motions.push(ClusterMotion {
                        source: Some(source.clone()),
                        retiring_sources: Vec::new(),
                        target: None,
                        source_canonical_range: Some(mapped_range),
                        kind: MotionKind::CrossFade,
                        caret_motion: None,
                        terminal_geometry_committed: false,
                        submitted_visible_width: 0.0,
                    });
                    continue;
                }
                let current_base_range = source
                    .canonical_range
                    .and_then(|range| frame_to_base_map.map_old_range_to_new(range.0, range.1));
                let edge_from_current_edit = current_base_range.and_then(|source_range| {
                    deleted_range_edges
                        .iter()
                        .find(|deleted| ranges_overlap(source_range, deleted.range))
                        .map(|deleted| deleted.edge)
                });
                let fallback_edge = deleted_range_edges
                    .first()
                    .map(|deleted| deleted.edge)
                    .unwrap_or(DeleteEdge::Trailing);
                let delete_edge = edge_from_current_edit
                    .or(source.delete_edge)
                    .unwrap_or(fallback_edge);
                motions.push(ClusterMotion {
                    source: Some(source.clone()),
                    retiring_sources: Vec::new(),
                    target: None,
                    source_canonical_range: None,
                    kind: MotionKind::Delete(delete_edge),
                    caret_motion: None,
                    terminal_geometry_committed: false,
                    submitted_visible_width: 0.0,
                });
            }
        }

        if let Some(caret_motion) = caret_motion {
            for motion in &mut motions {
                let line_id = match (&motion.source, &motion.target, motion.kind) {
                    (None, Some(target), MotionKind::Reveal)
                        if caret_motion.drives_new_reveal(target, offset_map) =>
                    {
                        Some(target.snapshot_id)
                    }
                    (Some(source), Some(target), MotionKind::RevealFromCommittedSlice)
                        if caret_motion.drives_committed_slice(source, target, offset_map) =>
                    {
                        Some(target.snapshot_id)
                    }
                    (Some(source), None, MotionKind::Delete(_)) => Some(source.snapshot_id),
                    _ => None,
                };
                if line_id.is_some_and(|line_id| caret_motion.can_drive_cluster_line(line_id)) {
                    motion.caret_motion = Some(caret_motion);
                }
            }
        }
        for motion in &mut motions {
            if let (Some(source), Some(target), MotionKind::RevealFromCommittedSlice) =
                (&motion.source, &motion.target, motion.kind)
            {
                motion.submitted_visible_width = source.rect.w.min(target.rect.w);
            }
        }

        let input_interval_ms = previous_edit_at
            .map(|previous| now.saturating_duration_since(previous).as_millis() as u64)
            .unwrap_or(base_duration_ms);
        let visual_lag_px = motions
            .iter()
            .filter_map(|motion| match (&motion.source, &motion.target) {
                (Some(source), Some(target)) => Some(rect_distance(&source.rect, &target.rect)),
                (None, Some(target)) => Some(target.rect.w),
                (Some(source), None) => Some(source.rect.w),
                _ => None,
            })
            .fold(0.0_f64, f64::max);
        let duration_ms = catch_up_duration(base_duration_ms, input_interval_ms, visual_lag_px);
        let minimum_submitted_frames =
            if base_duration_ms > 0 && input_interval_ms < base_duration_ms {
                MIN_CATCH_UP_SUBMITTED_FRAMES
            } else {
                1
            };

        Self {
            transition_id,
            source_frame,
            target_snapshot,
            motions,
            started_at: now,
            duration_ms,
            input_interval_ms,
            visual_lag_px,
            submitted_frame_count: 0,
            submitted_frame_ids: HashSet::new(),
            minimum_submitted_frames,
            terminal_frame_committed: false,
        }
    }

    pub(crate) fn shift_started_at(&mut self, delta: Duration) {
        self.started_at = self
            .started_at
            .checked_add(delta)
            .unwrap_or(self.started_at);
    }

    pub(crate) fn progress(&self, now: Instant) -> f64 {
        if self.duration_ms == 0 {
            return 1.0;
        }
        let elapsed = now.saturating_duration_since(self.started_at).as_millis() as f64;
        let linear = (elapsed / self.duration_ms as f64).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - linear).powi(3);
        self.submitted_progress_limit()
            .map_or(eased, |limit| eased.min(limit))
    }

    fn submitted_progress_limit(&self) -> Option<f64> {
        (self.minimum_submitted_frames > 1).then(|| {
            f64::from(
                self.submitted_frame_count
                    .saturating_add(1)
                    .min(self.minimum_submitted_frames),
            ) / f64::from(self.minimum_submitted_frames)
        })
    }

    pub(crate) fn commit_submitted_frame(&mut self, frame_id: u64) {
        if self.submitted_frame_count >= self.minimum_submitted_frames
            || !self.submitted_frame_ids.insert(frame_id)
        {
            return;
        }
        self.submitted_frame_count = self.submitted_frame_count.saturating_add(1);
    }

    pub(crate) fn remaining_duration_ms(&self, now: Instant) -> u64 {
        if self.duration_ms == 0 {
            return 0;
        }
        let eased = self.progress(now);
        let linear = 1.0 - (1.0 - eased).cbrt();
        (self.duration_ms as f64 * (1.0 - linear))
            .round()
            .clamp(1.0, self.duration_ms as f64) as u64
    }

    pub(crate) fn has_caret_driven_motions(&self) -> bool {
        self.motions
            .iter()
            .any(|motion| motion.caret_motion.is_some())
    }

    pub(crate) fn coordinated_caret_progress_limit(&self) -> Option<CoordinatedCaretProgressLimit> {
        let max_eased_progress = self.submitted_progress_limit()?;
        let caret_motion = self.motions.iter().find_map(|motion| motion.caret_motion)?;
        Some(CoordinatedCaretProgressLimit {
            transition_id: self.transition_id,
            document_session: caret_motion.document_session,
            layout_revision: caret_motion.layout_revision,
            target_x: caret_motion.to.x,
            target_y: caret_motion.to.y,
            max_eased_progress,
        })
    }

    pub(crate) fn commit_terminal_motions(&mut self, motion_indices: &[usize]) {
        for &motion_index in motion_indices {
            if let Some(motion) = self.motions.get_mut(motion_index) {
                motion.terminal_geometry_committed = true;
            }
        }
    }

    pub(crate) fn commit_submitted_visible_widths(&mut self, widths: &[(usize, f64)]) {
        for &(motion_index, width) in widths {
            if let Some(motion) = self.motions.get_mut(motion_index) {
                if matches!(motion.kind, MotionKind::RevealFromCommittedSlice) {
                    motion.submitted_visible_width =
                        motion.submitted_visible_width.max(width.max(0.0));
                }
            }
        }
    }

    pub(crate) fn clear_caret_driver(&mut self) {
        for motion in &mut self.motions {
            motion.caret_motion = None;
        }
    }

    pub(crate) fn active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids = Vec::new();
        for motion in &self.motions {
            if let Some(source) = motion.source.as_ref() {
                if !ids.contains(&source.snapshot_id) {
                    ids.push(source.snapshot_id);
                }
            }
            for source in &motion.retiring_sources {
                if !ids.contains(&source.snapshot_id) {
                    ids.push(source.snapshot_id);
                }
            }
            if let Some(target) = motion.target.as_ref() {
                if !ids.contains(&target.snapshot_id) {
                    ids.push(target.snapshot_id);
                }
            }
        }
        ids
    }

    pub(crate) fn build_ownership_plan(
        &self,
        now: Instant,
        visual_caret: Option<VisualCaretGeometry>,
    ) -> RenderOwnershipPlan {
        let p = self.progress(now);
        let submitted_progress_limit = self.submitted_progress_limit().unwrap_or(1.0);
        let mut terminal_frame = true;
        let mut owned = Vec::new();
        let mut glyphs = Vec::new();
        let mut terminal_motion_indices = Vec::new();
        let mut submitted_visible_widths = Vec::new();

        for (motion_index, motion) in self.motions.iter().enumerate() {
            if let Some(target) = motion.target.as_ref() {
                owned.push(RenderOwnershipPlan::owner_key(
                    target.snapshot_id,
                    target.byte_range,
                ));
            }

            match motion.kind {
                MotionKind::Transform => {
                    terminal_frame &= p >= 1.0;
                    let (Some(source), Some(target)) = (&motion.source, &motion.target) else {
                        continue;
                    };
                    let rect = lerp_rect(&source.rect, &target.rect, p);
                    glyphs.push(RenderOwnershipPlan::glyph(
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        lerp(source.opacity, 1.0, p),
                        source.snapshot_id,
                        source.source_rect.clone(),
                        target.byte_range,
                        Some(target.byte_range),
                        None,
                        target.shaping_identity.clone(),
                    ));
                    push_retiring_source_glyphs(motion, p, &mut glyphs);
                }
                MotionKind::RevealFromCommittedSlice => {
                    let (Some(source), Some(target)) = (&motion.source, &motion.target) else {
                        continue;
                    };
                    let start_width = source.rect.w.min(target.rect.w);
                    let boundary =
                        committed_slice_boundary(target, motion.caret_motion, visual_caret);
                    let proposed_visible = if motion.terminal_geometry_committed {
                        target.rect.w
                    } else {
                        match boundary {
                            MotionBoundary::VisualCaret(caret_x) => start_width.max(
                                reveal_width_to_caret_during_transition(source, target, caret_x, p)
                                    .min(target.rect.w * submitted_progress_limit),
                            ),
                            MotionBoundary::TextProgress => lerp(start_width, target.rect.w, p),
                        }
                    };
                    let visible = motion
                        .submitted_visible_width
                        .max(proposed_visible)
                        .clamp(0.0, target.rect.w);
                    submitted_visible_widths.push((motion_index, visible));
                    let motion_terminal = motion.terminal_geometry_committed
                        || (visible >= target.rect.w - 1e-6 && p >= 1.0);
                    terminal_frame &= motion_terminal;
                    if motion_terminal && !motion.terminal_geometry_committed {
                        terminal_motion_indices.push(motion_index);
                    }
                    if visible <= 1e-6 || target.rect.h <= 1e-6 {
                        continue;
                    }
                    let (target_slice, source_rect) = visible_slice(target, visible);
                    let mut start_rect = source.rect.clone();
                    if source.shaping_identity.direction_rtl {
                        start_rect.x += source.rect.w - visible;
                    }
                    start_rect.w = visible;
                    let rect = lerp_rect(&start_rect, &target_slice, p);
                    glyphs.push(RenderOwnershipPlan::glyph(
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        lerp(source.opacity, 1.0, p),
                        target.snapshot_id,
                        source_rect,
                        target.byte_range,
                        Some(target.byte_range),
                        None,
                        target.shaping_identity.clone(),
                    ));
                    push_retiring_source_glyphs(motion, p, &mut glyphs);
                }
                MotionKind::Reveal => {
                    let Some(target) = &motion.target else {
                        continue;
                    };
                    // Coordinated mode uses this frame's already-sampled visual caret as the
                    // reveal edge for clusters on its current line. Cross-line/layout cases
                    // retain the explicit VisualEditState progress geometry.
                    let boundary = new_reveal_boundary(target, motion.caret_motion, visual_caret);
                    let visible = if motion.terminal_geometry_committed {
                        target.rect.w
                    } else {
                        match boundary {
                            MotionBoundary::VisualCaret(caret_x) => {
                                reveal_width_to_caret(target, caret_x)
                                    .min(target.rect.w * submitted_progress_limit)
                            }
                            MotionBoundary::TextProgress => target.rect.w * p,
                        }
                    };
                    let motion_terminal =
                        motion.terminal_geometry_committed || visible >= target.rect.w - 1e-6;
                    terminal_frame &= motion_terminal;
                    if motion_terminal && !motion.terminal_geometry_committed {
                        terminal_motion_indices.push(motion_index);
                    }
                    if visible <= 1e-6 || target.rect.h <= 1e-6 {
                        continue;
                    }
                    let (rect, source_rect) = visible_slice(target, visible);
                    glyphs.push(RenderOwnershipPlan::glyph(
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        1.0,
                        target.snapshot_id,
                        source_rect,
                        target.byte_range,
                        Some(target.byte_range),
                        None,
                        target.shaping_identity.clone(),
                    ));
                }
                MotionKind::Delete(delete_edge) => {
                    let Some(source) = &motion.source else {
                        continue;
                    };
                    // A moving trailing caret boundary directly controls the retained slice.
                    // Leading-edge deletes keep the caret fixed at the deletion origin, so
                    // their explicit text-progress geometry remains the correct driver.
                    let boundary =
                        delete_boundary(source, delete_edge, motion.caret_motion, visual_caret);
                    let visible = if motion.terminal_geometry_committed {
                        0.0
                    } else {
                        match boundary {
                            MotionBoundary::VisualCaret(caret_x) => {
                                delete_width_to_caret(source, delete_edge, caret_x)
                                    .max(source.rect.w * (1.0 - submitted_progress_limit))
                            }
                            MotionBoundary::TextProgress => source.rect.w * (1.0 - p),
                        }
                    };
                    let motion_terminal = motion.terminal_geometry_committed
                        || visible <= 1e-6
                        || source.opacity <= 1e-6;
                    terminal_frame &= motion_terminal;
                    if motion_terminal && !motion.terminal_geometry_committed {
                        terminal_motion_indices.push(motion_index);
                    }
                    if visible <= 1e-6 || source.rect.h <= 1e-6 {
                        continue;
                    }
                    let (rect, source_rect) = visible_slice_source(source, visible, delete_edge);
                    glyphs.push(RenderOwnershipPlan::glyph(
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        source.opacity,
                        source.snapshot_id,
                        source_rect,
                        source.byte_range,
                        None,
                        Some(delete_edge),
                        source.shaping_identity.clone(),
                    ));
                }
                MotionKind::CrossFade => {
                    terminal_frame &= p >= 1.0;
                    let source_canonical_range = motion
                        .target
                        .as_ref()
                        .map(|target| target.byte_range)
                        .or(motion.source_canonical_range);
                    if let Some(source) = &motion.source {
                        push_crossfade_source_glyph(source, source_canonical_range, p, &mut glyphs);
                    }
                    for source in &motion.retiring_sources {
                        push_crossfade_source_glyph(source, source_canonical_range, p, &mut glyphs);
                    }
                    if let Some(target) = &motion.target {
                        if p > 1e-6 {
                            glyphs.push(RenderOwnershipPlan::glyph(
                                target.rect.x,
                                target.rect.y,
                                target.rect.w,
                                target.rect.h,
                                p,
                                target.snapshot_id,
                                target.source_rect.clone(),
                                target.byte_range,
                                Some(target.byte_range),
                                None,
                                target.shaping_identity.clone(),
                            ));
                        }
                    }
                }
            }
        }

        // Once a complete terminal geometry was successfully committed, preserve it
        // through the following frame even if a new, unrelated caret route now exists.
        let terminal_frame = terminal_frame || self.terminal_frame_committed;
        let handoff_pending = self.terminal_frame_committed;

        let mut plan = RenderOwnershipPlan::from_owner_table(
            &self.target_snapshot,
            owned,
            glyphs,
            handoff_pending,
            terminal_frame,
        );
        plan.transition_id = self.transition_id;
        plan.terminal_motion_indices = terminal_motion_indices;
        plan.submitted_visible_widths = submitted_visible_widths;
        plan
    }
}

fn caret_line_identity(
    snapshot: &EditorLayoutSnapshot,
    caret: CaretPosition,
) -> Option<VisualLineIdentity> {
    let caret_center_y = caret.y + caret.h * 0.5;
    let mut matching_lines = snapshot.line_snapshots.iter().filter(|line| {
        caret_center_y >= line.visual_line_top - 0.5
            && caret_center_y <= line.visual_line_bottom + 0.5
    });
    let line = matching_lines.next()?;
    if matching_lines.next().is_some() {
        return None;
    }
    Some(line.id.into())
}

fn committed_slice_boundary(
    target: &TargetCluster,
    driver: Option<CaretMotion>,
    visual_caret: Option<VisualCaretGeometry>,
) -> MotionBoundary {
    visual_caret
        .filter(|caret| driver.is_some_and(|driver| driver.matches_committed_slice(target, *caret)))
        .map(|caret| MotionBoundary::VisualCaret(caret.x))
        .unwrap_or(MotionBoundary::TextProgress)
}

fn new_reveal_boundary(
    target: &TargetCluster,
    driver: Option<CaretMotion>,
    visual_caret: Option<VisualCaretGeometry>,
) -> MotionBoundary {
    visual_caret
        .filter(|caret| driver.is_some_and(|driver| driver.matches_new_reveal(target, *caret)))
        .map(|caret| MotionBoundary::VisualCaret(caret.x))
        .unwrap_or(MotionBoundary::TextProgress)
}

fn delete_boundary(
    source: &VisualCluster,
    delete_edge: DeleteEdge,
    driver: Option<CaretMotion>,
    visual_caret: Option<VisualCaretGeometry>,
) -> MotionBoundary {
    visual_caret
        .filter(|caret| {
            driver.is_some_and(|driver| {
                driver.can_drive_cluster_line(source.snapshot_id) && driver.matches(*caret)
            })
        })
        .filter(|caret| !caret_is_stationary_leading_edge(source, delete_edge, caret.x))
        .map(|caret| MotionBoundary::VisualCaret(caret.x))
        .unwrap_or(MotionBoundary::TextProgress)
}

fn point_is_on_route(x: f64, y: f64, from_x: f64, from_y: f64, to_x: f64, to_y: f64) -> bool {
    let dx = to_x - from_x;
    let dy = to_y - from_y;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= 1e-6 {
        return (x - to_x).abs() <= 0.5 && (y - to_y).abs() <= 0.5;
    }
    let t = (((x - from_x) * dx + (y - from_y) * dy) / length_squared).clamp(0.0, 1.0);
    let projected_x = from_x + t * dx;
    let projected_y = from_y + t * dy;
    (x - projected_x).hypot(y - projected_y) <= 1.0
}

fn route_crosses_cluster_frontier(
    from: CaretPosition,
    to: CaretPosition,
    frontier_x: f64,
    cluster: &SourceRect,
) -> bool {
    let dx = to.x - from.x;
    if dx.abs() <= 0.5 {
        return false;
    }
    let t = (frontier_x - from.x) / dx;
    // Touching the edge at the route's start or end does not prove the caret passed
    // through this glyph. Such a reveal stays on this motion's text clock.
    if !(1e-3..1.0 - 1e-3).contains(&t) {
        return false;
    }
    let from_center_y = from.y + from.h * 0.5;
    let to_center_y = to.y + to.h * 0.5;
    let route_y = lerp(from_center_y, to_center_y, t);
    let cluster_center_y = cluster.y + cluster.h * 0.5;
    let vertical_tolerance = ((from.h + cluster.h) * 0.5).max(1.0);
    (route_y - cluster_center_y).abs() <= vertical_tolerance
}

fn route_reaches_cluster_frontier(
    from: CaretPosition,
    to: CaretPosition,
    frontier_x: f64,
    cluster: &SourceRect,
) -> bool {
    let dx = to.x - from.x;
    if dx.abs() <= 0.5 {
        return false;
    }
    let t = (frontier_x - from.x) / dx;
    let endpoint_tolerance = 0.5 / dx.abs();
    if t <= 1e-3 || t > 1.0 + endpoint_tolerance {
        return false;
    }
    let from_center_y = from.y + from.h * 0.5;
    let to_center_y = to.y + to.h * 0.5;
    let route_y = lerp(from_center_y, to_center_y, t.clamp(0.0, 1.0));
    let cluster_center_y = cluster.y + cluster.h * 0.5;
    let vertical_tolerance = ((from.h + cluster.h) * 0.5).max(1.0);
    (route_y - cluster_center_y).abs() <= vertical_tolerance
}

fn cluster_trailing_frontier_x(target: &TargetCluster) -> f64 {
    if target.shaping_identity.direction_rtl {
        target.rect.x
    } else {
        target.rect.x + target.rect.w
    }
}

fn reveal_width_to_caret(target: &TargetCluster, caret_x: f64) -> f64 {
    let right = target.rect.x + target.rect.w;
    let width = if target.shaping_identity.direction_rtl {
        right - caret_x
    } else {
        caret_x - target.rect.x
    };
    width.clamp(0.0, target.rect.w)
}

fn reveal_width_to_caret_during_transition(
    source: &VisualCluster,
    target: &TargetCluster,
    caret_x: f64,
    p: f64,
) -> f64 {
    let rtl = target.shaping_identity.direction_rtl;
    let source_edge = if rtl {
        source.rect.x + source.rect.w
    } else {
        source.rect.x
    };
    let target_edge = if rtl {
        target.rect.x + target.rect.w
    } else {
        target.rect.x
    };
    let moving_edge = lerp(source_edge, target_edge, p);
    let width = if rtl {
        moving_edge - caret_x
    } else {
        caret_x - moving_edge
    };
    width.clamp(0.0, target.rect.w)
}

fn delete_width_to_caret(source: &VisualCluster, delete_edge: DeleteEdge, caret_x: f64) -> f64 {
    let right = source.rect.x + source.rect.w;
    let width = if delete_retains_right_edge(delete_edge, source.shaping_identity.direction_rtl) {
        right - caret_x
    } else {
        caret_x - source.rect.x
    };
    width.clamp(0.0, source.rect.w)
}

fn caret_is_stationary_leading_edge(
    source: &VisualCluster,
    delete_edge: DeleteEdge,
    caret_x: f64,
) -> bool {
    if delete_edge != DeleteEdge::Leading {
        return false;
    }
    let leading_edge_x = if source.shaping_identity.direction_rtl {
        source.rect.x + source.rect.w
    } else {
        source.rect.x
    };
    (caret_x - leading_edge_x).abs() <= 0.5
}

fn push_crossfade_source_glyph(
    source: &VisualCluster,
    canonical_range: Option<(usize, usize)>,
    progress: f64,
    glyphs: &mut Vec<crate::sujian_editor_item::render_plan::TextAnimationGlyphInfo>,
) {
    let opacity = source.opacity * (1.0 - progress);
    if opacity <= 1e-6 {
        return;
    }
    glyphs.push(RenderOwnershipPlan::glyph(
        source.rect.x,
        source.rect.y,
        source.rect.w,
        source.rect.h,
        opacity,
        source.snapshot_id,
        source.source_rect.clone(),
        source.byte_range,
        canonical_range,
        None,
        source.shaping_identity.clone(),
    ));
}

fn push_retiring_source_glyphs(
    motion: &ClusterMotion,
    progress: f64,
    glyphs: &mut Vec<crate::sujian_editor_item::render_plan::TextAnimationGlyphInfo>,
) {
    let canonical_range = motion
        .target
        .as_ref()
        .map(|target| target.byte_range)
        .or(motion.source_canonical_range);
    for source in &motion.retiring_sources {
        push_crossfade_source_glyph(source, canonical_range, progress, glyphs);
    }
}

fn pair_cluster(
    source_indices: &[usize],
    target_index: usize,
    source_frame: &VisualFrame,
    targets: &[TargetCluster],
    offset_map: &OffsetMap,
    source_used: &mut [bool],
    target_used: &mut [bool],
    motions: &mut Vec<ClusterMotion>,
) {
    let target = targets[target_index].clone();
    let primary_source_index = source_indices
        .iter()
        .copied()
        .max_by(|left_index, right_index| {
            let left = &source_frame.clusters[*left_index];
            let right = &source_frame.clusters[*right_index];
            let left_same_shaping = left.shaping_identity == target.shaping_identity;
            let right_same_shaping = right.shaping_identity == target.shaping_identity;
            let left_overlap = left
                .canonical_range
                .and_then(|range| offset_map.map_old_range_to_new(range.0, range.1))
                .map(|range| overlap_len(range, target.byte_range))
                .unwrap_or(0);
            let right_overlap = right
                .canonical_range
                .and_then(|range| offset_map.map_old_range_to_new(range.0, range.1))
                .map(|range| overlap_len(range, target.byte_range))
                .unwrap_or(0);
            left_same_shaping
                .cmp(&right_same_shaping)
                .then_with(|| left_overlap.cmp(&right_overlap))
                .then_with(|| left.opacity.total_cmp(&right.opacity))
        })
        .unwrap_or(source_indices[0]);
    let source = source_frame.clusters[primary_source_index].clone();
    let retiring_sources: Vec<VisualCluster> = source_indices
        .iter()
        .copied()
        .filter(|index| *index != primary_source_index)
        .map(|index| source_frame.clusters[index].clone())
        .collect();
    for &source_index in source_indices {
        source_used[source_index] = true;
    }
    target_used[target_index] = true;

    let same_shaping = source.shaping_identity == target.shaping_identity;
    let moved = rect_distance(&source.rect, &target.rect) > 0.01
        || (source.rect.w - target.rect.w).abs() > 0.01
        || (source.rect.h - target.rect.h).abs() > 0.01
        || source.opacity < 0.999
        || (source.source_rect.w - target.source_rect.w).abs() > 0.01;
    if same_shaping && !moved && retiring_sources.is_empty() {
        return;
    }
    let starts_from_partial_width =
        source.rect.w + 0.01 < target.rect.w || source.source_rect.w + 0.01 < target.source_rect.w;
    let kind = if same_shaping && starts_from_partial_width {
        MotionKind::RevealFromCommittedSlice
    } else if same_shaping {
        MotionKind::Transform
    } else {
        MotionKind::CrossFade
    };
    let submitted_visible_width = if matches!(kind, MotionKind::RevealFromCommittedSlice) {
        source.rect.w.min(target.rect.w)
    } else {
        0.0
    };
    motions.push(ClusterMotion {
        source: Some(source),
        retiring_sources,
        target: Some(target),
        source_canonical_range: Some(targets[target_index].byte_range),
        kind,
        caret_motion: None,
        terminal_geometry_committed: false,
        submitted_visible_width,
    });
}

fn target_clusters(snapshot: &EditorLayoutSnapshot) -> Vec<TargetCluster> {
    let mut result = Vec::new();
    for line in &snapshot.line_snapshots {
        for cluster in &line.clusters {
            result.push(TargetCluster {
                snapshot_id: line.id,
                byte_range: (cluster.byte_start, cluster.byte_end),
                shaping_identity: cluster.shaping_identity.clone(),
                rect: line.source_rect_to_document_rect(&cluster.source_rect),
                source_rect: cluster.source_rect.clone(),
            });
        }
    }
    result
}

fn catch_up_duration(base_ms: u64, input_interval_ms: u64, lag_px: f64) -> u64 {
    if base_ms == 0 {
        return 0;
    }
    // 慢速或单次输入保留用户设置的完整时长；只有连续输入才压缩动画，
    // 而视觉落后越远，压缩越明显。
    if input_interval_ms >= base_ms {
        return base_ms;
    }
    let cadence_ms = input_interval_ms.max(1);
    let velocity = (1.0 - input_interval_ms as f64 / base_ms as f64).clamp(0.0, 1.0);
    let lag_factor = (lag_px / 160.0).clamp(0.0, 3.0);
    let compression = 1.0 + velocity * (1.0 + lag_factor);
    // Wall-clock compression can finish before Qt presents another frame. Fast-input
    // progress is therefore additionally capped by confirmed submitted frames above; this
    // duration is only the temporal envelope, not evidence that a transition was shown.
    ((cadence_ms as f64 / compression).round() as u64).clamp(1, base_ms)
}

fn visible_slice(target: &TargetCluster, visible_width: f64) -> (SourceRect, SourceRect) {
    let width = visible_width.clamp(0.0, target.rect.w);
    let rtl = target.shaping_identity.direction_rtl;
    let x_offset = if rtl { target.rect.w - width } else { 0.0 };
    let source_x_offset = if rtl {
        target.source_rect.w - width * target.source_rect.w / target.rect.w.max(1e-6)
    } else {
        0.0
    };
    (
        SourceRect {
            x: target.rect.x + x_offset,
            y: target.rect.y,
            w: width,
            h: target.rect.h,
        },
        SourceRect {
            x: target.source_rect.x + source_x_offset,
            y: target.source_rect.y,
            w: (target.source_rect.w * width / target.rect.w.max(1e-6))
                .clamp(0.0, target.source_rect.w),
            h: target.source_rect.h,
        },
    )
}

fn visible_slice_source(
    source: &VisualCluster,
    visible_width: f64,
    delete_edge: DeleteEdge,
) -> (SourceRect, SourceRect) {
    let width = visible_width.clamp(0.0, source.rect.w);
    let rtl = source.shaping_identity.direction_rtl;
    let retain_right_edge = delete_retains_right_edge(delete_edge, rtl);
    let x_offset = if retain_right_edge {
        source.rect.w - width
    } else {
        0.0
    };
    let source_width =
        (source.source_rect.w * width / source.rect.w.max(1e-6)).clamp(0.0, source.source_rect.w);
    let source_x_offset = if retain_right_edge {
        source.source_rect.w - source_width
    } else {
        0.0
    };
    (
        SourceRect {
            x: source.rect.x + x_offset,
            y: source.rect.y,
            w: width,
            h: source.rect.h,
        },
        SourceRect {
            x: source.source_rect.x + source_x_offset,
            y: source.source_rect.y,
            w: source_width,
            h: source.source_rect.h,
        },
    )
}

fn delete_retains_right_edge(delete_edge: DeleteEdge, rtl: bool) -> bool {
    matches!(
        (delete_edge, rtl),
        (DeleteEdge::Leading, false) | (DeleteEdge::Trailing, true)
    )
}

fn lerp_rect(from: &SourceRect, to: &SourceRect, p: f64) -> SourceRect {
    SourceRect {
        x: lerp(from.x, to.x, p),
        y: lerp(from.y, to.y, p),
        w: lerp(from.w, to.w, p),
        h: lerp(from.h, to.h, p),
    }
}

fn lerp(from: f64, to: f64, p: f64) -> f64 {
    from + (to - from) * p
}

fn rect_distance(from: &SourceRect, to: &SourceRect) -> f64 {
    (from.x - to.x)
        .abs()
        .max((from.y - to.y).abs())
        .max((from.w - to.w).abs())
        .max((from.h - to.h).abs())
}

fn ranges_overlap(left: (usize, usize), right: (usize, usize)) -> bool {
    left.0 < right.1 && right.0 < left.1
}

fn overlap_len(left: (usize, usize), right: (usize, usize)) -> usize {
    left.1.min(right.1).saturating_sub(left.0.max(right.0))
}
