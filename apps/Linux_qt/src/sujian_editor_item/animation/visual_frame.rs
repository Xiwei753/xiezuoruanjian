//! 收到 Qt 帧提交回执后的正文视觉事实。
//!
//! 该快照只记录画面结果，不包含动画时钟或历史事务。新编辑只能从成功渲染的
//! 这份快照开始，静态 cluster 和动画 glyph 都在同一个列表里。

use super::super::layout_revision::LayoutRevision;
use super::super::layout_snapshot::{EditorLayoutSnapshot, ShapingIdentity, SourceRect};
use super::super::render_ownership::RenderOwnershipPlan;
use super::super::snapshot_id::LineSnapshotId;
use crate::sujian_editor_item::edit_motion::{CursorRect, DeleteEdge};

/// Lightweight row geometry captured with the frame Qt actually submitted.
/// It deliberately excludes the row image and cluster payloads.
#[derive(Clone, Debug)]
pub(crate) struct VisualLineGeometry {
    pub id: LineSnapshotId,
    pub byte_start: usize,
    pub byte_end: usize,
    pub top: f64,
    pub bottom: f64,
    pub caret_top: f64,
    pub caret_height: f64,
    pub left: f64,
    pub right: f64,
    pub direction_rtl: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct VisualCluster {
    pub snapshot_id: LineSnapshotId,
    /// 纹理/旧 glyph 所属 layout 中的范围。
    pub byte_range: (usize, usize),
    /// 当前 frame canonical revision 中的逻辑身份；删除中的旧 glyph 可为 None。
    pub canonical_range: Option<(usize, usize)>,
    /// 删除中的 glyph 保留本次采用的 caret 边缘，供后续 retarget 延续方向。
    pub delete_edge: Option<DeleteEdge>,
    pub shaping_identity: ShapingIdentity,
    pub rect: SourceRect,
    pub source_rect: SourceRect,
    pub opacity: f64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct VisualFrame {
    /// 文档视觉会话。layout revision 可能在切章时重用，因此不能单独证明 source 有效。
    pub document_session: u64,
    pub canonical_revision: Option<LayoutRevision>,
    /// Static ownership is 0; animated ownership is tied to the frame submission ticket.
    pub ownership_revision: u64,
    pub canonical_byte_len: usize,
    /// Caret geometry from the frame Qt actually submitted. New edit timelines use this
    /// as their source when retargeting an in-flight visual edit.
    pub caret_rect: Option<CursorRect>,
    /// Screen-space motion speed captured from the same submitted timeline sample.
    pub spatial_speed_per_second: f64,
    /// Actual caret velocity at the frame accepted by Qt.
    pub caret_velocity: Option<CursorRect>,
    /// Row the submitted caret actually occupies. `None` is meaningful for a caret
    /// between rows while a cross-line path is in flight.
    pub caret_line_snapshot_id: Option<LineSnapshotId>,
    /// Row geometry from the same submitted visual frame, including rows that still
    /// contribute old glyphs during a reflow or cross-fade.
    pub line_geometries: Vec<VisualLineGeometry>,
    /// Monotonic diagnostic phase captured from the same submitted timeline sample.
    pub timeline_progress: f64,
    /// Captured visual contributions, not a one-entry-per-character index. A CrossFade
    /// can leave several layers with the same canonical range; preserve each rendered
    /// layer here and let the next edit group them back into one logical target cluster.
    pub clusters: Vec<VisualCluster>,
}

impl VisualFrame {
    /// Materialize the visual candidate represented by this rendered plan.
    pub(crate) fn from_rendered_plan(
        snapshot: &EditorLayoutSnapshot,
        ownership: &RenderOwnershipPlan,
        inherited_line_geometries: &[VisualLineGeometry],
    ) -> Self {
        if ownership.handoff_pending {
            return Self::from_static_snapshot(snapshot);
        }

        let mut clusters = Vec::new();
        for line in &snapshot.line_snapshots {
            for cluster in &line.clusters {
                let key = super::super::render_ownership::ClusterOwnerKey {
                    snapshot_id: line.id,
                    byte_range: (cluster.byte_start, cluster.byte_end),
                };
                if ownership.owns_animation_cluster(&key) {
                    continue;
                }
                let rect = line.source_rect_to_document_rect(&cluster.source_rect);
                clusters.push(VisualCluster {
                    snapshot_id: line.id,
                    byte_range: (cluster.byte_start, cluster.byte_end),
                    canonical_range: Some((cluster.byte_start, cluster.byte_end)),
                    delete_edge: None,
                    shaping_identity: cluster.shaping_identity.clone(),
                    rect,
                    source_rect: cluster.source_rect.clone(),
                    opacity: 1.0,
                });
            }
        }

        // Keep every committed layer. Multiple glyphs can represent one canonical cluster
        // during a CrossFade and must not be mistaken for separate logical characters.
        for glyph in &ownership.animated_glyphs {
            clusters.push(VisualCluster {
                snapshot_id: glyph.snapshot_id,
                byte_range: glyph.logical_range,
                canonical_range: glyph.canonical_range,
                delete_edge: glyph.delete_edge,
                shaping_identity: glyph.shaping_identity.clone(),
                rect: SourceRect {
                    x: glyph.x,
                    y: glyph.y,
                    w: glyph.w,
                    h: glyph.h,
                },
                source_rect: glyph.source_rect.clone(),
                opacity: glyph.opacity,
            });
        }
        let mut line_geometries = snapshot
            .line_snapshots
            .iter()
            .map(VisualLineGeometry::from)
            .collect::<Vec<_>>();
        for glyph in &ownership.animated_glyphs {
            if line_geometries
                .iter()
                .any(|line| line.id == glyph.snapshot_id)
            {
                continue;
            }
            if let Some(line) = inherited_line_geometries
                .iter()
                .find(|line| line.id == glyph.snapshot_id)
            {
                line_geometries.push(line.clone());
            }
        }
        Self {
            document_session: 0,
            canonical_revision: Some(snapshot.revision),
            ownership_revision: ownership.ownership_revision,
            canonical_byte_len: canonical_byte_len(snapshot),
            caret_rect: None,
            spatial_speed_per_second: ownership.spatial_speed_per_second,
            caret_velocity: ownership.caret_velocity,
            caret_line_snapshot_id: None,
            line_geometries,
            timeline_progress: ownership.timeline_progress,
            clusters,
        }
    }

    pub(crate) fn from_static_snapshot(snapshot: &EditorLayoutSnapshot) -> Self {
        let mut clusters = Vec::new();
        for line in &snapshot.line_snapshots {
            for cluster in &line.clusters {
                clusters.push(VisualCluster {
                    snapshot_id: line.id,
                    byte_range: (cluster.byte_start, cluster.byte_end),
                    canonical_range: Some((cluster.byte_start, cluster.byte_end)),
                    delete_edge: None,
                    shaping_identity: cluster.shaping_identity.clone(),
                    rect: line.source_rect_to_document_rect(&cluster.source_rect),
                    source_rect: cluster.source_rect.clone(),
                    opacity: 1.0,
                });
            }
        }
        Self {
            document_session: 0,
            canonical_revision: Some(snapshot.revision),
            ownership_revision: 0,
            canonical_byte_len: canonical_byte_len(snapshot),
            caret_rect: None,
            spatial_speed_per_second: 0.0,
            caret_velocity: None,
            caret_line_snapshot_id: None,
            line_geometries: snapshot
                .line_snapshots
                .iter()
                .map(VisualLineGeometry::from)
                .collect(),
            timeline_progress: 0.0,
            clusters,
        }
    }

    pub(crate) fn line_id_aligned_with_caret(
        &self,
        caret: CursorRect,
        preferred: Option<LineSnapshotId>,
    ) -> Option<LineSnapshotId> {
        let height = (caret.bottom - caret.top).max(0.0);
        let candidates: Vec<_> = self
            .line_geometries
            .iter()
            .filter(|line| {
                (caret.top - line.caret_top).abs() <= 1.5
                    && (height - line.caret_height).abs() <= 1.5
                    && caret.top >= line.top - 1.5
                    && caret.bottom <= line.bottom + 1.5
            })
            .map(|line| line.id)
            .collect();
        match candidates.as_slice() {
            [line_id] => Some(*line_id),
            _ => preferred.filter(|line_id| candidates.contains(line_id)),
        }
    }
}

impl From<&super::super::layout_snapshot::PreparedLineSnapshot> for VisualLineGeometry {
    fn from(line: &super::super::layout_snapshot::PreparedLineSnapshot) -> Self {
        let (left, right) =
            line.clusters
                .iter()
                .fold((line.visual_x, line.visual_x), |(left, right), cluster| {
                    let rect = line.source_rect_to_document_rect(&cluster.source_rect);
                    (left.min(rect.x), right.max(rect.x + rect.w))
                });
        Self {
            id: line.id,
            byte_start: line.byte_start,
            byte_end: line.byte_end,
            top: line.visual_line_top,
            bottom: line.visual_line_bottom,
            caret_top: line.caret_top,
            caret_height: line.caret_height,
            left,
            right,
            direction_rtl: line
                .clusters
                .first()
                .is_some_and(|cluster| cluster.shaping_identity.direction_rtl),
        }
    }
}

fn canonical_byte_len(snapshot: &EditorLayoutSnapshot) -> usize {
    snapshot
        .line_snapshots
        .iter()
        .map(|line| line.byte_end)
        .max()
        .unwrap_or(0)
}
