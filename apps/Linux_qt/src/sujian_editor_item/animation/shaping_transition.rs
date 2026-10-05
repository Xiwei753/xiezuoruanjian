//! Issue #826 评论 24/25：不可拆 shaping cluster 的当前态视觉交接。
//!
//! ## 底层规则
//!
//! **Core 的 `inserted_ranges` / `deleted_ranges` / `OffsetMap` 可以按字符切；
//! Qt 的视觉 owner 绝不能切开一个 shaping cluster。**
//!
//! `engine.rs` 里 cluster 边界来自 `QGlyphRun::stringIndexes()`，一个 cluster 可能
//! 覆盖多个字符（fi 连字、e + 组合音标、emoji ZWJ）。Core 只知道逻辑字符身份，
//! 不知道这些字符被 shaping 合成了哪一块视觉资源。
//!
//! 之前 `layout_snapshot::clusters_in_byte_range()` 只是 overlap 查询，
//! `EditFrontier` 拿到结果后却当成「这块 cluster 就属于这个逻辑 range」，于是：
//!
//! ```text
//! 第一笔 af   f 的 cluster = 1..2
//! 第二笔 afi  最新 shaping 把 fi 合成一块 cluster = 1..3
//!             Core 逻辑上：旧 f 仍映射 1..2，新 i 是 2..3
//! ```
//!
//! - 评论 23 的 `mapped_previous` 层用 range `1..2` 建 path，`overlap` 把整块
//!   `fi` 拉进来 —— 「只含旧 owner」的 path 视觉上已经含了新 `i`，身份判据再次失效；
//! - `find_cluster_geometry(target, 1..2)` 同样返回整块 `fi`，于是
//!   `carried.range = 1..2` 却拿整块 `fi` 的纹理，`subtract_ranges` 又把
//!   `2..3`  留给 scalar Reveal —— **同一块视觉 cluster 被两个 owner 同时控制**。
//!
//! ## 本层的职责
//!
//! 从「逻辑 changed range + 前后两份 layout」派生「视觉 affected cluster」层：
//!
//! - 改动**完整覆盖**的 cluster：照旧交给 `EditFrontier` 的 Reveal / Conceal
//!   （绝大多数中文单字、独立 glyph 都属于这一类，观感完全不变）；
//! - 不可拆的 cluster（逻辑改动只覆盖它一部分、边界变了、或 `shaping_identity`
//!   变了）：整块退出 EditFrontier 与 Reflow，作为
//!   [`ShapingTransitionGroup`] 进入本层。
//!
//! ## 评论 25 补上的两条硬约束
//!
//! ### 1. 必须从当前屏幕接手，不能从 canonical 重建
//!
//! `af` → `afi` 时，上一帧屏幕上的 `f` 只露了 8.75px。若本层直接拿
//! `base_snapshot` 里的**完整** `f` 当 old 侧起步，第二笔同一帧就会
//! 「8.75px 突然变成 10px，再开始 f → fi 淡变」。
//!
//! 所以 coordinator 在任何状态变更前先采一份 [`CurrentVisualCluster`]（这一帧
//! 屏幕上真实存在的视觉原子：Reveal scalar 的已露宽度、carry 的可见前缀、
//! Reflow 的当前位置、Conceal 仍可见的旧 glyph、本层自己正在淡入淡出的两侧），
//! `build_or_retarget()` 的第一帧必须从这份事实起步。
//!
//! ### 2. 必须能跨下一笔编辑 retarget
//!
//! 生命周期与 `ReflowState` 完全对齐：
//!
//! - 下一笔不碰它：映射到最新 revision 后继续；
//! - 下一笔影响它：先 sample 当前帧，再 retarget 到最新 shaping；
//! - 只有 `is_finished()` 或显式 `finish_edit_frontier_to_canonical()` 才消失。
//!
//! 这里**不排历史队列、不 per-key 累积、没有第二个动画时钟** —— 只保留
//! 「当前这一组 source atoms + 最新 target atoms」。
//!
//! 架构归属：`EditFrontier` 管 changed logical fact + mask timing；`Reflow` 管
//! same-shaping unchanged move；本层只管当前不可拆 cluster 的 old/new 视觉交接。
//! 三者都只保存「当前屏幕事实」。

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::animation::edit_frontier::{ease_out_cubic, ConcealSourceLine};
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, LineSnapshotId, PreparedLineSnapshot, SourceRect,
};

const EPS: f64 = 1e-6;

/// Issue #826 评论 25：**这一帧**屏幕上真实存在的一个视觉原子。
///
/// 它不是历史动画单元 —— 没有 `started_at`、没有 remaining duration、没有
/// historical stage、没有第二个动画对象。它就是「owner 换手时，旧 owner 交出去
/// 的那一帧像素事实」，与 `ReflowCurrentGeometry` 同一性质。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CurrentVisualCluster {
    /// 字符身份，坐标在**当前 target 坐标系**里。
    ///
    /// 下一笔编辑的 `base_snapshot` 就是这份 target，所以这个 range 可以直接
    /// 拿去和下一笔派生出来的 atom 对齐。
    pub logical_range: (usize, usize),
    /// 贴图来源行纹理。
    pub snapshot_id: LineSnapshotId,
    /// 那张行纹理里的源矩形（覆盖整个 cluster）。
    pub source_rect: SourceRect,
    /// 这一帧它在屏幕上的矩形（文档坐标）。
    pub dest_rect: SourceRect,
    /// 这一帧的不透明度。
    pub opacity: f64,
    /// 这一帧真正可见的宽度（`0..= dest_rect.w`）。
    ///
    /// 吐字只露了 8.75px 就必须是 8.75 —— 新 owner 第一帧若按整字宽起步，
    /// 屏幕会「先补满再淡变」。
    pub visible_clip: f64,
}

