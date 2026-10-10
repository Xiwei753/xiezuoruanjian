//! 收到 Qt 帧提交回执后的正文视觉事实。
//!
//! 该快照只记录画面结果，不包含动画时钟或历史事务。新编辑只能从成功渲染的
//! 这份快照开始，静态 cluster 和动画 glyph 都在同一个列表里。

use super::super::layout_revision::LayoutRevision;
use super::super::layout_snapshot::{EditorLayoutSnapshot, ShapingIdentity, SourceRect};
use super::super::render_ownership::RenderOwnershipPlan;
use super::super::snapshot_id::LineSnapshotId;
use crate::sujian_editor_item::edit_motion::{CursorRect, DeleteEdge};

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
        Self {
            document_session: 0,
            canonical_revision: Some(snapshot.revision),
            ownership_revision: ownership.ownership_revision,
            canonical_byte_len: canonical_byte_len(snapshot),
            caret_rect: None,
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
            clusters,
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
