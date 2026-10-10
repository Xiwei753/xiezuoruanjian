//! 单一的正文视觉过渡。
//!
//! 每次编辑都丢弃上一份 transition，从最近收到 Qt 提交回执的 VisualFrame 构造新目标。
//! 不保存 burst、stage、travelling distance 或排队中的编辑历史。

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use writer_core::editor::OffsetMap;

use super::super::layout_snapshot::{
    EditorLayoutSnapshot, LineSnapshotId, PreparedLineSnapshot, ShapingIdentity, SourceRect,
};
use super::super::render_ownership::RenderOwnershipPlan;
use super::super::render_plan::VisualCaretGeometry;
use super::edit_timeline::{CaretPath, EditMotionSample, EditVisualTimeline};
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

#[derive(Clone, Debug)]
struct CaretMotion {
    transition_id: u64,
    from: CaretPosition,
    to: CaretPosition,
    to_byte: usize,
    from_line: Option<VisualLineIdentity>,
    to_line: Option<VisualLineIdentity>,
    layout_revision: LayoutRevision,
    document_session: u64,
    path: Option<CaretPath>,
}

impl CaretMotion {
    fn new(
        transition_id: u64,
        from: CursorRect,
        to: CursorRect,
        caret_byte_offsets: (usize, usize),
        base_snapshot: &EditorLayoutSnapshot,
        target_snapshot: &EditorLayoutSnapshot,
        layout_revision: LayoutRevision,
        document_session: u64,
    ) -> Self {
        let from_rect = from;
        let to_rect = to;
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
        let from_line_snapshot = line_for_caret(base_snapshot, from);
        let to_line_snapshot = line_for_caret(target_snapshot, to);
        let path = from_line_snapshot
            .zip(to_line_snapshot)
            .filter(|(from_line, to_line)| {
                VisualLineIdentity::from(from_line.id) != VisualLineIdentity::from(to_line.id)
            })
            .and_then(|(from_line, to_line)| {
                build_caret_path(from_line, to_line, from_rect, to_rect, caret_byte_offsets)
            });
        Self {
            transition_id,
            from,
            to,
            to_byte: caret_byte_offsets.1,
            from_line: caret_line_identity(base_snapshot, from),
            to_line: caret_line_identity(target_snapshot, to),
            layout_revision,
            document_session,
            path,
        }
    }

    fn can_drive_cluster_line(&self, line_id: LineSnapshotId) -> bool {
        // Only clusters on the uniquely resolved source and destination row may use the
        // caret boundary. A route crossing rows stays on text progress for its whole life.
        matches!(
            (self.from_line, self.to_line),
            (Some(from), Some(to)) if from == to && from == line_id.into()
        )
    }