/// Issue #826 评论 25：一个不可拆视觉 cluster 的一侧原子。
///
/// 1:1 的一对 `old_cluster` / `new_cluster` 表达不了真实 shaping：
/// 一块 old `0..4` 删掉中间一位后可能变成两块 new（`0..1` + `1..3`），
/// 反向 N:1 也一样。所以本层用「一组 old 原子 ↔ 一组 new 原子」表达
/// **当前这一笔不可拆区域**，组内共用同一个 progress。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VisualClusterAtom {
    /// 这个 cluster 的完整边界（它所属 snapshot 的坐标系）。
    pub cluster: (usize, usize),
    /// 交给**下一笔**做 handoff 对齐用的字符身份（坐标在当前 state 的 target 系）。
    pub handoff_key: (usize, usize),
    /// 贴图来源行纹理。
    pub snapshot_id: LineSnapshotId,
    /// 那张行纹理里的源矩形（覆盖整个 cluster，绝不按 byte 比例裁）。
    pub source_rect: SourceRect,
    /// 目标矩形（文档坐标）。
    pub rect: SourceRect,
    /// 当前帧起点 —— 有 handoff 就是上一帧的真实矩形，没有就是 canonical。
    pub start_rect: SourceRect,
    /// 当前帧不透明度。old 侧从它淡出到 0，new 侧从它淡入到 1。
    pub start_opacity: f64,
    /// 当前帧可见宽度。old 侧保持不变（本来就在淡出），new 侧增长到 `rect.w`。
    pub start_visible_width: f64,
}

/// Issue #826 评论 25：一次不可拆 shaping 区域的当前态 old/new 视觉集合。
///
/// 共用一个 progress：`old_atoms` 整组淡出、`new_atoms` 整组淡入，
/// 每个原子保留自己的 snapshot / source / dest rect。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShapingTransitionGroup {
    pub old_atoms: Vec<VisualClusterAtom>,
    pub new_atoms: Vec<VisualClusterAtom>,
}

/// 一个原子在某一帧、某一侧的画面。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShapingTransitionSide {
    pub snapshot_id: LineSnapshotId,
    pub source_rect: SourceRect,
    pub rect: SourceRect,
    /// old 侧 `-> 0`，new 侧 `-> 1`。
    pub opacity: f64,
}

/// 一个 group 在某一帧的画面。某一侧所有原子都归零时不再画这一侧。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShapingTransitionFrame {
    pub old: Vec<ShapingTransitionSide>,
    pub new: Vec<ShapingTransitionSide>,
}

/// Issue #826 评论 24/25：不可拆 cluster 的当前态交接层。
///
/// 与 `ReflowState` 同构：只有当前这一组 cluster、共用一个 `progress`，
/// 没有历史队列。
#[derive(Clone, Debug, Default)]
pub(crate) struct ShapingTransitionState {
    pub groups: Vec<ShapingTransitionGroup>,
    pub started_at: Option<Instant>,
    pub duration_ms: u64,
    /// 旧侧 cluster 真正引用的行图（画旧 cluster 必须有这张图）。
    pub old_sources: Vec<ConcealSourceLine>,
    /// 本 state 的 target 坐标系对应的正文纯文本。
    ///
    /// `previous.target_text() == request.base_text` 表示下一笔仍在同一条编辑
    /// revision 链上，这时才可以 retarget 而不是从 canonical 重建。
    pub target_text: String,
}

