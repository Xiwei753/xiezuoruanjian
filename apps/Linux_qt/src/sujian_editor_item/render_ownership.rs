//! 每帧 cluster 静态/动画所有权的唯一计划。

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use super::animation::visual_frame::VisualFrame;
use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId, ShapingIdentity};
use super::qt_text_node::AnimationClipRect;
use super::render_plan::TextAnimationGlyphInfo;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ClusterOwnerKey {
    pub snapshot_id: LineSnapshotId,
    pub byte_range: (usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClusterVisualOwner {
    Static,
    Animation,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RenderOwnershipPlan {
    /// 当前 canonical 快照中的每个 target cluster 都有且只有一个 owner。
    pub cluster_owners: HashMap<ClusterOwnerKey, ClusterVisualOwner>,
    /// 同一 owner table 投影出的静态层 exclusions。
    pub static_exclusions: Vec<AnimationClipRect>,
    /// 同一 owner table 投影出的动画 glyph。
    pub animated_glyphs: Vec<TextAnimationGlyphInfo>,
    /// 对 owner table 的签名；空集合为 0。
    pub ownership_revision: u64,
    /// 文档视觉会话。跨章节的 plan/frame 不能互相作为视觉 source。
    pub document_session: u64,
    /// 当前帧是否已到完整动画终点。
    pub terminal_frame: bool,
    /// Reveal/Delete motions whose terminal geometry was successfully committed.
    pub terminal_motion_indices: Vec<usize>,
    /// RevealFromCommittedSlice widths represented by this candidate frame. These become
    /// monotonic floors only after the matching Qt frame submission is acknowledged.
    pub submitted_visible_widths: Vec<(usize, f64)>,
    /// 动画到达终点。本次 render 需先让静态层成功接管，再清空动画层。
    pub handoff_pending: bool,
    pub target_layout_revision: Option<LayoutRevision>,
    /// 该候选帧收到 Qt 提交回执后，才成为下一次编辑可读取的视觉起点。
    pub candidate_frame: VisualFrame,
    /// 纹理缺失时 canonical 静态层的实际视觉状态。
    pub canonical_frame: VisualFrame,
}

impl RenderOwnershipPlan {
    pub(crate) fn from_owner_table(
        snapshot: &EditorLayoutSnapshot,
        requested_animation_owners: Vec<ClusterOwnerKey>,
        mut animated_glyphs: Vec<TextAnimationGlyphInfo>,
        handoff_pending: bool,
        terminal_frame: bool,
    ) -> Self {
        let requested: HashSet<ClusterOwnerKey> = requested_animation_owners.into_iter().collect();
        let mut cluster_owners = HashMap::new();
        let mut matched_requests = HashSet::new();
        let mut cluster_count = 0usize;

        for line in &snapshot.line_snapshots {
            for cluster in &line.clusters {
                cluster_count += 1;
                let key = ClusterOwnerKey {
                    snapshot_id: line.id,
                    byte_range: (cluster.byte_start, cluster.byte_end),
                };
                let owner = if requested.contains(&key) {
                    matched_requests.insert(key.clone());
                    ClusterVisualOwner::Animation
                } else {
                    ClusterVisualOwner::Static
                };
                cluster_owners.insert(key, owner);
            }
        }

        // Owner request 与 canonical cluster 不一致时 fail closed：整帧回到 canonical，
        // 并在成功同步场景图后收掉这份无效 transition，不能产生 orphan glyph/exclusion。
        let owner_table_matches =
            matched_requests.len() == requested.len() && cluster_owners.len() == cluster_count;
        if !owner_table_matches {
            for owner in cluster_owners.values_mut() {
                *owner = ClusterVisualOwner::Static;
            }
            animated_glyphs.clear();
        }
        let mut static_exclusions = Vec::new();
        for line in &snapshot.line_snapshots {
            for cluster in &line.clusters {
                let key = ClusterOwnerKey {
                    snapshot_id: line.id,
                    byte_range: (cluster.byte_start, cluster.byte_end),
                };
                if cluster_owners.get(&key) == Some(&ClusterVisualOwner::Animation) {
                    let rect = line.source_rect_to_document_rect(&cluster.source_rect);
                    static_exclusions.push(AnimationClipRect {
                        x: rect.x,
                        y: rect.y,
                        w: rect.w,
                        h: rect.h,
                        snapshot_id: line.id,
                    });
                }
            }
        }

        let mut keys: Vec<ClusterOwnerKey> = cluster_owners
            .iter()
            .filter(|(_, owner)| **owner == ClusterVisualOwner::Animation)
            .map(|(key, _)| key.clone())
            .collect();
        keys.sort_by_key(|key| {
            (
                key.snapshot_id.layout_revision,
                key.snapshot_id.paragraph_id,
                key.snapshot_id.visual_line_ordinal,
                key.byte_range.0,
                key.byte_range.1,
            )
        });
        keys.dedup();

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        keys.hash(&mut hasher);
        let ownership_revision = if keys.is_empty() {
            0
        } else {
            hasher.finish().max(1)
        };
        let handoff_pending = handoff_pending || !owner_table_matches;

        let canonical_frame = VisualFrame::from_static_snapshot(snapshot);
        let mut plan = Self {
            cluster_owners,
            static_exclusions,
            animated_glyphs,
            ownership_revision,
            document_session: 0,
            terminal_frame,
            terminal_motion_indices: Vec::new(),
            submitted_visible_widths: Vec::new(),
            handoff_pending,
            target_layout_revision: Some(snapshot.revision),
            candidate_frame: VisualFrame::default(),
            canonical_frame,
        };
        plan.candidate_frame = VisualFrame::from_rendered_plan(snapshot, &plan);
        plan
    }

    pub(crate) fn canonical(snapshot: &EditorLayoutSnapshot) -> Self {
        Self::from_owner_table(snapshot, Vec::new(), Vec::new(), false, false)
    }

    pub(crate) fn static_revision_after_handoff(&self) -> u64 {
        if self.handoff_pending {
            0
        } else {
            self.ownership_revision
        }
    }

    pub(crate) fn committed_revision(&self, resources_ready: bool) -> u64 {
        if resources_ready {
            self.static_revision_after_handoff()
        } else {
            0
        }
    }

    pub(crate) fn clips_for_static_rebuild(&self) -> &[AnimationClipRect] {
        if self.handoff_pending {
            &[]
        } else {
            &self.static_exclusions
        }
    }

    pub(crate) fn owns_animation_cluster(&self, key: &ClusterOwnerKey) -> bool {
        self.cluster_owners.get(key) == Some(&ClusterVisualOwner::Animation)
    }

    pub(crate) fn has_animation_resources(
        &self,
        cache: &super::texture_cache::TextureCache,
    ) -> bool {
        self.animated_glyphs
            .iter()
            .all(|glyph| cache.contains_line(&glyph.snapshot_id))
            && self
                .static_exclusions
                .iter()
                .all(|rect| cache.contains_line(&rect.snapshot_id))
    }

    pub(crate) fn owner_key(
        snapshot_id: LineSnapshotId,
        byte_range: (usize, usize),
    ) -> ClusterOwnerKey {
        ClusterOwnerKey {
            snapshot_id,
            byte_range,
        }
    }

    pub(crate) fn glyph(
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        opacity: f64,
        snapshot_id: LineSnapshotId,
        source_rect: crate::sujian_editor_item::layout_snapshot::SourceRect,
        logical_range: (usize, usize),
        canonical_range: Option<(usize, usize)>,
        delete_edge: Option<crate::sujian_editor_item::edit_motion::DeleteEdge>,
        shaping_identity: ShapingIdentity,
    ) -> TextAnimationGlyphInfo {
        TextAnimationGlyphInfo {
            x,
            y,
            w,
            h,
            opacity,
            snapshot_id,
            source_rect,
            logical_range,
            canonical_range,
            delete_edge,
            shaping_identity,
        }
    }
}
