//! 上一帧已成功提交到 Qt Scene Graph 的正文视觉事实。
//!
//! 该快照只记录画面结果，不包含动画时钟或历史事务。新编辑只能从成功渲染的
//! 这份快照开始，静态 cluster 和动画 glyph 都在同一个列表里。

use super::super::layout_snapshot::{EditorLayoutSnapshot, ShapingIdentity, SourceRect};
use super::super::render_ownership::RenderOwnershipPlan;
use super::super::snapshot_id::LineSnapshotId;

#[derive(Clone, Debug)]
pub(crate) struct VisualCluster {
    pub snapshot_id: LineSnapshotId,
    pub byte_range: (usize, usize),
    pub shaping_identity: ShapingIdentity,
    pub rect: SourceRect,
    pub source_rect: SourceRect,
    pub opacity: f64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct VisualFrame {
    pub clusters: Vec<VisualCluster>,
}

impl VisualFrame {
    /// Materialize exactly what is on screen after a successful frame commit.
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
                    shaping_identity: cluster.shaping_identity.clone(),
                    rect,
                    source_rect: cluster.source_rect.clone(),
                    opacity: 1.0,
                });
            }
        }

        for glyph in &ownership.animated_glyphs {
            clusters.push(VisualCluster {
                snapshot_id: glyph.snapshot_id,
                byte_range: glyph.logical_range,
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
        Self { clusters }
    }

    pub(crate) fn from_static_snapshot(snapshot: &EditorLayoutSnapshot) -> Self {
        let mut clusters = Vec::new();
        for line in &snapshot.line_snapshots {
            for cluster in &line.clusters {
                clusters.push(VisualCluster {
                    snapshot_id: line.id,
                    byte_range: (cluster.byte_start, cluster.byte_end),
                    shaping_identity: cluster.shaping_identity.clone(),
                    rect: line.source_rect_to_document_rect(&cluster.source_rect),
                    source_rect: cluster.source_rect.clone(),
                    opacity: 1.0,
                });
            }
        }
        Self { clusters }
    }
}