impl ShapingTransitionState {
    /// Issue #826 评论 25：从当前屏幕事实出发，建立或 retarget 这一笔的交接层。
    ///
    /// `previous` 非 `None` 表示上一份交接层与本笔在同一条 revision 链上
    /// （coordinator 用 `target_text()` 判定）。此时做两件事：
    ///
    /// 1. 本笔**新产生**的不可拆 component 从 `current_visuals` 起步；
    /// 2. 上一份里**没被接管**的 group 继续映射到最新 target 后接着淡。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_or_retarget(
        previous: Option<&ShapingTransitionState>,
        current_visuals: &[CurrentVisualCluster],
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        deleted_ranges: &[(usize, usize)],
        inserted_ranges: &[(usize, usize)],
        old_to_new: &OffsetMap,
        now: Instant,
        duration_ms: u64,
        target_text: String,
    ) -> Self {
        let old_side = ClusterIndex::build(old_snapshot);
        let new_side = ClusterIndex::build(new_snapshot);
        let (components, identity_pairs) = collect_components(&old_side, &new_side, old_to_new);

        let mut consumed: HashSet<usize> = HashSet::new();
        let mut groups: Vec<ShapingTransitionGroup> = Vec::new();
        // 本笔接管掉的旧坐标 cluster —— 上一份的 new 侧若落进这里就被吸收了。
        let mut absorbed: Vec<(usize, usize)> = Vec::new();

        for component in &components {
            if component.old_nodes.is_empty() || component.new_nodes.is_empty() {
                // 纯删 / 纯插：整块消失或整块出现，EditFrontier 自己就够。
                continue;
            }
            if !component.needs_transition(
                &old_side,
                &new_side,
                &identity_pairs,
                deleted_ranges,
                inserted_ranges,
                old_to_new,
            ) {
                continue;
            }
            let group = build_group(
                component,
                &old_side,
                &new_side,
                old_to_new,
                deleted_ranges,
                current_visuals,
                &mut consumed,
            );
            let Some(group) = group else { continue };
            for node in &component.old_nodes {
                let range = old_side.cluster_range(*node);
                if !absorbed.contains(&range) {
                    absorbed.push(range);
                }
            }
            groups.push(group);
        }

        // 上一份里没被本笔接管的 group：映射到最新 target 后继续淡。
        if let Some(previous) = previous {
            for group in &previous.groups {
                if group.new_atoms.iter().any(|atom| {
                    absorbed
                        .iter()
                        .any(|range| ranges_overlap(atom.cluster, *range))
                }) {
                    continue;
                }
                if let Some(retargeted) = retarget_group(
                    group,
                    new_snapshot,
                    old_to_new,
                    current_visuals,
                    &mut consumed,
                ) {
                    groups.push(retargeted);
                }
            }
        }

        let mut old_sources: Vec<ConcealSourceLine> = Vec::new();
        for group in &groups {
            for atom in &group.old_atoms {
                if old_sources
                    .iter()
                    .any(|source| source.snapshot_id == atom.snapshot_id)
                {
                    continue;
                }
                if let Some(line) = old_snapshot
                    .line_snapshots
                    .iter()
                    .find(|line| line.id == atom.snapshot_id)
                    .or_else(|| {
                        new_snapshot
                            .line_snapshots
                            .iter()
                            .find(|line| line.id == atom.snapshot_id)
                    })
                {
                    old_sources.push(ConcealSourceLine {
                        snapshot_id: line.id,
                        image: line.image.clone(),
                    });
                }
            }
        }
        old_sources.sort_by_key(|source| {
            (
                source.snapshot_id.layout_revision,
                source.snapshot_id.visual_line_ordinal,
            )
        });

        Self {
            groups,
            started_at: Some(now),
            duration_ms: duration_ms.max(1),
            old_sources,
            target_text,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub(crate) fn is_finished(&self, now: Instant) -> bool {
        let Some(started_at) = self.started_at else {
            return true;
        };
        if self.groups.is_empty() {
            return true;
        }
        let elapsed_ms = now.saturating_duration_since(started_at).as_millis() as f64;
        self.duration_ms == 0 || elapsed_ms >= self.duration_ms as f64
    }

    /// Issue #826 评论 25：交给下一笔做 handoff 的当前帧视觉事实。
    ///
    /// range 用各原子在**本 state 的 target 坐标系**里的 `handoff_key`，
    /// 下一笔的 `base_snapshot` 正好就是这份 target。
    pub(crate) fn current_visuals(&self, now: Instant) -> Vec<CurrentVisualCluster> {
        let mut out = Vec::new();
        for (frame, group) in self.sample(now).into_iter().zip(self.groups.iter()) {
            // `sample` 的两侧与各自的 atom 列表逐项对齐，位置一一对应。
            for (atom, side) in group.old_atoms.iter().zip(frame.old.iter()) {
                if side.opacity > EPS && side.rect.w > EPS {
                    out.push(visual_from(atom, side));
                }
            }
            for (atom, side) in group.new_atoms.iter().zip(frame.new.iter()) {
                if side.opacity > EPS && side.rect.w > EPS {
                    out.push(visual_from(atom, side));
                }
            }
        }
        out
    }

    /// 按当前 progress 采样本帧画面：old 侧淡出、new 侧淡入，位置一起补间。
    pub(crate) fn sample(&self, now: Instant) -> Vec<ShapingTransitionFrame> {
        let Some(started_at) = self.started_at else {
            return Vec::new();
        };
        let elapsed_ms = now.saturating_duration_since(started_at).as_millis() as f64;
        let progress = if self.duration_ms == 0 {
            1.0
        } else {
            (elapsed_ms / self.duration_ms as f64).clamp(0.0, 1.0)
        };
        let t = ease_out_cubic(progress);
        self.groups
            .iter()
            .map(|group| {
                // 1:1 时这就是 old_rect / new_rect；1:N、N:1 时是整组包围盒，
                // 视觉上就是「一块 cluster 淡出成另一组 cluster」。
                let new_region = union_rect(group.new_atoms.iter().map(|atom| &atom.rect));
                let old_region = union_rect(group.old_atoms.iter().map(|atom| &atom.rect));
                let old = sample_side(&group.old_atoms, t, new_region.as_ref(), false);
                let new = sample_side(&group.new_atoms, t, old_region.as_ref(), true);
                ShapingTransitionFrame { old, new }
            })
            .collect()
    }

    /// 本层真正引用到的行纹理 id（old 侧 + new 侧）。
    pub(crate) fn active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        for group in &self.groups {
            for atom in group.old_atoms.iter().chain(group.new_atoms.iter()) {
                if !ids.contains(&atom.snapshot_id) {
                    ids.push(atom.snapshot_id);
                }
            }
        }
        ids
    }

    /// 旧侧 cluster 的行图资源（新侧用最新 target 的行图，由 canonical 栅格化）。
    pub(crate) fn old_source_lines(&self) -> Vec<ConcealSourceLine> {
        self.old_sources.clone()
    }

    /// 新侧 cluster 在 canonical 静态层要挖掉的目标矩形。
    ///
    /// 动画层正在画「正在淡入的那一份」，静态层如果同时画最终位置就是重影。
    pub(crate) fn target_clip_rects(&self) -> Vec<(SourceRect, LineSnapshotId)> {
        self.groups
            .iter()
            .flat_map(|group| {
                group
                    .new_atoms
                    .iter()
                    .map(|atom| (atom.rect.clone(), atom.snapshot_id))
            })
            .collect()
    }

    /// 被本层整块占用的**旧坐标** cluster 范围。
    ///
    /// `EditFrontier` 的吞字侧必须把它们排除：普通 Conceal 绝不能声称
    /// `owner = 2..3` 却拿整块 old `fi` 的 glyph 来吞。
    pub(crate) fn owned_old_clusters(&self) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for group in &self.groups {
            for atom in &group.old_atoms {
                if !out.contains(&atom.cluster) {
                    out.push(atom.cluster);
                }
            }
        }
        out
    }

    /// 被本层整块占用的**新坐标** cluster 范围。
    ///
    /// `EditFrontier` 的吐字侧（scalar region、carry、settled）必须把它们排除：
    /// 否则同一块视觉 cluster 会同时被 carry 与 scalar Reveal 控制。
    pub(crate) fn owned_new_clusters(&self) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for group in &self.groups {
            for atom in &group.new_atoms {
                if !out.contains(&atom.cluster) {
                    out.push(atom.cluster);
                }
            }
        }
        out
    }

    pub(crate) fn target_text(&self) -> &str {
        &self.target_text
    }

    /// 测试用：当前被本层整块占用的 group 形状（old 原子数 / new 原子数）。
    #[cfg(test)]
    pub(crate) fn owned_clusters_for_test(&self) -> Vec<((usize, usize), (usize, usize))> {
        self.groups
            .iter()
            .map(|group| {
                (
                    group
                        .old_atoms
                        .first()
                        .map(|atom| atom.cluster)
                        .unwrap_or((0, 0)),
                    group
                        .new_atoms
                        .first()
                        .map(|atom| atom.cluster)
                        .unwrap_or((0, 0)),
                )
            })
            .collect()
    }
}

