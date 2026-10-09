//! 单一的正文视觉过渡。
//!
//! 每次编辑都丢弃上一份 transition，从最近成功提交的 VisualFrame 直接构造新目标。
//! 不保存 burst、stage、travelling distance 或排队中的编辑历史。

use std::time::{Duration, Instant};

use writer_core::editor::OffsetMap;

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
    Reveal,
    Delete(DeleteEdge),
    CrossFade,
}

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

#[derive(Clone, Copy, Debug)]
struct CaretMotion {
    from: CaretPosition,
    to: CaretPosition,
    layout_revision: LayoutRevision,
    document_session: u64,
}

impl CaretMotion {
    fn new(
        from: CursorRect,
        to: CursorRect,
        layout_revision: LayoutRevision,
        document_session: u64,
    ) -> Self {
        Self {
            from: CaretPosition {
                x: from.x,
                y: from.top,
                h: (from.bottom - from.top).max(0.0),
            },
            to: CaretPosition {
                x: to.x,
                y: to.top,
                h: (to.bottom - to.top).max(0.0),
            },
            layout_revision,
            document_session,
        }
    }

    fn touches_line(self, rect: &SourceRect) -> bool {
        let from_center = self.from.y + self.from.h * 0.5;
        let to_center = self.to.y + self.to.h * 0.5;
        let min_y = from_center.min(to_center);
        let max_y = from_center.max(to_center);
        max_y >= rect.y - 2.0 && min_y <= rect.y + rect.h + 2.0
    }

    fn matches(self, caret: VisualCaretGeometry) -> bool {
        caret.movement_source == CursorMoveSource::TextTransaction
            && caret.layout_revision == Some(self.layout_revision)
            && caret.document_session == self.document_session
            && (caret.target_x - self.to.x).abs() <= 0.5
            && (caret.target_y - self.to.y).abs() <= 0.5
            && (caret.path_start_x - self.from.x).abs() <= 0.5
            && (caret.path_start_y - self.from.y).abs() <= 0.5
            && point_is_on_route(
                caret.x,
                caret.y,
                caret.path_start_x,
                caret.path_start_y,
                caret.target_x,
                caret.target_y,
            )
    }
}

#[derive(Clone, Debug)]
struct ClusterMotion {
    source: Option<VisualCluster>,
    target: Option<TargetCluster>,
    kind: MotionKind,
    caret_motion: Option<CaretMotion>,
}

#[derive(Clone, Debug)]
pub(crate) struct VisualEditState {
    pub source_frame: VisualFrame,
    pub target_snapshot: EditorLayoutSnapshot,
    motions: Vec<ClusterMotion>,
    pub started_at: Instant,
    pub duration_ms: u64,
    pub input_interval_ms: u64,
    pub visual_lag_px: f64,
    /// 终点帧已成功绘制；下一帧才可请求 Static ownership handoff。
    pub terminal_frame_committed: bool,
}