    fn drives_committed_slice(
        &self,
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
        if source.canonical_range != Some(mapped_source_range)
            && source.canonical_range != Some(target.byte_range)
        {
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

    fn drives_new_reveal(&self, target: &TargetCluster, offset_map: &OffsetMap) -> bool {
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

    fn matches(&self, caret: VisualCaretGeometry) -> bool {
        caret.movement_source == CursorMoveSource::TextTransaction
            && caret.transition_id == self.transition_id
            && caret.layout_revision == Some(self.layout_revision)
            && caret.document_session == self.document_session
            && (caret.target_x - self.to.x).abs() <= 0.5
            && (caret.target_y - self.to.y).abs() <= 0.5
            && self.path.as_ref().map_or_else(
                || {
                    point_is_on_route(
                        caret.x,
                        caret.y,
                        caret.path_start_x,
                        caret.path_start_y,
                        caret.target_x,
                        caret.target_y,
                    )
                },
                |path| path.contains_point(caret.x, caret.y),
            )
    }

    fn matches_committed_slice(&self, target: &TargetCluster, caret: VisualCaretGeometry) -> bool {
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

    fn matches_new_reveal(&self, target: &TargetCluster, caret: VisualCaretGeometry) -> bool {
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
    /// Cross-line glyphs follow the shared caret path while retaining their per-line offset.
    reflow_offsets: Option<CaretReflowOffsets>,
    terminal_geometry_committed: bool,
    /// Width represented by the latest Qt-submitted frame, never a staged plan.
    submitted_visible_width: f64,
}

#[derive(Clone, Copy, Debug)]
struct CaretReflowOffsets {
    source_x: f64,
    source_y: f64,
    target_x: f64,
    target_y: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct VisualEditState {
    pub transition_id: u64,
    pub transaction_id: u64,
    pub operation_kind: String,
    pub source_frame: VisualFrame,
    pub target_snapshot: EditorLayoutSnapshot,
    motions: Vec<ClusterMotion>,
    timeline: EditVisualTimeline,
    pub reflow_cluster_count: usize,
    pub crossfade_cluster_count: usize,
    pub ownership_conflict_count: usize,
    pub first_committed_frame_id: Option<u64>,
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
        inherited_spatial_speed_per_second: Option<f64>,
        inherited_caret_velocity: Option<CursorRect>,
        inherited_shared_progress: f64,
        transaction_id: u64,
        operation_kind: String,
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
                    transition_id,
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
                    reflow_offsets: None,
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
                    let mut contribution = source.clone();
                    contribution.canonical_range = Some(mapped_range);
                    let owner_motion_index = motions
                        .iter()
                        .enumerate()
                        .filter_map(|(index, motion)| {
                            let target = motion.target.as_ref()?;
                            ranges_overlap(mapped_range, target.byte_range).then_some((
                                index,
                                target.shaping_identity == contribution.shaping_identity,
                                overlap_len(mapped_range, target.byte_range),
                            ))
                        })
                        .max_by_key(|(_, same_shaping, overlap)| (*same_shaping, *overlap))
                        .map(|(index, _, _)| index);
                    if let Some(motion) =
                        owner_motion_index.and_then(|index| motions.get_mut(index))
                    {
                        if !motion
                            .source
                            .iter()
                            .chain(motion.retiring_sources.iter())
                            .any(|existing| same_visual_cluster(existing, &contribution))
                        {
                            if let Some(target) = motion.target.as_ref() {
                                if motion.source.is_none() {
                                    motion.kind = if contribution.shaping_identity
                                        == target.shaping_identity
                                    {
                                        MotionKind::Transform
                                    } else {
                                        MotionKind::CrossFade
                                    };
                                    motion.source = Some(contribution);
                                } else {
                                    if contribution.shaping_identity != target.shaping_identity {
                                        motion.kind = MotionKind::CrossFade;
                                    }
                                    motion.retiring_sources.push(contribution);
                                }
                            }
                        }
                        source_used[source_index] = true;
                        continue;
                    }
                    // If no target cluster exists, retain the submitted glyph until its
                    // source-only transition has faded it out from the actual source rect.
                    motions.push(ClusterMotion {
                        source: Some(contribution),
                        retiring_sources: Vec::new(),
                        target: None,
                        source_canonical_range: Some(mapped_range),
                        kind: MotionKind::CrossFade,
                        caret_motion: None,
                        reflow_offsets: None,
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
                    reflow_offsets: None,
                    terminal_geometry_committed: false,
                    submitted_visible_width: 0.0,
                });
            }
        }

        if let Some(caret_motion) = caret_motion.as_ref() {
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
                    motion.caret_motion = Some(caret_motion.clone());
                }
            }
        }
        if let Some(caret_motion) = caret_motion.as_ref() {
            if caret_motion.path.is_some() {
                for motion in &mut motions {
                    let (Some(source), Some(target)) = (&motion.source, &motion.target) else {
                        continue;
                    };
                    if VisualLineIdentity::from(source.snapshot_id)
                        == VisualLineIdentity::from(target.snapshot_id)
                    {
                        continue;
                    }
                    motion.reflow_offsets = Some(CaretReflowOffsets {
                        source_x: source.rect.x - caret_motion.from.x,
                        source_y: source.rect.y - caret_motion.from.y,
                        target_x: target.rect.x - caret_motion.to.x,
                        target_y: target.rect.y - caret_motion.to.y,
                    });
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

        let response_distance = response_distance(&motions, caret_motion.as_ref());
        let timeline = EditVisualTimeline::new(
            transition_id,
            document_session,
            target_snapshot.revision,
            now,
            base_duration_ms,
            caret_rects.map(|(from, _)| from),
            caret_rects.map(|(_, to)| to),
            caret_motion.as_ref().and_then(|motion| motion.path.clone()),
            inherited_spatial_speed_per_second,
            inherited_caret_velocity,
            response_distance,
            inherited_shared_progress,
        );
        let reflow_cluster_count = motions
            .iter()
            .filter(|motion| {
                matches!(motion.kind, MotionKind::Transform)
                    && motion
                        .source
                        .as_ref()
                        .zip(motion.target.as_ref())
                        .is_some_and(|(source, target)| {
                            (
                                source.snapshot_id.paragraph_id,
                                source.snapshot_id.visual_line_ordinal,
                            ) != (
                                target.snapshot_id.paragraph_id,
                                target.snapshot_id.visual_line_ordinal,
                            )
                        })
            })
            .count();
        let crossfade_cluster_count = motions
            .iter()
            .filter(|motion| matches!(motion.kind, MotionKind::CrossFade))
            .count();
        let mut logical_targets = HashSet::new();
        let mut ownership_conflict_count = 0usize;
        for motion in &motions {
            if let Some(target) = motion.target.as_ref() {
                if !logical_targets.insert((target.byte_range.0, target.byte_range.1)) {
                    ownership_conflict_count += 1;
                }
            }
        }

        Self {
            transition_id,
            transaction_id,
            operation_kind,
            source_frame,
            target_snapshot,
            motions,
            timeline,
            reflow_cluster_count,
            crossfade_cluster_count,
            ownership_conflict_count,
            first_committed_frame_id: None,
            terminal_frame_committed: false,
        }
    }

    pub(crate) fn shift_started_at(&mut self, delta: Duration) {
        self.timeline.shift_started_at(delta);
    }

    pub(crate) fn sample(&self, frame_now: Instant) -> EditMotionSample {
        self.timeline.sample(frame_now)
    }

    pub(crate) fn retime(&mut self, now: Instant, duration_ms: u64) -> f64 {
        self.timeline.retime(now, duration_ms)
    }

    pub(crate) fn duration_ms(&self) -> u64 {
        self.timeline.duration_ms()
    }

    pub(crate) fn has_caret_timeline(&self) -> bool {
        self.timeline.has_caret_motion()
    }

    pub(crate) fn has_caret_driven_motions(&self) -> bool {
        self.motions
            .iter()
            .any(|motion| motion.caret_motion.is_some())
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
        sample: EditMotionSample,
        visual_caret: Option<VisualCaretGeometry>,
    ) -> RenderOwnershipPlan {
        let p = sample.eased_progress;
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
                    let rect = motion_rect(motion, source, target, sample);
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
                    for retiring in &motion.retiring_sources {
                        push_crossfade_source_glyph_at(
                            retiring,
                            Some(target.byte_range),
                            p,
                            &rect,
                            source,
                            &mut glyphs,
                        );
                    }
                }
                MotionKind::RevealFromCommittedSlice => {
                    let (Some(source), Some(target)) = (&motion.source, &motion.target) else {
                        continue;
                    };
                    let start_width = source.rect.w.min(target.rect.w);
                    let boundary = committed_slice_boundary(
                        target,
                        motion.caret_motion.as_ref(),
                        visual_caret,
                    );
                    let proposed_visible = if motion.terminal_geometry_committed {
                        target.rect.w
                    } else {
                        match boundary {
                            MotionBoundary::VisualCaret(caret_x) => start_width.max(
                                reveal_width_to_caret_during_transition(source, target, caret_x, p),
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
                    let rect = motion
                        .reflow_offsets
                        .and_then(|_| sample.caret_rect)
                        .map(|_| {
                            let path_rect = motion_rect(motion, source, target, sample);
                            let x = if source.shaping_identity.direction_rtl {
                                path_rect.x + path_rect.w - visible
                            } else {
                                path_rect.x
                            };
                            SourceRect {
                                x,
                                y: path_rect.y,
                                w: visible,
                                h: path_rect.h,
                            }
                        })
                        .unwrap_or_else(|| lerp_rect(&start_rect, &target_slice, p));
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
                    for retiring in &motion.retiring_sources {
                        push_crossfade_source_glyph_at(
                            retiring,
                            Some(target.byte_range),
                            p,
                            &rect,
                            source,
                            &mut glyphs,
                        );
                    }
                }
                MotionKind::Reveal => {
                    let Some(target) = &motion.target else {
                        continue;
                    };
                    // Coordinated mode uses this frame's already-sampled visual caret as the
                    // reveal edge for clusters on its current line. Cross-line/layout cases
                    // retain the explicit VisualEditState progress geometry.
                    let boundary =
                        new_reveal_boundary(target, motion.caret_motion.as_ref(), visual_caret);
                    let visible = if motion.terminal_geometry_committed {
                        target.rect.w
                    } else {
                        match boundary {
                            MotionBoundary::VisualCaret(caret_x) => {
                                reveal_width_to_caret(target, caret_x)
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
                    let boundary = delete_boundary(
                        source,
                        delete_edge,
                        motion.caret_motion.as_ref(),
                        visual_caret,
                    );
                    let visible = if motion.terminal_geometry_committed {
                        0.0
                    } else {
                        match boundary {
                            MotionBoundary::VisualCaret(caret_x) => {
                                delete_width_to_caret(source, delete_edge, caret_x)
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
                    if let (Some(source), Some(target)) = (&motion.source, &motion.target) {
                        let path_rect = motion_rect(motion, source, target, sample);
                        push_crossfade_source_glyph_at(
                            source,
                            source_canonical_range,
                            p,
                            &path_rect,
                            source,
                            &mut glyphs,
                        );
                        for retiring in &motion.retiring_sources {
                            push_crossfade_source_glyph_at(
                                retiring,
                                source_canonical_range,
                                p,
                                &path_rect,
                                source,
                                &mut glyphs,
                            );
                        }
                        if p > 1e-6 {
                            glyphs.push(RenderOwnershipPlan::glyph(
                                path_rect.x,
                                path_rect.y,
                                path_rect.w,
                                path_rect.h,
                                p,
                                target.snapshot_id,
                                target.source_rect.clone(),
                                target.byte_range,
                                Some(target.byte_range),
                                None,
                                target.shaping_identity.clone(),
                            ));
                        }
                    } else if let Some(source) = &motion.source {
                        push_crossfade_source_glyph(
                            source,
                            motion.source_canonical_range,
                            p,
                            &mut glyphs,
                        );
                        for retiring in &motion.retiring_sources {
                            push_crossfade_source_glyph(
                                retiring,
                                motion.source_canonical_range,
                                p,
                                &mut glyphs,
                            );
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
        plan.shared_progress = sample.eased_progress;
        plan.timeline_progress = sample.timeline_progress;
        plan.spatial_speed_per_second = sample.spatial_speed_per_second;
        plan.caret_velocity = sample.caret_velocity;
        plan.reflow_cluster_count = self.reflow_cluster_count;
        plan.crossfade_cluster_count = self.crossfade_cluster_count;
        plan.ownership_conflict_count = plan
            .ownership_conflict_count
            .max(self.ownership_conflict_count);
        plan.terminal_motion_indices = terminal_motion_indices;
        plan.submitted_visible_widths = submitted_visible_widths;
        plan.candidate_frame = VisualFrame::from_rendered_plan(&self.target_snapshot, &plan);
        plan
    }
}

fn caret_line_identity(
    snapshot: &EditorLayoutSnapshot,
    caret: CaretPosition,
) -> Option<VisualLineIdentity> {
    line_for_caret(snapshot, caret).map(|line| line.id.into())
}

fn line_for_caret(
    snapshot: &EditorLayoutSnapshot,
    caret: CaretPosition,
) -> Option<&PreparedLineSnapshot> {
    let caret_center_y = caret.y + caret.h * 0.5;
    snapshot.line_snapshots.iter().min_by(|left, right| {
        let distance = |line: &PreparedLineSnapshot| {
            if caret_center_y < line.visual_line_top {
                line.visual_line_top - caret_center_y
            } else if caret_center_y > line.visual_line_bottom {
                caret_center_y - line.visual_line_bottom
            } else {
                0.0
            }
        };
        distance(left).total_cmp(&distance(right))
    })
}

fn build_caret_path(
    source_line: &PreparedLineSnapshot,
    target_line: &PreparedLineSnapshot,
    from: CursorRect,
    to: CursorRect,
    caret_byte_offsets: (usize, usize),
) -> Option<CaretPath> {
    let (source_line_left, source_line_right) = line_horizontal_bounds(source_line);
    let (target_line_left, target_line_right) = line_horizontal_bounds(target_line);
    let source_left = source_line_left.min(from.x);
    let source_right = source_line_right.max(from.x);
    let target_left = target_line_left.min(to.x);
    let target_right = target_line_right.max(to.x);
    let forward = caret_byte_offsets.1 >= caret_byte_offsets.0;
    let source_rtl = line_is_rtl(source_line);
    let target_rtl = line_is_rtl(target_line);
    let source_leading = if source_rtl {
        source_right
    } else {
        source_left
    };
    let source_trailing = if source_rtl {
        source_left
    } else {
        source_right
    };
    let target_leading = if target_rtl {
        target_right
    } else {
        target_left
    };
    let target_trailing = if target_rtl {
        target_left
    } else {
        target_right
    };
    let (exit_x, entry_x) = if (from.x - to.x).abs() <= 8.0 {
        // Enter at the current caret boundary when a split or soft wrap keeps the
        // horizontal caret coordinate stable; traversing the whole row would misroute it.
        (from.x, to.x)
    } else {
        (
            if forward {
                source_trailing
            } else {
                source_leading
            },
            if forward {
                target_leading
            } else {
                target_trailing
            },
        )
    };
    let source_exit = CursorRect {
        x: exit_x,
        top: source_line.caret_top,
        bottom: source_line.caret_top + source_line.caret_height,
        baseline_y: from.baseline_y,
    };
    let target_vertical = CursorRect {
        x: exit_x,
        top: target_line.caret_top,
        bottom: target_line.caret_top + target_line.caret_height,
        baseline_y: to.baseline_y,
    };
    let target_entry = CursorRect {
        x: entry_x,
        top: target_line.caret_top,
        bottom: target_line.caret_top + target_line.caret_height,
        baseline_y: to.baseline_y,
    };
    CaretPath::new(vec![from, source_exit, target_vertical, target_entry, to])
}

fn line_horizontal_bounds(line: &PreparedLineSnapshot) -> (f64, f64) {
    let mut left = line.visual_x;
    let mut right = line.visual_x;
    for cluster in &line.clusters {
        let rect = line.source_rect_to_document_rect(&cluster.source_rect);
        left = left.min(rect.x);
        right = right.max(rect.x + rect.w);
    }
    (left, right)
}

fn line_is_rtl(line: &PreparedLineSnapshot) -> bool {
    line.clusters
        .first()
        .is_some_and(|cluster| cluster.shaping_identity.direction_rtl)
}

fn committed_slice_boundary(
    target: &TargetCluster,
    driver: Option<&CaretMotion>,
    visual_caret: Option<VisualCaretGeometry>,
) -> MotionBoundary {
    visual_caret
        .filter(|caret| driver.is_some_and(|driver| driver.matches_committed_slice(target, *caret)))
        .map(|caret| MotionBoundary::VisualCaret(caret.x))
        .unwrap_or(MotionBoundary::TextProgress)
}

fn new_reveal_boundary(
    target: &TargetCluster,
    driver: Option<&CaretMotion>,
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
    driver: Option<&CaretMotion>,
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
        source.canonical_range.unwrap_or(source.byte_range),
        canonical_range,
        None,
        source.shaping_identity.clone(),
    ));
}

fn push_crossfade_source_glyph_at(
    source: &VisualCluster,
    canonical_range: Option<(usize, usize)>,
    progress: f64,
    path_rect: &SourceRect,
    primary: &VisualCluster,
    glyphs: &mut Vec<crate::sujian_editor_item::render_plan::TextAnimationGlyphInfo>,
) {
    let opacity = source.opacity * (1.0 - progress);
    if opacity <= 1e-6 {
        return;
    }
    let offset_scale = 1.0 - progress;
    glyphs.push(RenderOwnershipPlan::glyph(
        path_rect.x + (source.rect.x - primary.rect.x) * offset_scale,
        path_rect.y + (source.rect.y - primary.rect.y) * offset_scale,
        lerp(source.rect.w, path_rect.w, progress),
        lerp(source.rect.h, path_rect.h, progress),
        opacity,
        source.snapshot_id,
        source.source_rect.clone(),
        source.canonical_range.unwrap_or(source.byte_range),
        canonical_range,
        None,
        source.shaping_identity.clone(),
    ));
}

fn motion_rect(
    motion: &ClusterMotion,
    source: &VisualCluster,
    target: &TargetCluster,
    sample: EditMotionSample,
) -> SourceRect {
    let p = sample.eased_progress;
    if let (Some(offsets), Some(caret)) = (motion.reflow_offsets, sample.caret_rect) {
        return SourceRect {
            x: caret.x + lerp(offsets.source_x, offsets.target_x, p),
            y: caret.top + lerp(offsets.source_y, offsets.target_y, p),
            w: lerp(source.rect.w, target.rect.w, p),
            h: lerp(source.rect.h, target.rect.h, p),
        };
    }
    lerp_rect(&source.rect, &target.rect, p)
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
    // Keep each submitted contribution at its actual geometry. Only byte-for-byte
    // duplicates of the same texture, crop, rect, and opacity are redundant.
    let mut source_groups: Vec<VisualCluster> = Vec::new();
    for &source_index in source_indices {
        let contribution = source_frame.clusters[source_index].clone();
        if !source_groups
            .iter()
            .any(|existing| same_visual_cluster(existing, &contribution))
        {
            source_groups.push(contribution);
        }
    }
    for source in &mut source_groups {
        if let Some(range) = source.canonical_range {
            source.canonical_range = offset_map
                .map_old_range_to_new(range.0, range.1)
                .or(Some(range));
        }
    }
    let primary_source_index = (0..source_groups.len())
        .max_by(|left_index, right_index| {
            let left = &source_groups[*left_index];
            let right = &source_groups[*right_index];
            let left_same_shaping = left.shaping_identity == target.shaping_identity;
            let right_same_shaping = right.shaping_identity == target.shaping_identity;
            let left_overlap = left
                .canonical_range
                .map(|range| overlap_len(range, target.byte_range))
                .unwrap_or(0);
            let right_overlap = right
                .canonical_range
                .map(|range| overlap_len(range, target.byte_range))
                .unwrap_or(0);
            left_same_shaping
                .cmp(&right_same_shaping)
                .then_with(|| left_overlap.cmp(&right_overlap))
                .then_with(|| left.opacity.total_cmp(&right.opacity))
        })
        .unwrap_or(0);
    let source = source_groups[primary_source_index].clone();
    let same_shaping = source_groups
        .iter()
        .all(|source| source.shaping_identity == target.shaping_identity);
    // A target has one motion owner. Every distinct committed contribution stays attached
    // to it; each old layer follows its own source geometry into the target path.
    let retiring_sources: Vec<VisualCluster> = if source_groups.len() == 1 {
        Vec::new()
    } else {
        source_groups
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != primary_source_index)
            .map(|(_, source)| source.clone())
            .collect()
    };
    for &source_index in source_indices {
        source_used[source_index] = true;
    }
    target_used[target_index] = true;

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
    let kind = if same_shaping && source_groups.len() == 1 && starts_from_partial_width {
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
        reflow_offsets: None,
        terminal_geometry_committed: false,
        submitted_visible_width,
    });
}

fn same_visual_cluster(left: &VisualCluster, right: &VisualCluster) -> bool {
    left.snapshot_id == right.snapshot_id
        && left.byte_range == right.byte_range
        && left.canonical_range == right.canonical_range
        && left.delete_edge == right.delete_edge
        && left.shaping_identity == right.shaping_identity
        && left.rect.x == right.rect.x
        && left.rect.y == right.rect.y
        && left.rect.w == right.rect.w
        && left.rect.h == right.rect.h
        && left.source_rect == right.source_rect
        && left.opacity == right.opacity
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

fn response_distance(motions: &[ClusterMotion], caret_motion: Option<&CaretMotion>) -> f64 {
    let caret_distance = caret_motion
        .and_then(|motion| {
            motion.path.as_ref().map(CaretPath::length).or_else(|| {
                let from = CursorRect {
                    x: motion.from.x,
                    top: motion.from.y,
                    bottom: motion.from.y + motion.from.h,
                    baseline_y: motion.from.y + motion.from.h,
                };
                let to = CursorRect {
                    x: motion.to.x,
                    top: motion.to.y,
                    bottom: motion.to.y + motion.to.h,
                    baseline_y: motion.to.y + motion.to.h,
                };
                Some(
                    (from.x - to.x)
                        .hypot(from.top - to.top)
                        .max((from.bottom - to.bottom).abs())
                        .max((from.baseline_y - to.baseline_y).abs()),
                )
            })
        })
        .unwrap_or_default();
    let text_distance = motions
        .iter()
        .map(|motion| match (&motion.source, &motion.target) {
            (Some(source), Some(target)) => rect_distance(&source.rect, &target.rect),
            (Some(source), None) => source.rect.w.max(source.rect.h),
            (None, Some(target)) => target.rect.w.max(target.rect.h),
            (None, None) => 0.0,
        })
        .fold(0.0, f64::max);
    caret_distance.max(text_distance).max(1.0)
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