/// 采样一侧。`toward` 是这一侧要补间到的区域（1:1 时是对侧那一块，1:N / N:1
/// 时是整组包围盒），`is_new_side` 区分淡入 / 淡出。
///
/// **宽度不参与补间**：
///
/// - old 侧保持这一帧真实的可见宽度（它本来就在淡出，不需要再长）；
/// - new 侧始终是自己的整字宽 —— 它是一套**完全不同的字形资源**，按比例
///   裁会出现半个连字。
///
/// 只补间位置，让「正在消失的那一份」从它当前所在的位置起步。
fn sample_side(
    atoms: &[VisualClusterAtom],
    t: f64,
    toward: Option<&SourceRect>,
    is_new_side: bool,
) -> Vec<ShapingTransitionSide> {
    atoms
        .iter()
        .map(|atom| {
            // 1:1 时 toward 就是对侧那一块；1:N / N:1 时是整组包围盒。
            let to_rect = match toward {
                Some(region) => region.clone(),
                None => atom.rect.clone(),
            };
            let lerp = |from: f64, to: f64| from + (to - from) * t;
            let visible = if is_new_side {
                atom.rect.w
            } else {
                atom.start_visible_width.min(atom.rect.w)
            };
            let opacity_target = if is_new_side { 1.0 } else { 0.0 };
            let opacity =
                (atom.start_opacity + (opacity_target - atom.start_opacity) * t).clamp(0.0, 1.0);
            ShapingTransitionSide {
                snapshot_id: atom.snapshot_id,
                // 源矩形按可见比例裁。**这是裁「同一块 cluster 里已露出的那一段」，
                // 不是按 byte 比例裁** —— old / new 两侧的 source_rect 本来就是
                // 两套不同形状的资源，任何按字节的裁法都会画错字形。
                source_rect: SourceRect {
                    x: atom.source_rect.x,
                    y: atom.source_rect.y,
                    w: atom.source_rect.w * (visible / atom.rect.w.max(EPS)).clamp(0.0, 1.0),
                    h: atom.source_rect.h,
                },
                rect: SourceRect {
                    x: lerp(atom.start_rect.x, to_rect.x),
                    y: lerp(atom.start_rect.y, to_rect.y),
                    w: visible,
                    h: lerp(atom.start_rect.h, to_rect.h),
                },
                opacity,
            }
        })
        .collect()
}

fn visual_from(atom: &VisualClusterAtom, side: &ShapingTransitionSide) -> CurrentVisualCluster {
    CurrentVisualCluster {
        logical_range: atom.handoff_key,
        snapshot_id: side.snapshot_id,
        source_rect: side.source_rect.clone(),
        dest_rect: side.rect.clone(),
        opacity: side.opacity,
        visible_clip: side.rect.w,
    }
}