impl VisualEditState {
    pub(crate) fn new(
        committed_frame: Option<&VisualFrame>,
        base_snapshot: &EditorLayoutSnapshot,
        target_snapshot: EditorLayoutSnapshot,
        frame_to_base_map: &OffsetMap,
        offset_map: &OffsetMap,
        deleted_range_edges: &[DeletedRangeEdge],
        now: Instant,
        base_duration_ms: u64,
        previous_edit_at: Option<Instant>,
        caret_rects: Option<(CursorRect, CursorRect)>,
        document_session: u64,
    ) -> Self {
        let source_frame = committed_frame
            .cloned()
            .unwrap_or_else(|| VisualFrame::from_static_snapshot(base_snapshot));
        let targets = target_clusters(&target_snapshot);
        let caret_motion = caret_rects.map(|(from, to)| {
            CaretMotion::new(from, to, target_snapshot.revision, document_session)
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
            let exact = source_frame
                .clusters
                .iter()
                .enumerate()
                .filter(|(index, source)| {
                    !source_used[*index] && source.canonical_range == Some(old_range)
                })
                .max_by(|(_, left), (_, right)| {
                    let left_same = left.shaping_identity == target.shaping_identity;
                    let right_same = right.shaping_identity == target.shaping_identity;
                    left_same
                        .cmp(&right_same)
                        .then_with(|| left.opacity.total_cmp(&right.opacity))
                })
                .map(|(index, _)| index);
            if let Some(source_index) = exact {
                pair_cluster(
                    source_index,
                    target_index,
                    &source_frame,
                    &targets,
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
            let overlapping = source_frame
                .clusters
                .iter()
                .enumerate()
                .filter_map(|(index, source)| {
                    let canonical_range = source.canonical_range?;
                    let projected =
                        offset_map.map_old_range_to_new(canonical_range.0, canonical_range.1)?;
                    (!source_used[index] && ranges_overlap(projected, target.byte_range))
                        .then_some((index, projected))
                })
                .max_by(|(left_index, left_range), (right_index, right_range)| {
                    overlap_len(*left_range, target.byte_range)
                        .cmp(&overlap_len(*right_range, target.byte_range))
                        .then_with(|| {
                            source_frame.clusters[*left_index]
                                .opacity
                                .total_cmp(&source_frame.clusters[*right_index].opacity)
                        })
                })
                .map(|(index, _)| index);
            if let Some(source_index) = overlapping {
                pair_cluster(
                    source_index,
                    target_index,
                    &source_frame,
                    &targets,
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
                    target: Some(target.clone()),
                    kind: MotionKind::Reveal,
                    caret_motion: None,
                });
            }
        }
        for (source_index, source) in source_frame.clusters.iter().enumerate() {
            if !source_used[source_index] {
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
                    target: None,
                    kind: MotionKind::Delete(delete_edge),
                    caret_motion: None,
                });
            }
        }

        if let Some(caret_motion) = caret_motion {
            for motion in &mut motions {
                let rect = match (&motion.source, &motion.target, motion.kind) {
                    (None, Some(target), MotionKind::Reveal) => Some(&target.rect),
                    (Some(source), None, MotionKind::Delete(_)) => Some(&source.rect),
                    _ => None,
                };
                if rect.is_some_and(|rect| caret_motion.touches_line(rect)) {
                    motion.caret_motion = Some(caret_motion);
                }
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

        Self {
            source_frame,
            target_snapshot,
            motions,
            started_at: now,
            duration_ms,
            input_interval_ms,
            visual_lag_px,
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
        1.0 - (1.0 - linear).powi(3)
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
        let mut terminal_frame = true;
        let mut owned = Vec::new();
        let mut glyphs = Vec::new();

        for motion in &self.motions {
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
                }
                MotionKind::Reveal => {
                    let Some(target) = &motion.target else {
                        continue;
                    };
                    // Coordinated mode uses this frame's already-sampled visual caret as the
                    // reveal edge for clusters on its current line. Cross-line/layout cases
                    // retain the explicit VisualEditState progress geometry.
                    let boundary = reveal_boundary(target, motion.caret_motion, visual_caret);
                    let visible = match boundary {
                        MotionBoundary::VisualCaret(caret_x) => {
                            reveal_width_to_caret(target, caret_x)
                        }
                        MotionBoundary::TextProgress => target.rect.w * p,
                    };
                    terminal_frame &= match boundary {
                        MotionBoundary::VisualCaret(_) => visible >= target.rect.w - 1e-6,
                        MotionBoundary::TextProgress => p >= 1.0,
                    };
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
                    let visible = match boundary {
                        MotionBoundary::VisualCaret(caret_x) => {
                            delete_width_to_caret(source, delete_edge, caret_x)
                        }
                        MotionBoundary::TextProgress => source.rect.w * (1.0 - p),
                    };
                    terminal_frame &= match boundary {
                        MotionBoundary::VisualCaret(_) => visible <= 1e-6 || source.opacity <= 1e-6,
                        MotionBoundary::TextProgress => p >= 1.0,
                    };
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
                    if let Some(source) = &motion.source {
                        if source.opacity * (1.0 - p) > 1e-6 {
                            glyphs.push(RenderOwnershipPlan::glyph(
                                source.rect.x,
                                source.rect.y,
                                source.rect.w,
                                source.rect.h,
                                source.opacity * (1.0 - p),
                                source.snapshot_id,
                                source.source_rect.clone(),
                                source.byte_range,
                                motion.target.as_ref().map(|target| target.byte_range),
                                None,
                                source.shaping_identity.clone(),
                            ));
                        }
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

        RenderOwnershipPlan::from_owner_table(
            &self.target_snapshot,
            owned,
            glyphs,
            handoff_pending,
            terminal_frame,
        )
    }
}

fn caret_is_on_line(caret: VisualCaretGeometry, rect: &SourceRect) -> bool {
    let caret_center_y = caret.y + caret.h * 0.5;
    caret_center_y >= rect.y - 0.5 && caret_center_y <= rect.y + rect.h + 0.5
}

fn reveal_boundary(
    target: &TargetCluster,
    driver: Option<CaretMotion>,
    visual_caret: Option<VisualCaretGeometry>,
) -> MotionBoundary {
    visual_caret
        .filter(|caret| driver.is_some_and(|driver| driver.matches(*caret)))
        .filter(|caret| caret_is_on_line(*caret, &target.rect))
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
        .filter(|caret| driver.is_some_and(|driver| driver.matches(*caret)))
        .filter(|caret| caret_is_on_line(*caret, &source.rect))
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

fn reveal_width_to_caret(target: &TargetCluster, caret_x: f64) -> f64 {
    let right = target.rect.x + target.rect.w;
    let width = if target.shaping_identity.direction_rtl {
        right - caret_x
    } else {
        caret_x - target.rect.x
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

fn pair_cluster(
    source_index: usize,
    target_index: usize,
    source_frame: &VisualFrame,
    targets: &[TargetCluster],
    source_used: &mut [bool],
    target_used: &mut [bool],
    motions: &mut Vec<ClusterMotion>,
) {
    let source = source_frame.clusters[source_index].clone();
    let target = targets[target_index].clone();
    source_used[source_index] = true;
    target_used[target_index] = true;

    let same_shaping = source.shaping_identity == target.shaping_identity;
    let moved = rect_distance(&source.rect, &target.rect) > 0.01
        || (source.rect.w - target.rect.w).abs() > 0.01
        || (source.rect.h - target.rect.h).abs() > 0.01
        || source.opacity < 0.999
        || (source.source_rect.w - target.source_rect.w).abs() > 0.01;
    if same_shaping && !moved {
        return;
    }
    let starts_from_partial_visual = source.opacity < 0.999
        || (source.rect.w - target.rect.w).abs() > 0.01
        || (source.source_rect.w - target.source_rect.w).abs() > 0.01;
    motions.push(ClusterMotion {
        source: Some(source),
        target: Some(target),
        kind: if same_shaping && !starts_from_partial_visual {
            MotionKind::Transform
        } else {
            MotionKind::CrossFade
        },
        caret_motion: None,
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
    ((cadence_ms as f64 / compression).round() as u64).clamp(12.min(base_ms), base_ms)
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