/// 为一个新 component 造一个 group。
fn build_group(
    component: &Component,
    old_side: &ClusterIndex<'_>,
    new_side: &ClusterIndex<'_>,
    old_to_new: &OffsetMap,
    deleted_ranges: &[(usize, usize)],
    current_visuals: &[CurrentVisualCluster],
    consumed: &mut HashSet<usize>,
) -> Option<ShapingTransitionGroup> {
    // 先确认 new 侧真有 cluster，再开始消费 handoff —— 消费不可逆。
    if component.new_nodes.is_empty() {
        return None;
    }

    // ── 1. old 侧先接手：屏幕上这一帧的像素属于它 ────────────────────
    //
    // 顺序是关键。`af -> afi` 时那 8.75px 的 `f` 就是 `fi` 这一组 old 侧
    // 唯一的像素来源；若让 new 原子先按「一方包含另一方」抢走，old 侧就只能
    // 退回 `base_snapshot` 里的**完整** `f`，第二笔同一帧就会「8.75px 突然
    // 补满 10px，再开始 f -> fi」—— 评论 25 阻塞 1 描述的那个跳变。
    let mut old_atoms = Vec::new();
    for node in &component.old_nodes {
        let (line, cluster) = old_side.cluster(*node);
        let rect = line.source_rect_to_document_rect(&cluster.source_rect);
        let cluster_range = (cluster.byte_start, cluster.byte_end);
        let handoff = take_handoff(
            current_visuals,
            consumed,
            cluster_range,
            Some(cluster_range),
        );
        // 这一块 old cluster 在最新正文里还剩下哪几段（target 坐标系）——
        // 下一笔要靠它对齐，因为那时 base 坐标系已经变成这份 target。
        let key = untouched_parts(cluster_range, deleted_ranges)
            .into_iter()
            .filter_map(|(start, end)| old_to_new.map_old_range_to_new(start, end))
            .next()
            .unwrap_or(cluster_range);
        old_atoms.push(atom_from_cluster(
            cluster_range,
            key,
            line,
            cluster,
            rect,
            handoff,
            false,
        ));
    }

    // 上一份交接层正在淡出的、已经不在这一笔 old cluster 里的视觉原子，
    // 仍然在屏幕上 —— 它们必须作为额外 old 原子继续淡出，不能凭空消失。
    // 典型场景 `af -> afi -> afij`：屏幕上是 `f`(opacity 0.2) + `fi`(opacity 0.8)，
    // 新的交接组 old 侧是 `fi`，那份 `f` 就是这里补进来的。
    let old_region: Vec<(usize, usize)> = old_atoms.iter().map(|atom| atom.cluster).collect();
    for index in 0..current_visuals.len() {
        if consumed.contains(&index) {
            continue;
        }
        let visual = &current_visuals[index];
        if !old_region
            .iter()
            .any(|region| containment_overlap(visual.logical_range, *region))
        {
            continue;
        }
        consumed.insert(index);
        old_atoms.push(VisualClusterAtom {
            cluster: visual.logical_range,
            handoff_key: visual.logical_range,
            snapshot_id: visual.snapshot_id,
            source_rect: visual.source_rect.clone(),
            rect: visual.dest_rect.clone(),
            start_rect: visual.dest_rect.clone(),
            start_opacity: visual.opacity,
            start_visible_width: visual.visible_clip,
        });
    }

    if old_atoms.is_empty() {
        return None;
    }

    // ── 2. new 侧最后接手，且**只接受精确身份**的 handoff ─────────────
    //
    // new 侧是「正在出现的那一份」，默认从 canonical 起步（不透明度 0、
    // 整字宽）并淡入 —— 它不能去抢 old 侧或上一份 leftover 的像素。只有当
    // 它与屏幕上某个视觉原子**字节完全一致**时，才说明这块字没变、只是换了
    // 一次 shaping，此时从当前像素接手才正确。
    let mut new_atoms = Vec::new();
    for node in &component.new_nodes {
        let (line, cluster) = new_side.cluster(*node);
        let rect = line.source_rect_to_document_rect(&cluster.source_rect);
        let cluster_range = (cluster.byte_start, cluster.byte_end);
        // new 侧的 handoff 键在 base 坐标系：把 target 坐标的 cluster 映回去。
        // 映不回说明它完全由本次插入构成，旧正文里没有对应身份，不能接手。
        let key = old_to_new.map_new_range_to_old(cluster_range.0, cluster_range.1);
        let handoff = key.and_then(|key| take_exact_handoff(current_visuals, consumed, key));
        new_atoms.push(atom_from_cluster(
            cluster_range,
            key.unwrap_or(cluster_range),
            line,
            cluster,
            rect,
            handoff,
            true,
        ));
    }
    Some(ShapingTransitionGroup {
        old_atoms,
        new_atoms,
    })
}

/// 把上一份 group 映射到最新 target 后继续淡。
fn retarget_group(
    group: &ShapingTransitionGroup,
    new_snapshot: &EditorLayoutSnapshot,
    prev_target_to_new: &OffsetMap,
    current_visuals: &[CurrentVisualCluster],
    consumed: &mut HashSet<usize>,
) -> Option<ShapingTransitionGroup> {
    let mut new_atoms = Vec::new();
    for atom in &group.new_atoms {
        let Some((start, end)) =
            prev_target_to_new.map_old_range_to_new(atom.cluster.0, atom.cluster.1)
        else {
            // 这段字在新正文里已经没有连续的对应（被吸收进别的 cluster），
            // 本笔若有新 component 会接手；这里直接丢掉这一侧。
            continue;
        };
        let Some((line, cluster)) = find_exact_cluster(new_snapshot, start, end) else {
            continue;
        };
        let rect = line.source_rect_to_document_rect(&cluster.source_rect);
        let cluster_range = (cluster.byte_start, cluster.byte_end);
        let handoff = take_exact_handoff(current_visuals, consumed, atom.handoff_key);
        new_atoms.push(atom_from_cluster(
            cluster_range,
            cluster_range,
            line,
            cluster,
            rect,
            handoff,
            true,
        ));
    }
    if new_atoms.is_empty() {
        return None;
    }

    // 旧侧只能保留**这一帧还看得见**的那些原子。
    //
    // `current_visuals` 是上一份交接层在 `now` 时刻的真实采样，并过滤掉了
    // 不透明度 / 可见宽度归零的原子。所以「没有 handoff」只有一个含义：
    // 它已经淡出，屏幕上不再有它的像素。此时若退回 `atom.start_opacity`
    // （new group 恒为 1.0），`started_at` 重置会让它闪回全不透明。
    let mut old_atoms = Vec::new();
    for atom in &group.old_atoms {
        let Some(handoff) = take_handoff(current_visuals, consumed, atom.handoff_key, None) else {
            continue;
        };
        if handoff.opacity <= EPS {
            continue;
        }
        // handoff 可能来自上一份交接层的 old 侧 —— 那一块字的贴图在更早的
        // 行纹理里，必须继续用它，否则旧侧会突然换一张图。
        old_atoms.push(VisualClusterAtom {
            cluster: atom.cluster,
            handoff_key: atom.handoff_key,
            snapshot_id: handoff.snapshot_id,
            source_rect: handoff.source_rect,
            rect: atom.rect.clone(),
            start_rect: handoff.dest_rect,
            start_opacity: handoff.opacity,
            start_visible_width: handoff.visible_clip,
        });
    }

    if old_atoms.is_empty() && new_atoms.iter().all(|atom| atom.start_opacity >= 1.0 - EPS) {
        return None;
    }
    Some(ShapingTransitionGroup {
        old_atoms,
        new_atoms,
    })
}

#[allow(clippy::too_many_arguments)]
fn atom_from_cluster(
    cluster: (usize, usize),
    handoff_key: (usize, usize),
    line: &PreparedLineSnapshot,
    cluster_snapshot: &LineClusterSnapshot,
    rect: SourceRect,
    handoff: Option<CurrentVisualCluster>,
    is_new_side: bool,
) -> VisualClusterAtom {
    let (start_rect, start_opacity, start_visible, texture) = match handoff {
        Some(handoff) => (
            handoff.dest_rect.clone(),
            handoff.opacity,
            handoff.visible_clip,
            // new 侧**必须**用它自己的新资源：handoff 的 `source_rect` 是上一份
            // 字形的裁剪片段，拿来画新 cluster 就是半个连字。old 侧相反 ——
            // handoff 可能来自更早的 revision，那时这张行图才是唯一来源。
            (!is_new_side).then(|| (handoff.snapshot_id, handoff.source_rect.clone())),
        ),
        None => (
            rect.clone(),
            if is_new_side { 0.0 } else { 1.0 },
            rect.w,
            None,
        ),
    };
    let (snapshot_id, source_rect) =
        texture.unwrap_or_else(|| (line.id, cluster_snapshot.source_rect.clone()));
    VisualClusterAtom {
        cluster,
        handoff_key,
        snapshot_id,
        source_rect,
        rect,
        start_rect,
        start_opacity,
        start_visible_width: start_visible,
    }
}

/// 找一条可用的 handoff 并标记消费。
///
/// 优先精确匹配（视觉身份完全一致）；否则接受「一方完整包含另一方」的匹配，
/// 这样 `af -> afi` 里那份还在淡出的 `f` 能被新 group 接管。
fn take_handoff(
    current_visuals: &[CurrentVisualCluster],
    consumed: &mut HashSet<usize>,
    key: (usize, usize),
    exact: Option<(usize, usize)>,
) -> Option<CurrentVisualCluster> {
    let mut fallback: Option<usize> = None;
    for (index, visual) in current_visuals.iter().enumerate() {
        if consumed.contains(&index) {
            continue;
        }
        if let Some(exact) = exact {
            if visual.logical_range == exact {
                consumed.insert(index);
                return Some(visual.clone());
            }
        }
        if visual.logical_range == key || containment_overlap(visual.logical_range, key) {
            fallback.get_or_insert(index);
        }
    }
    let index = fallback?;
    consumed.insert(index);
    Some(current_visuals[index].clone())
}

/// 取一条**字节身份完全一致**的 handoff 并标记消费。
///
/// 与 [`take_handoff`] 的区别是不接受「一方包含另一方」的宽松匹配。宽松匹配
/// 只能用于 old 侧（那块 cluster 确实就是屏幕上的那一块），new 侧一旦用宽松
/// 匹配就会抢走别人的像素：它必须从 canonical 起步再淡入。
fn take_exact_handoff(
    current_visuals: &[CurrentVisualCluster],
    consumed: &mut HashSet<usize>,
    key: (usize, usize),
) -> Option<CurrentVisualCluster> {
    let index = current_visuals
        .iter()
        .enumerate()
        .position(|(index, visual)| !consumed.contains(&index) && visual.logical_range == key)?;
    consumed.insert(index);
    Some(current_visuals[index].clone())
}

/// 一方完整包含另一方的重叠。
fn containment_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    let contains =
        |outer: (usize, usize), inner: (usize, usize)| outer.0 <= inner.0 && inner.1 <= outer.1;
    ranges_overlap(a, b) && (contains(a, b) || contains(b, a))
}

fn ranges_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

// ── cluster 连通分量 ────────────────────────────────────────────────────────

/// 一个 snapshot 里所有 cluster 的扁平索引，带按 byte 二分查找。
struct ClusterIndex<'a> {
    entries: Vec<Entry<'a>>,
    /// `(byte_start, node)` 排序后的查找表。
    sorted: Vec<(usize, usize)>,
}

struct Entry<'a> {
    line: &'a PreparedLineSnapshot,
    cluster: &'a LineClusterSnapshot,
}

impl<'a> ClusterIndex<'a> {
    fn build(snapshot: &'a EditorLayoutSnapshot) -> Self {
        let mut entries = Vec::new();
        for line in &snapshot.line_snapshots {
            for cluster in &line.clusters {
                entries.push(Entry { line, cluster });
            }
        }
        let mut sorted: Vec<(usize, usize)> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.cluster.byte_start, index))
            .collect();
        sorted.sort_unstable();
        Self { entries, sorted }
    }

    fn cluster(&self, node: usize) -> (&'a PreparedLineSnapshot, &'a LineClusterSnapshot) {
        let entry = &self.entries[node];
        (entry.line, entry.cluster)
    }

    fn cluster_range(&self, node: usize) -> (usize, usize) {
        let entry = &self.entries[node];
        (entry.cluster.byte_start, entry.cluster.byte_end)
    }

    /// 包含 `byte` 的 cluster 节点。cluster 互不重叠且按 byte_start 有序。
    fn node_at_byte(&self, byte: usize) -> Option<usize> {
        let position = self.sorted.partition_point(|&(start, _)| start <= byte);
        if position == 0 {
            return None;
        }
        let node = self.sorted[position - 1].1;
        let (start, end) = self.cluster_range(node);
        if start <= byte && byte < end {
            Some(node)
        } else {
            None
        }
    }
}

/// old / new cluster 之间的一个连通分量。
struct Component {
    old_nodes: Vec<usize>,
    new_nodes: Vec<usize>,
}

impl Component {
    /// 这个分量是否必须走 shaping 交接层。
    fn needs_transition(
        &self,
        old_side: &ClusterIndex<'_>,
        new_side: &ClusterIndex<'_>,
        identity_pairs: &[(usize, usize)],
        deleted_ranges: &[(usize, usize)],
        inserted_ranges: &[(usize, usize)],
        old_to_new: &OffsetMap,
    ) -> bool {
        // 1. 逻辑改动只覆盖 cluster 的一部分 —— 评论 24 的原始判据。
        let mixed = self.old_nodes.iter().any(|node| {
            let range = old_side.cluster_range(*node);
            deleted_ranges
                .iter()
                .any(|changed| is_mixed(range, *changed))
        }) || self.new_nodes.iter().any(|node| {
            let range = new_side.cluster_range(*node);
            inserted_ranges
                .iter()
                .any(|changed| is_mixed(range, *changed))
        });
        if mixed {
            return true;
        }
        // 2. 边界变了（1:N / N:1 / N:M）。
        //
        // 注意不能只比数量：`af -> afi -> afij` 里 old `fi 1..3` 与 new `fij 1..4`
        // 数量相同，但 cluster 边界确实变了，必须交接。判据是「把 old 侧 cluster
        // 映到新坐标之后，与 new 侧 cluster 集合是否逐个相等」。
        let mut mapped: Vec<(usize, usize)> = Vec::new();
        for node in &self.old_nodes {
            let (start, end) = old_side.cluster_range(*node);
            match old_to_new.map_old_range_to_new(start, end) {
                Some(range) if !mapped.contains(&range) => mapped.push(range),
                None => return true,
                _ => {}
            }
        }
        let new_ranges: Vec<(usize, usize)> = self
            .new_nodes
            .iter()
            .map(|node| new_side.cluster_range(*node))
            .collect();
        if mapped.len() != new_ranges.len()
            || new_ranges.iter().any(|range| !mapped.contains(range))
        {
            return true;
        }
        // 3. `shaping_identity` 变了。
        //
        // 复杂脚本里在别处插一个字符会改变**未被改动范围 overlap** 的字符形态：
        // 逻辑 range 完整映射、cluster 边界也没变，但 glyph shape 变了。
        // `Reflow` 看到 `is_same_shaping` 为假就 `continue`，本层若也不接，
        // 这块字会直接跳成最新 canonical。
        identity_pairs.iter().any(|(old_node, new_node)| {
            if !self.old_nodes.contains(old_node) || !self.new_nodes.contains(new_node) {
                return false;
            }
            let (_, old_cluster) = old_side.cluster(*old_node);
            let (_, new_cluster) = new_side.cluster(*new_node);
            !old_cluster
                .shaping_identity
                .is_same_shaping(&new_cluster.shaping_identity)
        })
    }
}

/// 用 `OffsetMap` 里那些**没被改动**的字节身份，把 old / new cluster 连成连通分量。
///
/// 每个 entry 是一段没被改动的静态文本，逐字节把「含这个字节的 old cluster」与
/// 「含映射后字节的 new cluster」union 起来 —— 同一段文字因此必然落在同一分量里。
/// 顺带记下这些「共享身份」的 old/new 配对，`needs_transition` 用它比对
/// `shaping_identity`。
fn collect_components(
    old_side: &ClusterIndex<'_>,
    new_side: &ClusterIndex<'_>,
    old_to_new: &OffsetMap,
) -> (Vec<Component>, Vec<(usize, usize)>) {
    let total = old_side.entries.len() + new_side.entries.len();
    let mut parent: Vec<usize> = (0..total).collect();
    let mut identity_pairs: HashSet<(usize, usize)> = HashSet::new();

    for entry in &old_to_new.entries {
        for offset in 0..entry.length {
            let Some(old_node) = old_side.node_at_byte(entry.old_byte_offset.value() + offset)
            else {
                continue;
            };
            let new_byte = entry.new_byte_offset.value() + offset;
            let Some(new_node) = new_side.node_at_byte(new_byte) else {
                continue;
            };
            // union-find 用「old 侧下标 + old 侧长度」作为 new 侧下标；
            // `identity_pairs` 与 `Component` 一律用**各自侧的局部下标**，
            // 否则 `needs_transition` 里的 `contains` 永远不成立。
            union(&mut parent, old_node, new_node + old_side.entries.len());
            identity_pairs.insert((old_node, new_node));
        }
    }

    let mut buckets: HashMap<usize, Component> = HashMap::new();
    for node in 0..total {
        let root = find(&mut parent, node);
        let bucket = buckets.entry(root).or_insert_with(|| Component {
            old_nodes: Vec::new(),
            new_nodes: Vec::new(),
        });
        if node < old_side.entries.len() {
            bucket.old_nodes.push(node);
        } else {
            bucket.new_nodes.push(node - old_side.entries.len());
        }
    }
    let mut components: Vec<Component> = buckets
        .into_values()
        .filter(|component| !component.old_nodes.is_empty() && !component.new_nodes.is_empty())
        .collect();
    components.sort_by_key(|component| {
        component
            .old_nodes
            .first()
            .map(|node| old_side.cluster_range(*node))
            .unwrap_or((0, 0))
    });
    (components, identity_pairs.into_iter().collect())
}

fn find(parent: &mut [usize], node: usize) -> usize {
    let mut root = node;
    while parent[root] != root {
        root = parent[root];
    }
    let mut current = node;
    while parent[current] != root {
        let next = parent[current];
        parent[current] = root;
        current = next;
    }
    root
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[rb] = ra;
    }
}

// ── 自由函数 ────────────────────────────────────────────────────────────────

/// 本笔逻辑改动是否只覆盖了 `cluster` 的一部分。
///
/// 「完整覆盖」或「完全不碰」都不算 mixed —— 前者照旧走 Reveal / Conceal，
/// 后者照旧走 Reflow。只有落在中间这一档才是不可拆的 mixed visual cluster。
fn is_mixed(cluster: (usize, usize), changed: (usize, usize)) -> bool {
    let overlaps = cluster.0 < changed.1 && changed.0 < cluster.1;
    let fully_covered = changed.0 <= cluster.0 && cluster.1 <= changed.1;
    overlaps && !fully_covered
}

/// `cluster` 去掉 `changed` 之后**仍然存在**的那些片段。
///
/// 评论 25 阻塞 4：这里必须返回**全部**片段。旧 cluster `0..4` 删掉中间 `1..2`
/// 之后还剩 `0..1` 与 `2..4` 两段，它们可能分别 shaping 成两块 new cluster；
/// 只取第一段会让后一块被 canonical 直接放出来。
fn untouched_parts(cluster: (usize, usize), changed: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut remaining = vec![cluster];
    for &(start, end) in changed {
        let mut next: Vec<(usize, usize)> = Vec::new();
        for (seg_start, seg_end) in remaining {
            if end <= seg_start || seg_end <= start {
                next.push((seg_start, seg_end));
                continue;
            }
            if seg_start < start {
                next.push((seg_start, start.min(seg_end)));
            }
            if end < seg_end {
                next.push((end.max(seg_start), seg_end));
            }
        }
        remaining = next;
    }
    remaining.retain(|(start, end)| end > start);
    remaining
}

fn find_exact_cluster<'a>(
    snapshot: &'a EditorLayoutSnapshot,
    byte_start: usize,
    byte_end: usize,
) -> Option<(&'a PreparedLineSnapshot, &'a LineClusterSnapshot)> {
    for line in &snapshot.line_snapshots {
        if let Some(cluster) = line.cluster_exact_for_range((byte_start, byte_end)) {
            return Some((line, cluster));
        }
    }
    None
}

fn union_rect<'a>(rects: impl Iterator<Item = &'a SourceRect>) -> Option<SourceRect> {
    let mut acc: Option<SourceRect> = None;
    for rect in rects {
        acc = Some(match acc {
            None => rect.clone(),
            Some(current) => {
                let x0 = current.x.min(rect.x);
                let y0 = current.y.min(rect.y);
                let x1 = (current.x + current.w).max(rect.x + rect.w);
                let y1 = (current.y + current.h).max(rect.y + rect.h);
                SourceRect {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                }
            }
        });
    }
    acc
}

#[cfg(test)]
mod tests;
