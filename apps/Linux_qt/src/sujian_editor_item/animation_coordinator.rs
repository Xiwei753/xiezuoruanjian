//! Linux Qt 文字动画协调器。
//!
//! 主链：
//! ```text
//! Core EditorVisualTransaction
//! → 捕获 old/new layout snapshot
//! → 生成 AnimatedSlice + StaticLinePatch
//! → 准备平台视觉资源
//! → PreparedTransactionQueue
//! → rendering overlay + cursor transition
//! ```
//!
//! 关键约束：
//! - 先完成视觉资源准备，再允许静态层隐藏：`texture_prepared` 为 true 前静态层不裁剪，
//!   否则准备纹理与第一帧 overlay 之间会出现空白帧。
//! - 连续输入从当前视觉帧 rebase：新事务与旧事务 byte range 重叠时，先从旧事务当前
//!   progress 计算已显示帧位置，rebase 新 slice 的 from_document_rect，再取消旧事务，
//!   保证视觉无跳变。
//! - scrolling/window inactive 使用 pause/resume 而非销毁事务：滚动结束后 revision
//!   未变则累加 paused duration 继续，避免重新创建事务的开销和视觉跳变。
//! - revision 不匹配时必须取消：旧 source rect 是旧布局的产物，不能套到新布局上，
//!   否则坐标和 shaping 全部错误。
//! - shaping identity 变化走 crossfade 而非强行 move：字体、glyph、方向、格式任一变化
//!   都意味着旧视觉资源与新排版结果不是同一视觉对象，强行移动会导致 ligature/RTL/emoji
//!   渲染错误。

use std::time::Instant;

use writer_core::editor::{CursorRect, EditorAnimationKind, EditorVisualTransaction, OffsetMap};

use super::animated_slice::{AnimatedSlice, AnimatedSliceKind};
pub(crate) use super::animation_mode::AnimationMode;
pub(crate) use super::cursor_animation::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId, ShapingIdentity, SourceRect};
pub(crate) use super::render_plan::{
    CursorRenderState, PreeditRange, RenderPlan, SelectionPreeditPlan, SelectionRange,
    TextAnimationGlyphInfo, TextAnimationPlan,
};
use super::static_line_patch::StaticLinePatch;
use super::text_visual_transaction::{
    PreparedTextVisualTransaction, PreparedTransactionQueue, TextVisualOperationKind,
    TextVisualTransactionState, TransactionTimeline,
};
pub(crate) use super::transaction_key::VisualTransactionKey;

use crate::sujian_editor_item::editor_animation_debug_log;

/// Issue #690 评论 5675007226 步骤 1: 同一帧的统一时间采样。
///
/// `update_paint_node()` 入口处取一次 `Instant::now()` 作为 `frame_now`，
/// 后续文字 progress、光标 progress、cursor timeline sample 全部从这一个时间点计算。
/// 消除 GUI 线程 FrameAnimation tick 和 Scene Graph 渲染帧之间的采样偏差。
#[derive(Clone, Copy, Debug)]
pub(crate) struct AnimationFrameSample {
    /// 本帧统一采样时间点。
    pub frame_now: Instant,
}

impl AnimationFrameSample {
    pub fn new(frame_now: Instant) -> Self {
        Self { frame_now }
    }
}

/// Issue #679 评论 5657313927 (3c): 按 driver key 取样 Timeline 进度的结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CursorTimelineSample {
    /// 事务仍处于 Pending / Prepared，Timeline 还没开始走，应保持当前视觉位置。
    Waiting,
    /// 事务处于 Rendering / Paused，返回当前 progress（已 clamp 到 [0,1]）。
    Running(f64),
}

/// Issue #690 评论 5675007226 步骤 3: 将旧事务的视觉帧 rebase 到新事务的 slice 上。
///
/// 与旧版的区别：rebase 时传入 `visible_fraction`，使 InsertReveal/DeleteConceal
/// 从当前可见比例继续，而不是重新 0→1 / 1→0。
/// `visible_fraction` 从旧 slice 的 `compute_frame(old_progress)` 计算：
/// - InsertReveal：visible = frame.w / slice.to_document_rect.w（已吐出比例）
/// - DeleteConceal：visible = frame.w / slice.from_document_rect.w（剩余比例）
///
/// 三层匹配策略不变（tier1 精确匹配 → tier2 offset map → tier3 shaping identity）。
fn match_rebase_frames(
    rebase_frames: &[(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)],
    slices: &mut [AnimatedSlice],
    offset_map: &OffsetMap,
) {
    let mut consumed_indices: Vec<usize> = Vec::new();
    for (bs, be, fx, fy, fo, ref shaping, visible_fraction) in rebase_frames {
        let tier1 = slices
            .iter_mut()
            .enumerate()
            .filter(|(idx, _)| !consumed_indices.contains(idx))
            .find(|(_, ns)| ns.byte_start == *bs && ns.byte_end == *be);
        if let Some((idx, new_slice)) = tier1 {
            new_slice.rebase_from(*fx, *fy, *fo, *visible_fraction);
            consumed_indices.push(idx);
            continue;
        }
        if let (Some(mbs), Some(mbe)) = (
            offset_map.map_old_to_new(*bs),
            offset_map.map_old_to_new(*be),
        ) {
            let tier2 = slices
                .iter_mut()
                .enumerate()
                .filter(|(idx, _)| !consumed_indices.contains(idx))
                .find(|(_, ns)| ns.byte_start == mbs && ns.byte_end == mbe);
            if let Some((idx, new_slice)) = tier2 {
                new_slice.rebase_from(*fx, *fy, *fo, *visible_fraction);
                consumed_indices.push(idx);
                continue;
            }
            if let Some(ref sid) = shaping {
                let mapped_center = (mbs + mbe) as i64 / 2;
                let best = slices
                    .iter_mut()
                    .enumerate()
                    .filter(|(idx, _)| !consumed_indices.contains(idx))
                    .filter(|(_, ns)| ns.shaping_identity.as_ref() == Some(sid))
                    .filter(|(_, ns)| ns.byte_start >= mbs && ns.byte_end <= mbe.max(mbs + 1))
                    .min_by_key(|(idx, ns)| {
                        let candidate_center = (ns.byte_start + ns.byte_end) as i64 / 2;
                        let abs_dist = (candidate_center - mapped_center).abs();
                        (abs_dist, ns.byte_start, *idx)
                    });
                if let Some((idx, new_slice)) = best {
                    new_slice.rebase_from(*fx, *fy, *fo, *visible_fraction);
                    consumed_indices.push(idx);
                }
            }
        }
    }
}

/// Issue #658 评论 5630181473 问题 3: cluster 级 reflow 的引用条目。
///
/// 指向一个 old 或 new cluster 的位置信息，用于构建二分图边和 connected components。
#[derive(Clone, Debug)]
struct ReflowClusterRef {
    line_idx: usize,
    cluster_idx: usize,
    byte_start: usize,
    byte_end: usize,
}

/// Issue #658 评论 5630181473 问题 3: 一个 reflow run — 由 byte range 重叠连边
/// 形成的 connected component。
///
/// 包含至少一个 old cluster 和一个 new cluster。run 内的所有成员共享同一动画：
/// - 1 old + 1 new + range 完全对应 + shaping 相同 → geometry 变了才 reflow_move
/// - 1 old + 1 new + shaping 不同 → 一对一 crossfade
/// - 其他情况（1→N / N→1 / N→M）→ old 每个淡出一次，new 每个淡入一次
#[derive(Clone, Debug)]
struct ReflowRun {
    old: Vec<ReflowClusterRef>,
    new: Vec<ReflowClusterRef>,
}

// ── Issue #687: 显式 changed range 拥有函数 ──
//
// 插入和删除的 changed range 必须由 Core 给出的 inserted_range / deleted_range
// 显式拥有，不再让 reflow cluster 二分图推断。这两个函数按明确范围直接生成
// InsertReveal / DeleteConceal 切片和对应的 StaticLinePatch。

/// 按 Core 给出的 inserted_range 从 new_snapshot 显式生成 InsertReveal 切片。
///
/// 只接 `vt.inserted_range + new_snapshot`。按这个明确范围找 new cluster/sourceRect，
/// 直接生成 `InsertReveal`，同时生成对应 new line 的 `StaticLinePatch`，
/// 静态层在动画期间只隐藏这些新字的 sourceRect。
///
/// # 参数
/// - `key`：事务键。
/// - `new_snapshot`：新布局快照。
/// - `inserted_range`：Core 给出的插入范围 (byte_start, byte_end)。
///
/// # 返回
/// `(slices, static_patches)`：InsertReveal 动画切片和 insert 级静态行补丁。
fn build_insert_reveal_slices(
    key: VisualTransactionKey,
    new_snapshot: &EditorLayoutSnapshot,
    inserted_range: (usize, usize),
) -> (Vec<AnimatedSlice>, Vec<StaticLinePatch>) {
    let mut slices = Vec::new();
    let mut managed_new_clusters: Vec<(usize, usize, SourceRect)> = Vec::new();
    let (range_start, range_end) = inserted_range;

    for (line_idx, new_line) in new_snapshot.line_snapshots.iter().enumerate() {
        for (cluster_idx, new_cluster) in new_line.clusters.iter().enumerate() {
            // 只处理落在 inserted_range 内的 cluster
            if new_cluster.byte_start >= range_start && new_cluster.byte_end <= range_end {
                let new_sr = new_cluster.source_rect.clone();
                let new_doc = new_line.source_rect_to_document_rect(&new_sr);
                slices.push(AnimatedSlice::insert_reveal(
                    key,
                    new_line.id,
                    new_sr.clone(),
                    new_doc,
                    0.0,
                    0.0,
                    new_cluster.byte_start,
                    new_cluster.byte_end,
                    Some(new_cluster.shaping_identity.clone()),
                ));
                managed_new_clusters.push((line_idx, cluster_idx, new_sr));
            }
        }
    }

    // 生成 StaticLinePatches：Insert 的新字 sourceRect 必须在静态层隐藏到 Reveal 完成。
    let mut patches_by_line: std::collections::HashMap<usize, Vec<SourceRect>> =
        std::collections::HashMap::new();
    for (line_idx, _cluster_idx, sr) in &managed_new_clusters {
        patches_by_line
            .entry(*line_idx)
            .or_default()
            .push(sr.clone());
    }
    let mut static_patches = Vec::new();
    for (line_idx, hidden_rects) in patches_by_line {
        let new_line = &new_snapshot.line_snapshots[line_idx];
        static_patches.push(StaticLinePatch::insert_patch(
            new_line.id,
            hidden_rects,
            Vec::new(),
            new_line.byte_start,
            new_line.byte_end,
        ));
    }

    let slices = merge_adjacent_slices(slices);
    (slices, static_patches)
}

/// 按 Core 给出的 deleted_range 从 old_snapshot 显式生成 DeleteConceal 切片。
///
/// 只接 `vt.deleted_range + old_snapshot + new_cursor_rect`。按明确删除范围从
/// old snapshot 取纹理，直接生成 `DeleteConceal`。删除后的 canonical new text
/// 可以立即作为背景，旧字只由 overlay 吞掉，因此不生成 StaticLinePatch。
///
/// # 参数
/// - `key`：事务键。
/// - `old_snapshot`：旧布局快照。
/// - `deleted_range`：Core 给出的删除范围 (byte_start, byte_end)。
/// - `new_cursor_rect`：新光标矩形，用于决定吞字方向（conceal_from_left）。
///
/// # 返回
/// `slices`：DeleteConceal 动画切片。
fn build_delete_conceal_slices(
    key: VisualTransactionKey,
    old_snapshot: &EditorLayoutSnapshot,
    deleted_range: (usize, usize),
    new_cursor_rect: Option<&CursorRect>,
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    let (range_start, range_end) = deleted_range;
    let new_cx = new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
    let new_cy = new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

    for old_line in &old_snapshot.line_snapshots {
        for old_cluster in &old_line.clusters {
            // 只处理落在 deleted_range 内的 cluster
            if old_cluster.byte_start >= range_start && old_cluster.byte_end <= range_end {
                let old_sr = old_cluster.source_rect.clone();
                let old_doc = old_line.source_rect_to_document_rect(&old_sr);
                // 按新光标落在被删文字哪一侧决定保留左段还是右段。
                // 光标靠近右端 → 保留左段（conceal_from_left=true，Backspace 场景）；
                // 光标靠近左端 → 保留右段（conceal_from_left=false，Delete 键场景）。
                let left = old_doc.x;
                let right = old_doc.x + old_doc.w;
                let conceal_from_left = (new_cx - right).abs() <= (new_cx - left).abs();
                slices.push(AnimatedSlice::delete_conceal(
                    key,
                    old_line.id,
                    old_sr,
                    old_doc,
                    new_cx,
                    new_cy,
                    old_cluster.byte_start,
                    old_cluster.byte_end,
                    Some(old_cluster.shaping_identity.clone()),
                    conceal_from_left,
                ));
            }
        }
    }

    // Delete 不生成 StaticLinePatch：删除后的 canonical new text 可以立即作为背景，
    // 旧字只由 overlay 吞掉。
    let slices = merge_adjacent_slices(slices);
    slices
}

/// 构建 cluster/run 级 reflow 切片和静态行补丁。
///
/// 两阶段算法：
/// 1. 建关系：遍历所有未 excluded 的 old/new cluster，用 OffsetMap range mapping
///    变到同一逻辑 byte 坐标，只要逻辑范围有重叠就连边，对二分图求 connected components。
/// 2. 按 run 分类：每个 component 按 old/new 成员数量和 shaping 一致性决定动画类型。
///    真正没有任何映射边的 new cluster 才是 insert_reveal；
///    真正没有任何映射边的 old cluster 才是 delete_conceal。
///
/// # 参数
/// - `excluded_old_ranges`：已被 insert/delete 动画接管的 old byte range，跳过不处理。
/// - `excluded_new_ranges`：已被 insert/delete 动画接管的 new byte range，跳过不处理。
/// - `old_cursor_rect`：用于 reflow 中检测到的 insert_reveal 的起始位置。
/// - `new_cursor_rect`：用于 reflow 中检测到的 delete_conceal 的收缩目标。
///
/// # 返回
/// `(slices, static_patches)`：动画切片和 cluster 级静态行补丁。
fn build_cluster_reflow_slices(
    key: VisualTransactionKey,
    old_snapshot: &EditorLayoutSnapshot,
    new_snapshot: &EditorLayoutSnapshot,
    offset_map: &OffsetMap,
    excluded_old_ranges: &[(usize, usize)],
    excluded_new_ranges: &[(usize, usize)],
    old_cursor_rect: Option<&CursorRect>,
    new_cursor_rect: Option<&CursorRect>,
) -> (Vec<AnimatedSlice>, Vec<StaticLinePatch>) {
    eprintln!(
        "[BUGFIX_REPRO_TRACE] build_cluster_reflow_slices: excluded_old_ranges={:?}, excluded_new_ranges={:?}",
        excluded_old_ranges, excluded_new_ranges
    );
    let mut slices = Vec::new();
    let mut static_patches = Vec::new();

    // Issue #687: old_cx/old_cy/new_cx/new_cy 不再需要——changed range 由
    // build_insert_reveal_slices / build_delete_conceal_slices 显式拥有，
    // reflow 只处理 unchanged material。
    let _ = (old_cursor_rect, new_cursor_rect);

    // ── 阶段 1：收集所有未 excluded 的 old/new cluster refs ──
    // Issue #658 评论 5630181473 问题 3: 被 excluded 的 old cluster 仍保留在
    // old_refs 中（标记 excluded=true），不参与边构建，但最终在阶段 4 中作为
    // "纯 old" run 生成 delete_conceal。被 excluded 的 new cluster 直接跳过。
    let mut old_refs: Vec<ReflowClusterRef> = Vec::new();
    let mut old_excluded_flags: Vec<bool> = Vec::new();
    for (line_idx, old_line) in old_snapshot.line_snapshots.iter().enumerate() {
        for (cluster_idx, old_cluster) in old_line.clusters.iter().enumerate() {
            let is_excluded = excluded_old_ranges
                .iter()
                .any(|(s, e)| old_cluster.byte_start >= *s && old_cluster.byte_end <= *e);
            old_refs.push(ReflowClusterRef {
                line_idx,
                cluster_idx,
                byte_start: old_cluster.byte_start,
                byte_end: old_cluster.byte_end,
            });
            old_excluded_flags.push(is_excluded);
        }
    }

    let mut new_refs: Vec<ReflowClusterRef> = Vec::new();
    for (line_idx, new_line) in new_snapshot.line_snapshots.iter().enumerate() {
        for (cluster_idx, new_cluster) in new_line.clusters.iter().enumerate() {
            if excluded_new_ranges
                .iter()
                .any(|(s, e)| new_cluster.byte_start >= *s && new_cluster.byte_end <= *e)
            {
                continue;
            }
            new_refs.push(ReflowClusterRef {
                line_idx,
                cluster_idx,
                byte_start: new_cluster.byte_start,
                byte_end: new_cluster.byte_end,
            });
        }
    }

    // ── 阶段 2：构建二分图边（byte range 重叠）──
    // 被 excluded 的 old cluster 不参与连边，最终作为 "纯 old" run 生成 delete_conceal。
    // Union-Find: 0..old_refs.len() 为 old 节点, old_refs.len().. 为 new 节点
    let n_old = old_refs.len();
    let n_new = new_refs.len();
    let n = n_old + n_new;
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    // 连边条件：old cluster 的 byte range 与 new cluster 的 byte range 有逻辑重叠
    // 被 excluded 的 old cluster 跳过（不连边）
    for (oi, oref) in old_refs.iter().enumerate() {
        if old_excluded_flags[oi] {
            continue;
        }
        for (ni, nref) in new_refs.iter().enumerate() {
            // 将 new cluster 的 byte range 映射到 old 坐标系
            let mapped_old_range = offset_map.map_new_range_to_old(nref.byte_start, nref.byte_end);

            let overlaps = if let Some((mos, moe)) = mapped_old_range {
                // 严格半开区间重叠：[a_start, a_end) ∩ [b_start, b_end) ≠ ∅
                // 即 a_start < b_end && b_start < a_end。
                // 共享端点不算重叠（[0,1) 和 [1,2) 只是相邻，不连边）。
                oref.byte_start < moe && mos < oref.byte_end
            } else {
                // new cluster 跨越映射边界 — 逐端点回退检查
                let start_mapped = offset_map.map_new_to_old(nref.byte_start);
                let last_byte = if nref.byte_end > 0 {
                    nref.byte_end - 1
                } else {
                    0
                };
                let end_mapped = offset_map.map_new_to_old(last_byte);
                if let (Some(ms), Some(me)) = (start_mapped, end_mapped) {
                    (ms >= oref.byte_start && ms < oref.byte_end)
                        || (me >= oref.byte_start && me < oref.byte_end)
                        || (ms <= oref.byte_start && me >= oref.byte_end)
                } else {
                    false
                }
            };

            if overlaps {
                union(&mut parent, oi, n_old + ni);
            }
        }
    }

    // ── 阶段 3：提取 connected components → ReflowRuns ──
    let mut component_map: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    let mut runs: Vec<ReflowRun> = Vec::new();

    for oi in 0..n_old {
        let root = find(&mut parent, oi);
        let comp_idx = *component_map.entry(root).or_insert_with(|| {
            let idx = runs.len();
            runs.push(ReflowRun {
                old: Vec::new(),
                new: Vec::new(),
            });
            idx
        });
        runs[comp_idx].old.push(old_refs[oi].clone());
    }
    for ni in 0..n_new {
        let root = find(&mut parent, n_old + ni);
        let comp_idx = *component_map.entry(root).or_insert_with(|| {
            let idx = runs.len();
            runs.push(ReflowRun {
                old: Vec::new(),
                new: Vec::new(),
            });
            idx
        });
        runs[comp_idx].new.push(new_refs[ni].clone());
    }

    // ── 阶段 4：按 run 分类生成动画切片 ──
    // 跟踪已被 run 接管的 new cluster，用于生成 StaticLinePatch
    let mut run_managed_new_clusters: Vec<(usize, usize, SourceRect)> = Vec::new();

    for run in &runs {
        if run.old.is_empty() && run.new.is_empty() {
            continue;
        }

        let run_old = &run.old;
        let run_new = &run.new;

        // Issue #687: 纯 new（无 old 对应）和纯 old（无 new 对应）的 run 不再由
        // reflow 推断 InsertReveal / DeleteConceal。changed range 所有权由
        // build_insert_reveal_slices / build_delete_conceal_slices 显式拥有。
        // reflow 只处理 unchanged material（ReflowMove / ReflowCrossFade）。
        // 纯 new / 纯 old run 理论上不应出现（changed range 已被 excluded），
        // 若因边界情况出现则直接跳过，不生成切片。
        if run_old.is_empty() || run_new.is_empty() {
            continue;
        }

        // 1 old + 1 new：精确匹配
        if run_old.len() == 1 && run_new.len() == 1 {
            let oref = &run_old[0];
            let nref = &run_new[0];
            let old_line = &old_snapshot.line_snapshots[oref.line_idx];
            let old_cluster = &old_line.clusters[oref.cluster_idx];
            let new_line = &new_snapshot.line_snapshots[nref.line_idx];
            let new_cluster = &new_line.clusters[nref.cluster_idx];

            let same_shaping = old_cluster
                .shaping_identity
                .is_same_shaping(&new_cluster.shaping_identity);

            let old_sr = old_cluster.source_rect.clone();
            let new_sr = new_cluster.source_rect.clone();
            let old_doc = old_line.source_rect_to_document_rect(&old_sr);
            let new_doc = new_line.source_rect_to_document_rect(&new_sr);

            if same_shaping {
                let geometry_same = (old_doc.x - new_doc.x).abs() < 0.5
                    && (old_doc.y - new_doc.y).abs() < 0.5
                    && (old_doc.w - new_doc.w).abs() < 0.5
                    && (old_doc.h - new_doc.h).abs() < 0.5;
                if !geometry_same {
                    slices.push(AnimatedSlice::reflow_move(
                        key,
                        old_line.id,
                        old_sr,
                        old_doc,
                        new_line.id,
                        new_sr.clone(),
                        new_doc,
                        new_cluster.byte_start,
                        new_cluster.byte_end,
                        Some(old_cluster.shaping_identity.clone()),
                    ));
                    run_managed_new_clusters.push((nref.line_idx, nref.cluster_idx, new_sr));
                }
            } else {
                // shaping 改变：old 淡出 + new 淡入
                slices.push(AnimatedSlice::reflow_crossfade_old(
                    key,
                    old_line.id,
                    old_sr,
                    old_doc.clone(),
                    new_doc.clone(),
                    new_cluster.byte_start,
                    new_cluster.byte_end,
                ));
                slices.push(AnimatedSlice::reflow_crossfade_new(
                    key,
                    new_line.id,
                    new_sr.clone(),
                    old_doc,
                    new_doc,
                    new_cluster.byte_start,
                    new_cluster.byte_end,
                ));
                run_managed_new_clusters.push((nref.line_idx, nref.cluster_idx, new_sr));
            }
            continue;
        }

        // N→M（多 old ↔ 多 new）：无法可靠一一对应时，每个成员独立动画。
        // old 每个在自己的 old_doc 原位 fade-out（from == to == old_doc，不移动只淡出）。
        // new 每个在自己的 new_doc 原位 fade-in（from == to == new_doc，不移动只淡入）。
        // 不再拿第一个 cluster 当 run 锚点——避免整组文字往一个 cluster 上聚拢。
        for oref in run_old {
            let old_line = &old_snapshot.line_snapshots[oref.line_idx];
            let old_cluster = &old_line.clusters[oref.cluster_idx];
            let old_sr = old_cluster.source_rect.clone();
            let old_doc = old_line.source_rect_to_document_rect(&old_sr);

            slices.push(AnimatedSlice::reflow_crossfade_old(
                key,
                old_line.id,
                old_sr,
                old_doc.clone(),
                old_doc,
                old_cluster.byte_start,
                old_cluster.byte_end,
            ));
        }

        for nref in run_new {
            let new_line = &new_snapshot.line_snapshots[nref.line_idx];
            let new_cluster = &new_line.clusters[nref.cluster_idx];
            let new_sr = new_cluster.source_rect.clone();
            let new_doc = new_line.source_rect_to_document_rect(&new_sr);

            slices.push(AnimatedSlice::reflow_crossfade_new(
                key,
                new_line.id,
                new_sr.clone(),
                new_doc.clone(),
                new_doc,
                new_cluster.byte_start,
                new_cluster.byte_end,
            ));
            run_managed_new_clusters.push((nref.line_idx, nref.cluster_idx, new_sr));
        }
    }

    // ── 阶段 5：生成 StaticLinePatches ──
    // 按 line_idx 分组，只为有被接管 cluster 的行生成 patch
    let mut patches_by_line: std::collections::HashMap<usize, Vec<SourceRect>> =
        std::collections::HashMap::new();
    for (line_idx, _cluster_idx, sr) in &run_managed_new_clusters {
        patches_by_line
            .entry(*line_idx)
            .or_default()
            .push(sr.clone());
    }
    for (line_idx, hidden_rects) in patches_by_line {
        let new_line = &new_snapshot.line_snapshots[line_idx];
        static_patches.push(StaticLinePatch::reflow_patch(
            new_line.id,
            hidden_rects,
            Vec::new(),
            new_line.byte_start,
            new_line.byte_end,
        ));
    }

    let slices = merge_adjacent_slices(slices);
    (slices, static_patches)
}

/// 合并相邻同类型、同方向、同快照的动画切片为 run，避免一个字一个 slice。
///
/// 合并条件（全部满足才合并）：
/// 1. 相同 `kind`（AnimatedSliceKind）
/// 2. 相同 `snapshot_id`（来自同一行快照）
/// 3. 相邻 byte range：`slice[i].byte_end == slice[i+1].byte_start`
/// 4. 同方向：
///    - InsertReveal：`from_document_rect` 的 y 相同（同一行吐字）
///    - DeleteConceal：`from_document_rect` 的 y 相同（同一行吞字）且 `conceal_from_left` 相同
///    - ReflowMove：移动向量相同（dx/dy 差值在 0.5 像素以内）
///    - ReflowCrossFade：移动向量相同
///
/// 合并操作：byte range 取 min/max，矩形取 bounding box，标量取第一个 slice 的值。
/// 合并是贪心的：一旦合并就继续尝试与下一个合并。不改变 slices 的顺序。
fn merge_adjacent_slices(slices: Vec<AnimatedSlice>) -> Vec<AnimatedSlice> {
    if slices.len() <= 1 {
        return slices;
    }

    let mut result: Vec<AnimatedSlice> = Vec::with_capacity(slices.len());
    let mut current = slices[0].clone();

    for next in &slices[1..] {
        if can_merge(&current, next) {
            current = merge_two(&current, next);
        } else {
            result.push(current);
            current = next.clone();
        }
    }
    result.push(current);
    result
}

/// 判断两个相邻 slice 是否可以合并。
fn can_merge(a: &AnimatedSlice, b: &AnimatedSlice) -> bool {
    // 条件 1：相同 kind
    if a.kind != b.kind {
        return false;
    }
    // 条件 2：相同 snapshot_id
    if a.snapshot_id != b.snapshot_id {
        return false;
    }
    // 条件 3：相邻 byte range
    if a.byte_end != b.byte_start {
        return false;
    }
    // 条件 4：同方向
    match a.kind {
        AnimatedSliceKind::InsertReveal => {
            // 同一行吐字：from_document_rect 的 y 相同
            (a.from_document_rect.y - b.from_document_rect.y).abs() < 0.5
        }
        AnimatedSliceKind::DeleteConceal => {
            // 同一行吞字且同方向：from_document_rect 的 y 相同，conceal_from_left 相同
            (a.from_document_rect.y - b.from_document_rect.y).abs() < 0.5
                && a.conceal_from_left == b.conceal_from_left
        }
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
            // 移动向量相同：dx = to.x - from.x, dy = to.y - from.y
            let a_dx = a.to_document_rect.x - a.from_document_rect.x;
            let a_dy = a.to_document_rect.y - a.from_document_rect.y;
            let b_dx = b.to_document_rect.x - b.from_document_rect.x;
            let b_dy = b.to_document_rect.y - b.from_document_rect.y;
            (a_dx - b_dx).abs() < 0.5 && (a_dy - b_dy).abs() < 0.5
        }
    }
}

/// 合并两个 slice 为一个 run。
fn merge_two(a: &AnimatedSlice, b: &AnimatedSlice) -> AnimatedSlice {
    AnimatedSlice {
        kind: a.kind,
        snapshot_id: a.snapshot_id,
        source_rect: bounding_box(&a.source_rect, &b.source_rect),
        from_document_rect: bounding_box(&a.from_document_rect, &b.from_document_rect),
        to_document_rect: bounding_box(&a.to_document_rect, &b.to_document_rect),
        opacity_from: a.opacity_from,
        opacity_to: a.opacity_to,
        scale_from: a.scale_from,
        scale_to: a.scale_to,
        byte_start: a.byte_start.min(b.byte_start),
        byte_end: a.byte_end.max(b.byte_end),
        shaping_identity: a.shaping_identity.clone(),
        conceal_from_left: a.conceal_from_left,
        start_fraction: a.start_fraction.min(b.start_fraction),
    }
}

/// 计算两个 SourceRect 的 bounding box（取最小 x/y 和最大 right/bottom）。
fn bounding_box(a: &SourceRect, b: &SourceRect) -> SourceRect {
    let min_x = a.x.min(b.x);
    let min_y = a.y.min(b.y);
    let max_right = (a.x + a.w).max(b.x + b.w);
    let max_bottom = (a.y + a.h).max(b.y + b.h);
    SourceRect {
        x: min_x,
        y: min_y,
        w: max_right - min_x,
        h: max_bottom - min_y,
    }
}

/// Linux Qt 文字动画协调器 — 管理动画事务的生命周期和 rebase。
///
/// - `next_key_id`：事务键 ID 分配器（单调递增），每个事务有唯一键用于取消和 rebase。
/// - `prepared_queue`：已准备好的动画事务队列，按时间线顺序执行。
/// - `layout_revision`：上次处理事务时的布局修订号，用于检测布局是否已变化。
///   与 `EditorKernel.revision` 不同——`layout_revision` 跟踪排版结果变更（含窗口宽度、字号等），
///   `revision` 跟踪文本内容变更。两者独立递增。
pub(crate) struct LinuxEditorAnimationCoordinator {
    next_key_id: u64,
    pub(crate) prepared_queue: PreparedTransactionQueue,
    layout_revision: LayoutRevision,
    /// 打字/预输入动画时长（毫秒）。本地生成的事务不来自 core 的
    /// `EditorVisualTransaction`，因此在此持有该视觉配置，与 core 把
    /// `duration_ms` 放进 visual transaction 结构体的设计方向一致。
    typing_animation_duration_ms: u32,
    /// 光标平滑移动动画时长（毫秒）。
    cursor_animation_duration_ms: u32,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            next_key_id: 1,
            prepared_queue: PreparedTransactionQueue::new(),
            layout_revision: LayoutRevision::initial(),
            typing_animation_duration_ms: 160,
            cursor_animation_duration_ms: 120,
        }
    }

    pub(crate) fn alloc_key(&mut self) -> VisualTransactionKey {
        let id = self.next_key_id;
        self.next_key_id += 1;
        VisualTransactionKey::new(id, id)
    }

    pub(crate) fn set_typing_animation_duration_ms(&mut self, ms: u32) {
        self.typing_animation_duration_ms = ms;
    }

    pub(crate) fn set_cursor_animation_duration_ms(&mut self, ms: u32) {
        self.cursor_animation_duration_ms = ms;
    }

    pub fn process_transaction(
        &mut self,
        vt: &EditorVisualTransaction,
        typing_animation_enabled: bool,
        is_scrolling: bool,
        is_loading: bool,
        is_applying_format: bool,
        is_applying_settings: bool,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
    ) -> Option<VisualTransactionKey> {
        if !typing_animation_enabled
            || is_scrolling
            || is_loading
            || is_applying_format
            || is_applying_settings
        {
            return None;
        }

        let mode = AnimationMode::from_core(vt.animation_mode);
        if !mode.should_create_transaction() {
            return None;
        }

        let new_revision = LayoutRevision::next();

        match vt.kind {
            EditorAnimationKind::Insert => {
                if let Some(range) = vt.inserted_range {
                    let range_start = range.start().value();
                    let range_end = range.end().value();
                    let conflicting = self
                        .prepared_queue
                        .find_conflicting_transaction(range_start, range_end);
                    let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> =
                        if let Some(old_key) = conflicting {
                            let mut frames = Vec::new();
                            if let Some(old_tx) = self
                                .prepared_queue
                                .active_transactions()
                                .iter()
                                .find(|t| t.key == old_key)
                            {
                                let old_progress = old_tx.progress(Instant::now());
                                if old_progress > 0.0 && old_progress < 1.0 {
                                    for old_slice in &old_tx.slices {
                                        let frame = old_slice.compute_frame(old_progress);
                                        let visible_fraction = match old_slice.kind {
                                            AnimatedSliceKind::InsertReveal => {
                                                if old_slice.to_document_rect.w > 0.0 {
                                                    frame.w / old_slice.to_document_rect.w
                                                } else {
                                                    0.0
                                                }
                                            }
                                            AnimatedSliceKind::DeleteConceal => {
                                                if old_slice.from_document_rect.w > 0.0 {
                                                    frame.w / old_slice.from_document_rect.w
                                                } else {
                                                    0.0
                                                }
                                            }
                                            _ => 0.0,
                                        };
                                        frames.push((
                                            old_slice.byte_start,
                                            old_slice.byte_end,
                                            frame.x,
                                            frame.y,
                                            frame.opacity,
                                            old_slice.shaping_identity.clone(),
                                            visible_fraction,
                                        ));
                                    }
                                }
                            }
                            self.prepared_queue.cancel(old_key, "rebased");
                            frames
                        } else {
                            Vec::new()
                        };

                    let key = self.alloc_key();
                    let mut slices = Vec::new();
                    let mut static_patches = Vec::new();

                    let insert_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);

                    // Issue #687: Insert 事务先生成显式 InsertReveal，再调用 reflow builder；
                    // reflow 必须排除 inserted_range。changed range 由 Core 显式拥有。
                    let inserted_range_tuple = (range_start, range_end);
                    let (reveal_slices, reveal_patches) = build_insert_reveal_slices(
                        key,
                        new_snapshot,
                        inserted_range_tuple,
                    );
                    slices.extend(reveal_slices);
                    static_patches.extend(reveal_patches);

                    let (reflow_slices, reflow_patches) = build_cluster_reflow_slices(
                        key,
                        old_snapshot,
                        new_snapshot,
                        &insert_offset_map,
                        &[],
                        &[inserted_range_tuple],
                        old_cursor_rect.as_ref(),
                        new_cursor_rect.as_ref(),
                    );
                    slices.extend(reflow_slices);
                    static_patches.extend(reflow_patches);

                    match_rebase_frames(&rebase_frames, &mut slices, &insert_offset_map);

                    // Issue #690 评论 5675007226 步骤 5: rebase 诊断事件。
                    if !rebase_frames.is_empty() {
                        editor_animation_debug_log(&format!(
                            "anim_rebase: new_key={:?} rebase_count={} rebase_fractions={:?}",
                            key,
                            rebase_frames.len(),
                            rebase_frames.iter().map(|r| r.6).collect::<Vec<_>>(),
                        ));
                    }

                    // Issue #687: 动画生命周期日志
                    eprintln!(
                        "[BUGFIX_687] Insert tx: key={:?}, inserted_range={:?}, slice_kinds={:?}, slice_count={}",
                        key,
                        inserted_range_tuple,
                        slices.iter().map(|s| s.kind).collect::<Vec<_>>(),
                        slices.len()
                    );

                    // Issue #690 评论 5675007226 步骤 5: 紧凑诊断事件日志。
                    // 每笔动画只留一条进正式诊断日志：transaction key、operation kind、
                    // old/new caret、visual unit kinds、首帧时间。
                    let log_slice_kinds: Vec<String> =
                        slices.iter().map(|s| format!("{:?}", s.kind)).collect();
                    let log_old_caret = old_cursor_rect
                        .as_ref()
                        .map(|r| format!("({:.1},{:.1})", r.x, r.top));
                    let log_new_caret = new_cursor_rect
                        .as_ref()
                        .map(|r| format!("({:.1},{:.1})", r.x, r.top));

                    let prepared = PreparedTextVisualTransaction {
                        key,
                        state: TextVisualTransactionState::Pending,
                        operation_kind: TextVisualOperationKind::Insert,
                        timeline: TransactionTimeline::new(vt.duration_ms),
                        slices,
                        static_patches,
                        old_cursor_rect,
                        new_cursor_rect,
                        cancel_reason: None,
                        texture_prepared: false,
                        old_snapshot: Some(old_snapshot.clone()),
                        new_snapshot: Some(new_snapshot.clone()),
                    };

                    self.layout_revision = new_revision;
                    self.prepared_queue.enqueue(prepared);

                    editor_animation_debug_log(&format!(
                        "anim_event: key={:?} op=Insert inserted={:?} slice_kinds={:?} old_caret={:?} new_caret={:?}",
                        key,
                        inserted_range_tuple,
                        log_slice_kinds,
                        log_old_caret,
                        log_new_caret,
                    ));

                    return Some(key);
                }
            }
            EditorAnimationKind::Delete => {
                let deleted_ranges: Vec<(usize, usize)> = if let Some(range) = vt.deleted_range {
                    vec![(range.start().value(), range.end().value())]
                } else {
                    let changes = writer_core::editor::diff_plain_text(&vt.old_text, &vt.new_text);
                    let mut ranges = Vec::new();
                    for change in &changes {
                        if let writer_core::editor::EditorChange::Delete { index, text } = change {
                            let range_start = index.value();
                            let range_end = range_start + text.len();
                            ranges.push((range_start, range_end));
                        }
                    }
                    ranges
                };

                let rebase_byte_start = deleted_ranges.first().map(|(s, _)| *s).unwrap_or(0);
                let rebase_byte_end = deleted_ranges.last().map(|(_, e)| *e).unwrap_or(0);
                let conflicting = self
                    .prepared_queue
                    .find_conflicting_transaction(rebase_byte_start, rebase_byte_end);
                let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> =
                    if let Some(old_key) = conflicting {
                        let mut frames = Vec::new();
                        if let Some(old_tx) = self
                            .prepared_queue
                            .active_transactions()
                            .iter()
                            .find(|t| t.key == old_key)
                        {
                            let old_progress = old_tx.progress(Instant::now());
                            if old_progress > 0.0 && old_progress < 1.0 {
                                for old_slice in &old_tx.slices {
                                    let frame = old_slice.compute_frame(old_progress);
                                    let visible_fraction = match old_slice.kind {
                                        AnimatedSliceKind::InsertReveal => {
                                            if old_slice.to_document_rect.w > 0.0 {
                                                frame.w / old_slice.to_document_rect.w
                                            } else {
                                                0.0
                                            }
                                        }
                                        AnimatedSliceKind::DeleteConceal => {
                                            if old_slice.from_document_rect.w > 0.0 {
                                                frame.w / old_slice.from_document_rect.w
                                            } else {
                                                0.0
                                            }
                                        }
                                        _ => 0.0,
                                    };
                                    frames.push((
                                        old_slice.byte_start,
                                        old_slice.byte_end,
                                        frame.x,
                                        frame.y,
                                        frame.opacity,
                                        old_slice.shaping_identity.clone(),
                                        visible_fraction,
                                    ));
                                }
                            }
                        }
                        self.prepared_queue.cancel(old_key, "rebased");
                        frames
                    } else {
                        Vec::new()
                    };

                let key = self.alloc_key();
                let new_revision = LayoutRevision::next();

                let mut slices = Vec::new();
                let mut static_patches = Vec::new();

                let delete_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);

                // Issue #687: Delete 事务先生成显式 DeleteConceal，再调用 reflow builder；
                // reflow 必须排除 deleted_range。changed range 由 Core 显式拥有。
                // 对每个 deleted range 生成显式 DeleteConceal 切片。
                for &(d_start, d_end) in &deleted_ranges {
                    let conceal_slices = build_delete_conceal_slices(
                        key,
                        old_snapshot,
                        (d_start, d_end),
                        new_cursor_rect.as_ref(),
                    );
                    slices.extend(conceal_slices);
                }

                let (reflow_slices, reflow_patches) = build_cluster_reflow_slices(
                    key,
                    old_snapshot,
                    new_snapshot,
                    &delete_offset_map,
                    &deleted_ranges,
                    &[],
                    old_cursor_rect.as_ref(),
                    new_cursor_rect.as_ref(),
                );
                slices.extend(reflow_slices);
                static_patches.extend(reflow_patches);

                match_rebase_frames(&rebase_frames, &mut slices, &delete_offset_map);

                // Issue #687: 动画生命周期日志
                eprintln!(
                    "[BUGFIX_687] Delete tx: key={:?}, deleted_ranges={:?}, slice_kinds={:?}, slice_count={}",
                    key,
                    deleted_ranges,
                    slices.iter().map(|s| s.kind).collect::<Vec<_>>(),
                    slices.len()
                );

                // Issue #690 评论 5675007226 步骤 5: 紧凑诊断事件日志。
                let log_slice_kinds: Vec<String> =
                    slices.iter().map(|s| format!("{:?}", s.kind)).collect();
                let log_old_caret = old_cursor_rect
                    .as_ref()
                    .map(|r| format!("({:.1},{:.1})", r.x, r.top));
                let log_new_caret = new_cursor_rect
                    .as_ref()
                    .map(|r| format!("({:.1},{:.1})", r.x, r.top));

                let prepared = PreparedTextVisualTransaction {
                    key,
                    state: TextVisualTransactionState::Pending,
                    operation_kind: TextVisualOperationKind::Delete,
                    timeline: TransactionTimeline::new(vt.duration_ms),
                    slices,
                    static_patches,
                    old_cursor_rect: old_cursor_rect.clone(),
                    new_cursor_rect: new_cursor_rect.clone(),
                    cancel_reason: None,
                    texture_prepared: false,
                    old_snapshot: Some(old_snapshot.clone()),
                    new_snapshot: Some(new_snapshot.clone()),
                };

                self.layout_revision = new_revision;
                self.prepared_queue.enqueue(prepared);

                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Delete deleted={:?} slice_kinds={:?} old_caret={:?} new_caret={:?}",
                    key,
                    deleted_ranges,
                    log_slice_kinds,
                    log_old_caret,
                    log_new_caret,
                ));

                return Some(key);
            }
            EditorAnimationKind::Cursor => {
                let key = self.alloc_key();
                let new_revision = LayoutRevision::next();

                let prepared = PreparedTextVisualTransaction {
                    key,
                    state: TextVisualTransactionState::Pending,
                    operation_kind: TextVisualOperationKind::Cursor,
                    timeline: TransactionTimeline::new(vt.duration_ms),
                    slices: Vec::new(),
                    static_patches: Vec::new(),
                    old_cursor_rect,
                    new_cursor_rect,
                    cancel_reason: None,
                    texture_prepared: false,
                    old_snapshot: None,
                    new_snapshot: None,
                };

                self.layout_revision = new_revision;
                self.prepared_queue.enqueue(prepared);
                return Some(key);
            }
        }
        None
    }

    pub fn handle_composition_update(
        &mut self,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        composition_byte_start: usize,
        composition_byte_end: usize,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    ) -> Option<VisualTransactionKey> {
        let conflicting = self
            .prepared_queue
            .find_conflicting_transaction(composition_byte_start, composition_byte_end);
        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> =
            if let Some(old_key) = conflicting {
                let mut frames = Vec::new();
                if let Some(old_tx) = self
                    .prepared_queue
                    .active_transactions()
                    .iter()
                    .find(|t| t.key == old_key)
                {
                    let old_progress = old_tx.progress(Instant::now());
                    if old_progress > 0.0 && old_progress < 1.0 {
                        for old_slice in &old_tx.slices {
                            let frame = old_slice.compute_frame(old_progress);
                            let visible_fraction = match old_slice.kind {
                                AnimatedSliceKind::InsertReveal => {
                                    if old_slice.to_document_rect.w > 0.0 {
                                        frame.w / old_slice.to_document_rect.w
                                    } else {
                                        0.0
                                    }
                                }
                                AnimatedSliceKind::DeleteConceal => {
                                    if old_slice.from_document_rect.w > 0.0 {
                                        frame.w / old_slice.from_document_rect.w
                                    } else {
                                        0.0
                                    }
                                }
                                _ => 0.0,
                            };
                            frames.push((
                                old_slice.byte_start,
                                old_slice.byte_end,
                                frame.x,
                                frame.y,
                                frame.opacity,
                                old_slice.shaping_identity.clone(),
                                visible_fraction,
                            ));
                        }
                    }
                }
                self.prepared_queue.cancel(old_key, "rebased");
                frames
            } else {
                Vec::new()
            };

        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);

        let key = self.alloc_key();
        let new_revision = LayoutRevision::next();

        let mut slices = Vec::new();
        let mut static_patches = Vec::new();

        // Issue #687: IME 组合更新也显式拥有 changed range。
        // 用 diff_plain_text 找到 inserted/deleted range，显式生成 InsertReveal/DeleteConceal，
        // reflow 只处理 unchanged material。
        let comp_changes = writer_core::editor::diff_plain_text(
            &old_snapshot.virtual_text,
            &new_snapshot.virtual_text,
        );
        let mut comp_inserted_ranges: Vec<(usize, usize)> = Vec::new();
        let mut comp_deleted_ranges: Vec<(usize, usize)> = Vec::new();
        for change in &comp_changes {
            match change {
                writer_core::editor::EditorChange::Insert { index, text } => {
                    let rs = index.value();
                    comp_inserted_ranges.push((rs, rs + text.len()));
                }
                writer_core::editor::EditorChange::Delete { index, text } => {
                    let rs = index.value();
                    comp_deleted_ranges.push((rs, rs + text.len()));
                }
            }
        }

        for &(i_start, i_end) in &comp_inserted_ranges {
            let (reveal_slices, reveal_patches) =
                build_insert_reveal_slices(key, new_snapshot, (i_start, i_end));
            slices.extend(reveal_slices);
            static_patches.extend(reveal_patches);
        }
        for &(d_start, d_end) in &comp_deleted_ranges {
            let conceal_slices = build_delete_conceal_slices(
                key,
                old_snapshot,
                (d_start, d_end),
                new_cursor_rect.as_ref(),
            );
            slices.extend(conceal_slices);
        }

        let (reflow_slices, reflow_patches) = build_cluster_reflow_slices(
            key,
            old_snapshot,
            new_snapshot,
            &offset_map,
            &comp_deleted_ranges,
            &comp_inserted_ranges,
            old_cursor_rect.as_ref(),
            new_cursor_rect.as_ref(),
        );
        slices.extend(reflow_slices);
        static_patches.extend(reflow_patches);

        match_rebase_frames(&rebase_frames, &mut slices, &offset_map);

        let prepared = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::CompositionUpdate,
            timeline: TransactionTimeline::new(u64::from(self.typing_animation_duration_ms)),
            slices,
            static_patches,
            old_cursor_rect,
            new_cursor_rect,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: Some(old_snapshot.clone()),
            new_snapshot: Some(new_snapshot.clone()),
        };

        self.layout_revision = new_revision;
        self.prepared_queue.enqueue(prepared);
        Some(key)
    }

    pub fn handle_composition_commit_or_cancel(
        &mut self,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        preedit_byte_start: usize,
        preedit_byte_end: usize,
        is_commit: bool,
        visual_text_unchanged: bool,
        candidate_byte_start: usize,
        candidate_byte_end: usize,
        committed_replace_start: usize,
        committed_replace_end: usize,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    ) -> Option<VisualTransactionKey> {
        let conflict_start = committed_replace_start.min(preedit_byte_start);
        let conflict_end = committed_replace_end.max(preedit_byte_end);
        let conflicting = self
            .prepared_queue
            .find_conflicting_transaction(conflict_start, conflict_end);
        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> =
            if let Some(old_key) = conflicting {
                let mut frames = Vec::new();
                if let Some(old_tx) = self
                    .prepared_queue
                    .active_transactions()
                    .iter()
                    .find(|t| t.key == old_key)
                {
                    let old_progress = old_tx.progress(Instant::now());
                    if old_progress > 0.0 && old_progress < 1.0 {
                        for old_slice in &old_tx.slices {
                            let frame = old_slice.compute_frame(old_progress);
                            let visible_fraction = match old_slice.kind {
                                AnimatedSliceKind::InsertReveal => {
                                    if old_slice.to_document_rect.w > 0.0 {
                                        frame.w / old_slice.to_document_rect.w
                                    } else {
                                        0.0
                                    }
                                }
                                AnimatedSliceKind::DeleteConceal => {
                                    if old_slice.from_document_rect.w > 0.0 {
                                        frame.w / old_slice.from_document_rect.w
                                    } else {
                                        0.0
                                    }
                                }
                                _ => 0.0,
                            };
                            frames.push((
                                old_slice.byte_start,
                                old_slice.byte_end,
                                frame.x,
                                frame.y,
                                frame.opacity,
                                old_slice.shaping_identity.clone(),
                                visible_fraction,
                            ));
                        }
                    }
                }
                self.prepared_queue.cancel(old_key, "rebased");
                frames
            } else {
                Vec::new()
            };

        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);

        let key = self.alloc_key();
        let new_revision = LayoutRevision::next();

        let mut slices = Vec::new();
        let mut static_patches = Vec::new();

        if !is_commit {
            // Issue #687: cancel 时显式生成 DeleteConceal for preedit 范围的 old cluster，
            // reflow 只处理 unchanged material。changed range 由显式函数拥有。
            let cancel_deleted_range = (preedit_byte_start, preedit_byte_end);
            let conceal_slices = build_delete_conceal_slices(
                key,
                old_snapshot,
                cancel_deleted_range,
                new_cursor_rect.as_ref(),
            );
            slices.extend(conceal_slices);

            let cancel_excluded_old: [(usize, usize); 1] = [cancel_deleted_range];
            let (reflow_slices, reflow_patches) = build_cluster_reflow_slices(
                key,
                old_snapshot,
                new_snapshot,
                &offset_map,
                &cancel_excluded_old,
                &[],
                old_cursor_rect.as_ref(),
                new_cursor_rect.as_ref(),
            );
            slices.extend(reflow_slices);
            static_patches.extend(reflow_patches);
        } else {
            if visual_text_unchanged {
            } else {
                let insert_cx = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
                let insert_cy = old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);
                let shrink_x = new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
                let shrink_y = new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

                for old_line in
                    old_snapshot.lines_in_byte_range(preedit_byte_start, preedit_byte_end)
                {
                    for old_cluster in
                        old_line.clusters_in_byte_range(preedit_byte_start, preedit_byte_end)
                    {
                        let mapped_new_bs = offset_map.map_old_to_new(old_cluster.byte_start);
                        let mapped_new_be = offset_map.map_old_to_new(old_cluster.byte_end);
                        let matched_in_new =
                            if let (Some(mbs), Some(mbe)) = (mapped_new_bs, mapped_new_be) {
                                new_snapshot.line_snapshots.iter().any(|nl| {
                                    nl.clusters
                                        .iter()
                                        .any(|nc| nc.byte_start == mbs && nc.byte_end == mbe)
                                })
                            } else {
                                false
                            };
                        if !matched_in_new {
                            if let Some(old_sr) = old_line.source_rect_for_byte_range(
                                old_cluster.byte_start,
                                old_cluster.byte_end,
                            ) {
                                let from_doc = old_line.source_rect_to_document_rect(&old_sr);
                                // Issue #686 评论 5666452462：cancel 时 preedit 文字
                                // 走 delete_conceal，按 old rect 两侧与新光标距离
                                // 决定收进方向：靠近右端 → 保留左段（Backspace），
                                // 靠近左端 → 保留右段（Delete 键）。
                                let left = from_doc.x;
                                let right = from_doc.x + from_doc.w;
                                let conceal_from_left =
                                    (shrink_x - right).abs() <= (shrink_x - left).abs();
                                slices.push(AnimatedSlice::delete_conceal(
                                    key,
                                    old_line.id,
                                    old_sr,
                                    from_doc,
                                    shrink_x,
                                    shrink_y,
                                    old_cluster.byte_start,
                                    old_cluster.byte_end,
                                    Some(old_cluster.shaping_identity.clone()),
                                    conceal_from_left,
                                ));
                            }
                        } else if let (Some(mbs), Some(mbe)) = (mapped_new_bs, mapped_new_be) {
                            if let Some((new_line, new_cluster)) = new_snapshot
                                .line_snapshots
                                .iter()
                                .filter_map(|nl| {
                                    nl.clusters
                                        .iter()
                                        .find(|nc| nc.byte_start == mbs && nc.byte_end == mbe)
                                        .map(|nc| (nl, nc))
                                })
                                .next()
                            {
                                if !old_cluster
                                    .shaping_identity
                                    .is_same_shaping(&new_cluster.shaping_identity)
                                {
                                    if let Some(old_sr) = old_line.source_rect_for_byte_range(
                                        old_cluster.byte_start,
                                        old_cluster.byte_end,
                                    ) {
                                        let old_doc =
                                            old_line.source_rect_to_document_rect(&old_sr);
                                        if let Some(new_sr) = new_line.source_rect_for_byte_range(
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                        ) {
                                            let new_doc =
                                                new_line.source_rect_to_document_rect(&new_sr);
                                            slices.push(AnimatedSlice::reflow_crossfade_old(
                                                key,
                                                old_line.id,
                                                old_sr,
                                                old_doc,
                                                new_doc,
                                                old_cluster.byte_start,
                                                old_cluster.byte_end,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                for new_line in
                    new_snapshot.lines_in_byte_range(candidate_byte_start, candidate_byte_end)
                {
                    for new_cluster in
                        new_line.clusters_in_byte_range(candidate_byte_start, candidate_byte_end)
                    {
                        let mapped_old_bs = offset_map.map_new_to_old(new_cluster.byte_start);
                        let mapped_old_be = offset_map.map_new_to_old(new_cluster.byte_end);
                        let found_in_old =
                            if let (Some(mbs), Some(mbe)) = (mapped_old_bs, mapped_old_be) {
                                old_snapshot.line_snapshots.iter().any(|ol| {
                                    ol.clusters
                                        .iter()
                                        .any(|oc| oc.byte_start == mbs && oc.byte_end == mbe)
                                })
                            } else {
                                false
                            };
                        if !found_in_old {
                            if let Some(new_sr) = new_line.source_rect_for_byte_range(
                                new_cluster.byte_start,
                                new_cluster.byte_end,
                            ) {
                                let to_doc = new_line.source_rect_to_document_rect(&new_sr);
                                slices.push(AnimatedSlice::insert_reveal(
                                    key,
                                    new_line.id,
                                    new_sr.clone(),
                                    to_doc,
                                    insert_cx,
                                    insert_cy,
                                    new_cluster.byte_start,
                                    new_cluster.byte_end,
                                    Some(new_cluster.shaping_identity.clone()),
                                ));
                                static_patches.push(StaticLinePatch::insert_patch(
                                    new_line.id,
                                    vec![new_sr],
                                    Vec::new(),
                                    new_cluster.byte_start,
                                    new_cluster.byte_end,
                                ));
                            }
                        } else if let (Some(mbs), Some(mbe)) = (mapped_old_bs, mapped_old_be) {
                            if let Some((old_line, old_cluster)) = old_snapshot
                                .line_snapshots
                                .iter()
                                .filter_map(|ol| {
                                    ol.clusters
                                        .iter()
                                        .find(|oc| oc.byte_start == mbs && oc.byte_end == mbe)
                                        .map(|oc| (ol, oc))
                                })
                                .next()
                            {
                                let same_shaping = old_cluster
                                    .shaping_identity
                                    .is_same_shaping(&new_cluster.shaping_identity);
                                if !same_shaping {
                                    if let Some(new_sr) = new_line.source_rect_for_byte_range(
                                        new_cluster.byte_start,
                                        new_cluster.byte_end,
                                    ) {
                                        let old_doc = old_line.source_rect_to_document_rect(
                                            &old_line
                                                .source_rect_for_byte_range(
                                                    old_cluster.byte_start,
                                                    old_cluster.byte_end,
                                                )
                                                .unwrap_or(SourceRect::zero()),
                                        );
                                        let new_doc =
                                            new_line.source_rect_to_document_rect(&new_sr);
                                        slices.push(AnimatedSlice::reflow_crossfade_new(
                                            key,
                                            new_line.id,
                                            new_sr.clone(),
                                            old_doc,
                                            new_doc,
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                        ));
                                        static_patches.push(StaticLinePatch::insert_patch(
                                            new_line.id,
                                            vec![new_sr],
                                            Vec::new(),
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                        ));
                                    }
                                } else {
                                    if let (Some(old_sr), Some(new_sr)) = (
                                        old_line.source_rect_for_byte_range(
                                            old_cluster.byte_start,
                                            old_cluster.byte_end,
                                        ),
                                        new_line.source_rect_for_byte_range(
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                        ),
                                    ) {
                                        let old_doc =
                                            old_line.source_rect_to_document_rect(&old_sr);
                                        let new_doc =
                                            new_line.source_rect_to_document_rect(&new_sr);
                                        let geometry_same = (old_doc.x - new_doc.x).abs() < 0.5
                                            && (old_doc.y - new_doc.y).abs() < 0.5
                                            && (old_doc.w - new_doc.w).abs() < 0.5
                                            && (old_doc.h - new_doc.h).abs() < 0.5;
                                        if !geometry_same {
                                            slices.push(AnimatedSlice::reflow_move(
                                                key,
                                                old_line.id,
                                                old_sr,
                                                old_doc,
                                                new_line.id,
                                                new_sr.clone(),
                                                new_doc,
                                                new_cluster.byte_start,
                                                new_cluster.byte_end,
                                                Some(new_cluster.shaping_identity.clone()),
                                            ));
                                            static_patches.push(StaticLinePatch::insert_patch(
                                                new_line.id,
                                                vec![new_sr],
                                                Vec::new(),
                                                new_cluster.byte_start,
                                                new_cluster.byte_end,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let (reflow_slices, reflow_patches) = build_cluster_reflow_slices(
                    key,
                    old_snapshot,
                    new_snapshot,
                    &offset_map,
                    &[(preedit_byte_start, preedit_byte_end)],
                    &[(candidate_byte_start, candidate_byte_end)],
                    old_cursor_rect.as_ref(),
                    new_cursor_rect.as_ref(),
                );
                slices.extend(reflow_slices);
                static_patches.extend(reflow_patches);
            }
        }

        match_rebase_frames(&rebase_frames, &mut slices, &offset_map);

        let prepared = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::CompositionCommitOrCancel,
            timeline: TransactionTimeline::new(u64::from(self.typing_animation_duration_ms)),
            slices,
            static_patches,
            old_cursor_rect,
            new_cursor_rect,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: Some(old_snapshot.clone()),
            new_snapshot: Some(new_snapshot.clone()),
        };

        self.layout_revision = new_revision;
        self.prepared_queue.enqueue(prepared);
        Some(key)
    }

    pub fn active_composition_new_snapshot(&self) -> Option<&EditorLayoutSnapshot> {
        self.prepared_queue
            .active_transactions()
            .iter()
            .filter(|t| {
                t.operation_kind == TextVisualOperationKind::CompositionUpdate
                    && t.state != TextVisualTransactionState::Cancelled
                    && t.state != TextVisualTransactionState::Completed
            })
            .filter_map(|t| t.new_snapshot.as_ref())
            .next_back()
    }

    pub fn cancel_active_composition(&mut self, reason: &str) {
        let keys: Vec<VisualTransactionKey> = self
            .prepared_queue
            .active_transactions()
            .iter()
            .filter(|t| {
                t.operation_kind == TextVisualOperationKind::CompositionUpdate
                    && t.state != TextVisualTransactionState::Cancelled
                    && t.state != TextVisualTransactionState::Completed
            })
            .map(|t| t.key)
            .collect();
        for key in keys {
            self.prepared_queue.cancel(key, reason);
        }
    }

    pub fn handle_cursor_only(
        &mut self,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    ) -> Option<VisualTransactionKey> {
        let key = self.alloc_key();
        let new_revision = LayoutRevision::next();

        let prepared = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Cursor,
            timeline: TransactionTimeline::new(u64::from(self.cursor_animation_duration_ms)),
            slices: Vec::new(),
            static_patches: Vec::new(),
            old_cursor_rect: old_cursor_rect.clone(),
            new_cursor_rect: new_cursor_rect.clone(),
            cancel_reason: None,
            texture_prepared: true,
            old_snapshot: None,
            new_snapshot: None,
        };

        self.layout_revision = new_revision;
        self.prepared_queue.enqueue(prepared);
        // Issue #679 评论 5657313927 (3a): CursorOnly 没有文字切片也没有纹理准备阶段，
        // 创建后立即推进到 Prepared，不要让它以 Pending 留在队列里导致光标不移动。
        self.prepared_queue.mark_prepared(key);
        Some(key)
    }

    pub fn finish_by_key(&mut self, key: VisualTransactionKey) -> Option<Vec<LineSnapshotId>> {
        self.prepared_queue.complete(key)
    }

    pub fn cancel_by_key(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        self.prepared_queue.cancel(key, reason)
    }

    pub fn suppress_all(&mut self) -> bool {
        if self.prepared_queue.is_empty() {
            return false;
        }
        self.prepared_queue.cancel_all("suppress_all");
        true
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        let expired = self.prepared_queue.tick(now);
        !expired.is_empty()
    }

    pub fn has_active_insert(&self) -> bool {
        self.prepared_queue.has_active_insert()
    }

    /// Issue #679 评论 5657313927 (3c): 按 driver key 取样 Timeline 进度。
    ///
    /// 不再排除 1.0（旧 `active_cursor_progress` 过滤 `0 < p < 1` 导致 `p == 1`
    /// 永远送不到光标）。key 不存在返回 None 说明 Timeline 已结束/取消。
    pub(crate) fn cursor_timeline_sample(
        &self,
        key: VisualTransactionKey,
    ) -> Option<CursorTimelineSample> {
        let tx = self
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|tx| tx.key == key)?;

        match tx.state {
            TextVisualTransactionState::Pending | TextVisualTransactionState::Prepared => {
                Some(CursorTimelineSample::Waiting)
            }
            TextVisualTransactionState::Rendering | TextVisualTransactionState::Paused => Some(
                CursorTimelineSample::Running(tx.progress(Instant::now()).clamp(0.0, 1.0)),
            ),
            TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled => None,
        }
    }

    /// Issue #686 评论 5664857575 领域2：返回当前活动的正文编辑事务（Insert/Delete，
    /// 不含 Cursor）的 key。光标按事务身份绑定，不靠浮点坐标反查。
    ///
    /// 从新到旧找最近一条 state 不是 Completed/Cancelled 的正文事务（operation_kind
    /// 为 Insert/Delete/CompositionUpdate/CompositionCommitOrCancel）。
    pub(crate) fn active_text_transaction_key(&self) -> Option<VisualTransactionKey> {
        for tx in self.prepared_queue.active_transactions().iter().rev() {
            if matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                continue;
            }
            if tx.operation_kind != TextVisualOperationKind::Cursor {
                return Some(tx.key);
            }
        }
        None
    }

    /// Issue #679 评论 5657313927 (3d): 按当前光标 target 查找对应的事务。
    ///
    /// Issue #686 评论 5664857575 领域2：当存在活动正文事务时，直接返回该事务的 key
    /// 和它的 old/new cursor rect，不靠浮点坐标相等反查。光标按事务身份绑定。
    /// 只有在没有正文事务时才走原来的 CursorOnly 查找逻辑（按 target x/y 匹配）。
    pub(crate) fn find_cursor_transaction_for_target(
        &self,
        target_x: f64,
        target_y: f64,
        _target_h: f64,
    ) -> Option<(VisualTransactionKey, Option<CursorRect>, Option<CursorRect>)> {
        // 领域2：优先按事务身份绑定——存在活动正文事务时直接返回。
        if let Some(key) = self.active_text_transaction_key() {
            if let Some(tx) = self
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == key)
            {
                return Some((
                    tx.key,
                    tx.old_cursor_rect.clone(),
                    tx.new_cursor_rect.clone(),
                ));
            }
        }

        // 没有正文事务时走 CursorOnly 查找逻辑（按 target x/y 匹配）。
        for tx in self.prepared_queue.active_transactions().iter().rev() {
            if matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                continue;
            }
            if let Some(ref new_rect) = tx.new_cursor_rect {
                if (new_rect.x - target_x).abs() <= 0.01 && (new_rect.top - target_y).abs() <= 0.01
                {
                    return Some((
                        tx.key,
                        tx.old_cursor_rect.clone(),
                        tx.new_cursor_rect.clone(),
                    ));
                }
            }
        }
        None
    }

    pub fn has_prepared_or_rendering(&self) -> bool {
        self.prepared_queue.active_transactions().iter().any(|t| {
            t.state == TextVisualTransactionState::Prepared
                || t.state == TextVisualTransactionState::Rendering
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_cursor_plan(
        &self,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        cursor_x: f64,
        cursor_y: f64,
        cursor_h: f64,
        editor_enabled: bool,
        has_selection: bool,
        viewport_height: f64,
        is_scrolling: bool,
        is_selecting: bool,
        is_preediting: bool,
        smooth_cursor_enabled: bool,
        _smooth_cursor_duration_ms: u32,
        coordinated_enabled: bool,
        scroll_y: f64,
        old_scroll_y: f64,
        old_visible: bool,
        old_blink_visible: bool,
        old_visual_x: f64,
        old_visual_y: f64,
        force_snap_next: bool,
        cursor_animation: Option<&super::rendering::CursorAnimationState>,
        // Issue #679 评论 5657313927: Tween 必须带 driver key（从找到的或刚创建的事务获取）。
        // None 时所有 Tween 路径 fallback 到 Snap。
        driver_key: Option<VisualTransactionKey>,
    ) -> CursorAnimationPlan {
        let in_viewport = cursor_y + cursor_h > 0.0 && cursor_y < viewport_height;
        let should_be_visible = editor_enabled && !has_selection && in_viewport && !is_scrolling;

        let has_active = self.has_active_insert();
        // Issue #679 评论 5657313927: blink_mode 不再固化进 CursorAnimationPlan，
        // 由 tick_cursor_animation 每帧从 has_active_insert() 实时计算。
        let _blink_mode = if coordinated_enabled && has_active {
            CursorBlinkMode::Suppressed
        } else {
            CursorBlinkMode::Normal
        };

        let scroll_changed = (old_scroll_y - scroll_y).abs() > 0.01;

        let dy = (cursor_y - old_visual_y).abs();

        let cross_line_snap = dy > cursor_h * 3.0;

        // Issue #679 评论 5658087764 (1): force_snap_next 是一次性强制 Snap 标记，
        // 不再附加"距离够大才算"的条件；点击/滚动/选择/不可见/滚动变化都硬 Snap，
        // 不再被协调动画覆盖为 Tween。
        let hard_snap =
            force_snap_next || is_scrolling || is_selecting || !old_visible || scroll_changed;

        // Issue #679 评论 5657313927: 没有 driver key 时无法构造 Tween（需要 driver_key
        // 字段），fallback 到 Snap。
        let can_tween = driver_key.is_some();

        let transition = if !should_be_visible || hard_snap {
            CursorTransition::Snap
        } else if !smooth_cursor_enabled || cross_line_snap {
            if can_tween
                && coordinated_enabled
                && has_active
                && old_cursor_rect.is_some()
                && new_cursor_rect.is_some()
            {
                CursorTransition::Tween {
                    old_rect: old_cursor_rect.clone().unwrap(),
                    new_rect: new_cursor_rect.clone().unwrap(),
                    driver_key: driver_key.unwrap(),
                }
            } else {
                CursorTransition::Snap
            }
        } else if let Some(anim) = cursor_animation {
            if (anim.target_x - cursor_x).abs() > 0.01 || (anim.target_y - cursor_y).abs() > 0.01 {
                if can_tween
                    && coordinated_enabled
                    && has_active
                    && old_cursor_rect.is_some()
                    && new_cursor_rect.is_some()
                {
                    CursorTransition::Tween {
                        old_rect: old_cursor_rect.clone().unwrap(),
                        new_rect: new_cursor_rect.clone().unwrap(),
                        driver_key: driver_key.unwrap(),
                    }
                } else if can_tween {
                    CursorTransition::Tween {
                        old_rect: CursorRect {
                            x: anim.start_x,
                            top: anim.start_y,
                            bottom: anim.start_y + cursor_h,
                            baseline_y: anim.start_y + cursor_h * 0.8,
                        },
                        new_rect: CursorRect {
                            x: cursor_x,
                            top: cursor_y,
                            bottom: cursor_y + cursor_h,
                            baseline_y: cursor_y + cursor_h * 0.8,
                        },
                        driver_key: driver_key.unwrap(),
                    }
                } else {
                    CursorTransition::Snap
                }
            } else {
                CursorTransition::Snap
            }
        } else if (old_visual_x - cursor_x).abs() > 0.01 || (old_visual_y - cursor_y).abs() > 0.01 {
            if can_tween
                && coordinated_enabled
                && has_active
                && old_cursor_rect.is_some()
                && new_cursor_rect.is_some()
            {
                CursorTransition::Tween {
                    old_rect: old_cursor_rect.clone().unwrap(),
                    new_rect: new_cursor_rect.clone().unwrap(),
                    driver_key: driver_key.unwrap(),
                }
            } else if can_tween {
                CursorTransition::Tween {
                    old_rect: CursorRect {
                        x: old_visual_x,
                        top: old_visual_y,
                        bottom: old_visual_y + cursor_h,
                        baseline_y: old_visual_y + cursor_h * 0.8,
                    },
                    new_rect: CursorRect {
                        x: cursor_x,
                        top: cursor_y,
                        bottom: cursor_y + cursor_h,
                        baseline_y: cursor_y + cursor_h * 0.8,
                    },
                    driver_key: driver_key.unwrap(),
                }
            } else {
                CursorTransition::Snap
            }
        } else {
            CursorTransition::Snap
        };

        let _ = (is_preediting, old_blink_visible);

        CursorAnimationPlan {
            should_be_visible,
            transition,
            cursor_x,
            cursor_y,
            cursor_h,
        }
    }

    pub(crate) fn pause_all(&mut self) {
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.pause();
        }
    }

    pub(crate) fn resume_all(&mut self) {
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.resume();
        }
    }

    /// Issue #690 评论 5675007226 步骤 1+2: 接受 `frame_now`，统一采样文字和光标 progress。
    ///
    /// 文字和光标的 progress 全部从同一个 `frame_now` 计算，消除 GUI 线程 tick 和
    /// Scene Graph 渲染帧之间的采样偏差。当正文编辑事务活跃且 coordinated 动画启用时，
    /// 光标位置直接从 text animation progress 计算（跟随文字吞吐边界），
    /// 不再使用 GUI 线程上一帧留下的 `cursor_ctrl.visual_x/y`。
    pub(crate) fn build_render_plan_full(
        &mut self,
        mut cursor_render_state: CursorRenderState,
        selection_preedit: SelectionPreeditPlan,
        mut frame_context: super::render_plan::FrameContext,
        cursor_style: super::render_plan::CursorStyle,
        selection_preedit_style: super::render_plan::SelectionPreeditStyle,
        frame_now: Instant,
        coordinated_enabled: bool,
    ) -> RenderPlan {
        let (text_animation, keys_to_complete) =
            self.build_text_animation_plan_with_time(frame_now);
        frame_context.keys_to_complete = keys_to_complete;
        let active_keys: Vec<VisualTransactionKey> = self
            .prepared_queue
            .active_transactions()
            .iter()
            .map(|t| t.key)
            .collect();
        frame_context.active_transaction_keys = active_keys;

        // Issue #658: 收集已准备好的 static_patches 供静态正文裁剪。
        // 只有 texture_prepared == true 的事务才允许静态层隐藏，
        // 避免纹理准备完成前出现空白帧。
        // 同时将 hidden_source_rects 通过 source_rect_to_document_rect()
        // 转换为 doc_hidden_rects，供 QSGClipNode 直接使用文档逻辑坐标。
        // Issue #679 评论 5657313927 (3e): 只允许 Prepared / Rendering / Paused
        // 的事务裁剪静态正文；Pending 无论 texture_prepared 是什么都不能隐藏正文，
        // 否则资源还没准备好就会出现空洞。
        let mut static_patches = Vec::new();
        for tx in self.prepared_queue.active_transactions() {
            if tx.texture_prepared
                && matches!(
                    tx.state,
                    TextVisualTransactionState::Prepared
                        | TextVisualTransactionState::Rendering
                        | TextVisualTransactionState::Paused
                )
            {
                for mut patch in tx.static_patches.iter().cloned() {
                    // 查找对应行快照，将 hidden_source_rects 转换为文档坐标
                    if !patch.hidden_source_rects.is_empty() && patch.doc_hidden_rects.is_empty() {
                        if let Some(ref new_snapshot) = tx.new_snapshot {
                            if let Some(line_snap) = new_snapshot
                                .line_snapshots
                                .iter()
                                .find(|ls| ls.id == patch.snapshot_id)
                            {
                                patch.doc_hidden_rects = patch
                                    .hidden_source_rects
                                    .iter()
                                    .map(|sr| line_snap.source_rect_to_document_rect(sr))
                                    .collect();
                            }
                        }
                    }
                    static_patches.push(patch);
                }
            }
        }

        // Issue #690 评论 5675007226 步骤 2: 协同光标位置从同一 frame_now 计算。
        // 当正文编辑事务活跃且 coordinated 动画启用时，光标位置直接从 text animation
        // progress 计算（跟随文字吞吐边界），不再使用 GUI 线程上一帧留下的
        // cursor_ctrl.visual_x/y。
        if coordinated_enabled {
            if let Some((progress, old_rect, new_rect)) =
                self.compute_coordinated_cursor(frame_now)
            {
                let suppressed = matches!(
                    self.active_operation_kind(),
                    Some(TextVisualOperationKind::Insert)
                );
                let blink_mode = if suppressed {
                    CursorBlinkMode::Suppressed
                } else {
                    CursorBlinkMode::Normal
                };
                let eased = 1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(3);
                let x = old_rect.x + (new_rect.x - old_rect.x) * eased;
                let y = old_rect.top + (new_rect.top - old_rect.top) * eased;
                let h = new_rect.bottom - new_rect.top;
                let opacity = if blink_mode == CursorBlinkMode::Suppressed {
                    1.0
                } else {
                    cursor_render_state.opacity
                };
                cursor_render_state = CursorRenderState {
                    visible: true,
                    x,
                    y,
                    h,
                    opacity,
                };
            }
        }

        RenderPlan {
            text_animation,
            selection_preedit,
            cursor: cursor_render_state,
            frame_context,
            cursor_style,
            selection_preedit_style,
            static_patches,
        }
    }

    /// Issue #690 评论 5675007226 步骤 1: 用 `frame_now` 统一采样文字 progress。
    ///
    /// 替代原来的 `build_text_animation_plan()`（内部各自 `Instant::now()`），
    /// 所有 transaction 的 progress 都从同一个 `frame_now` 计算。
    fn build_text_animation_plan_with_time(
        &mut self,
        frame_now: Instant,
    ) -> (TextAnimationPlan, Vec<VisualTransactionKey>) {
        let mut glyphs = Vec::new();
        let mut keys_to_complete = Vec::new();

        for tx in self.prepared_queue.active_transactions_mut() {
            if tx.state == TextVisualTransactionState::Cancelled
                || tx.state == TextVisualTransactionState::Completed
            {
                continue;
            }

            if tx.state == TextVisualTransactionState::Pending {
                continue;
            }

            if tx.state == TextVisualTransactionState::Prepared {
                tx.state = TextVisualTransactionState::Rendering;
                if !tx.timeline.is_started() {
                    tx.timeline.mark_first_frame();
                }
            }

            let progress = tx.progress(frame_now);

            if progress >= 1.0 {
                keys_to_complete.push(tx.key);
                continue;
            }

            for slice in &tx.slices {
                let frame = slice.compute_frame(progress);
                glyphs.push(TextAnimationGlyphInfo {
                    x: frame.x,
                    y: frame.y,
                    w: frame.w,
                    h: frame.h,
                    opacity: frame.opacity,
                    snapshot_id: frame.snapshot_id,
                    source_rect: frame.source_rect,
                });
            }
        }

        (TextAnimationPlan { glyphs }, keys_to_complete)
    }

    /// Issue #690 评论 5675007226 步骤 2: 从当前正文事务计算协同光标位置。
    ///
    /// 返回 `(progress, old_cursor_rect, new_cursor_rect)` 供 `build_render_plan_full`
    /// 用同一 ease 函数计算光标中间位置。光标和文字共用同一个 progress，
    /// 不再有 GUI 线程 tick 和 Scene Graph 渲染帧的采样偏差。
    fn compute_coordinated_cursor(
        &self,
        frame_now: Instant,
    ) -> Option<(f64, CursorRect, CursorRect)> {
        let key = self.active_text_transaction_key()?;
        let tx = self
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key)?;

        let old_rect = tx.old_cursor_rect.clone()?;
        let new_rect = tx.new_cursor_rect.clone()?;

        match tx.state {
            TextVisualTransactionState::Rendering | TextVisualTransactionState::Paused => {
                let progress = tx.progress(frame_now);
                Some((progress, old_rect, new_rect))
            }
            _ => None,
        }
    }

    /// Issue #690 评论 5675007226 步骤 1: 用 `frame_now` 取样 cursor timeline。
    ///
    /// 替代原来的 `cursor_timeline_sample()`（内部 `Instant::now()`），
    /// 确保光标 progress 和文字 progress 来自同一个帧采样时间点。
    pub(crate) fn cursor_timeline_sample_with_time(
        &self,
        key: VisualTransactionKey,
        frame_now: Instant,
    ) -> Option<CursorTimelineSample> {
        let tx = self
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|tx| tx.key == key)?;

        match tx.state {
            TextVisualTransactionState::Pending | TextVisualTransactionState::Prepared => {
                Some(CursorTimelineSample::Waiting)
            }
            TextVisualTransactionState::Rendering | TextVisualTransactionState::Paused => Some(
                CursorTimelineSample::Running(tx.progress(frame_now).clamp(0.0, 1.0)),
            ),
            TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled => None,
        }
    }

    /// 返回当前最新正文编辑事务的操作类型，用于决定光标 blink mode。
    fn active_operation_kind(&self) -> Option<TextVisualOperationKind> {
        self.prepared_queue
            .active_transactions()
            .iter()
            .rev()
            .find(|t| {
                !matches!(
                    t.state,
                    TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
                ) && t.operation_kind != TextVisualOperationKind::Cursor
            })
            .map(|t| t.operation_kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sujian_editor_item::animated_slice::AnimatedSliceKind;
    use writer_core::editor::Utf8ByteOffset;

    #[test]
    fn test_coordinator_suppress_all() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        assert!(!coord.has_active_insert());
        let suppressed = coord.suppress_all();
        assert!(!suppressed);
    }

    #[test]
    fn test_coordinator_finish_by_key() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = VisualTransactionKey::new(1, 1);
        let removed = coord.finish_by_key(key);
        assert!(removed.is_none());
    }

    #[test]
    fn test_animation_mode_system_suppressed_no_transaction() {
        let mode = AnimationMode::SystemSuppressed;
        assert!(!mode.should_create_transaction());
    }

    #[test]
    fn test_animation_mode_glyph_creates_transaction() {
        let mode = AnimationMode::GlyphAnimation;
        assert!(mode.should_create_transaction());
    }

    #[test]
    fn test_rebase_uses_offset_map_and_shaping_identity() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font_a".to_string(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 3,
            direction_rtl: false,
            format_fingerprint: 100,
        };
        let sid_b = ShapingIdentity {
            text_content_hash: 2,
            raw_font_fingerprint: "font_b".to_string(),
            glyph_indexes_hash: 20,
            cluster_glyph_count: 2,
            direction_rtl: false,
            format_fingerprint: 200,
        };

        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> = vec![
            (10, 20, 100.0, 200.0, 0.5, Some(sid_a.clone()), 0.0),
            (30, 40, 150.0, 250.0, 0.7, Some(sid_b.clone()), 0.0),
        ];

        let mut slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                50,
                60,
                Some(sid_a.clone()),
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                70,
                80,
                Some(sid_b.clone()),
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        match_rebase_frames(&rebase_frames, &mut slices, &offset_map);

        assert!((slices[0].from_document_rect.x - 0.0).abs() < 0.01);
        assert!((slices[1].from_document_rect.x - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_rebase_tier3_closest_position_match_with_duplicate_shaping() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> =
            vec![(10, 30, 10.0, 100.0, 0.3, Some(sid_dup.clone()), 0.3)];

        let mut slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                18,
                22,
                Some(sid_dup.clone()),
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        match_rebase_frames(&rebase_frames, &mut slices, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (18 + 22) as i64 / 2;
        let dist_0 = (center_0 - mapped_center).abs();
        let dist_1 = (center_1 - mapped_center).abs();
        assert!(
            dist_1 < dist_0,
            "test setup: slice 1 (dist={}) should be closer than slice 0 (dist={})",
            dist_1,
            dist_0
        );
        assert!(
            (slices[1].start_fraction - 0.3).abs() < 0.01,
            "slice 1 (center={}, abs dist={}) should match rebase frame, got start_fraction={}",
            center_1,
            dist_1,
            slices[1].start_fraction
        );
        assert!(
            (slices[0].start_fraction - 0.0).abs() < 0.01,
            "slice 0 (center={}, abs dist={}) should NOT be matched, got start_fraction={}",
            center_0,
            dist_0,
            slices[0].start_fraction
        );
    }

    #[test]
    fn test_rebase_tier3_absolute_distance_not_signed() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> =
            vec![(10, 30, 10.0, 100.0, 0.3, Some(sid_dup.clone()), 0.3)];

        let mut slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                22,
                26,
                Some(sid_dup.clone()),
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        match_rebase_frames(&rebase_frames, &mut slices, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (22 + 26) as i64 / 2;
        let signed_0 = center_0 - mapped_center;
        let signed_1 = center_1 - mapped_center;
        let abs_0 = (center_0 - mapped_center).abs();
        let abs_1 = (center_1 - mapped_center).abs();
        assert!(
            signed_0 < signed_1,
            "test setup: slice 0 signed diff ({}) should be more negative than slice 1 ({})",
            signed_0,
            signed_1
        );
        assert!(
            abs_1 < abs_0,
            "test setup: slice 1 abs dist ({}) should be less than slice 0 ({})",
            abs_1,
            abs_0
        );
        assert!((slices[1].start_fraction - 0.3).abs() < 0.01,
            "slice 1 (abs dist={}) should be chosen over slice 0 (abs dist={}, signed={}), got start_fraction={}",
            abs_1, abs_0, signed_0, slices[1].start_fraction);
    }

    #[test]
    fn test_rebase_tier1_consumed_prevents_reuse() {
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font_a".to_string(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 3,
            direction_rtl: false,
            format_fingerprint: 100,
        };

        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> = vec![
            (50, 60, 10.0, 100.0, 0.3, Some(sid_a.clone()), 0.3),
            (50, 60, 20.0, 200.0, 0.5, Some(sid_a.clone()), 0.5),
        ];

        let mut slices = vec![AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            0.0,
            0.0,
            50,
            60,
            Some(sid_a.clone()),
        )];

        let offset_map = OffsetMap {
            entries: Vec::new(),
        };
        match_rebase_frames(&rebase_frames, &mut slices, &offset_map);

        assert!(
            (slices[0].start_fraction - 0.3).abs() < 0.01,
            "slice 0 should get first rebase frame start_fraction 0.3 (not second 0.5), got {}",
            slices[0].start_fraction
        );
    }

    #[test]
    fn test_rebase_tier3_consumed_prevents_reuse() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> = vec![
            (10, 30, 10.0, 100.0, 0.3, Some(sid_dup.clone()), 0.3),
            (10, 30, 20.0, 200.0, 0.5, Some(sid_dup.clone()), 0.5),
        ];

        let mut slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                18,
                22,
                Some(sid_dup.clone()),
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        match_rebase_frames(&rebase_frames, &mut slices, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (18 + 22) as i64 / 2;
        let dist_0 = (center_0 - mapped_center).abs();
        let dist_1 = (center_1 - mapped_center).abs();
        assert!(dist_1 < dist_0, "test setup: slice 1 should be closer");
        assert!(
            (slices[1].start_fraction - 0.3).abs() < 0.01,
            "slice 1 should get first rebase frame (start_fraction=0.3), got start_fraction={}",
            slices[1].start_fraction
        );
        assert!(
            (slices[0].start_fraction - 0.5).abs() < 0.01,
            "slice 0 should get second rebase frame (start_fraction=0.5), not reuse slice 1's frame, got start_fraction={}",
            slices[0].start_fraction
        );
    }

    #[test]
    fn test_rebase_tier2_consumed_prevents_reuse() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font_a".to_string(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 3,
            direction_rtl: false,
            format_fingerprint: 100,
        };

        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> = vec![
            (50, 70, 10.0, 100.0, 0.3, Some(sid_a.clone()), 0.3),
            (50, 70, 20.0, 200.0, 0.5, Some(sid_a.clone()), 0.5),
        ];

        let mut slices = vec![AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            0.0,
            0.0,
            150,
            170,
            Some(sid_a.clone()),
        )];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::unchecked(100),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        match_rebase_frames(&rebase_frames, &mut slices, &offset_map);

        assert!(
            (slices[0].start_fraction - 0.3).abs() < 0.01,
            "slice 0 should get first rebase frame via tier2 (start_fraction=0.3), got start_fraction={}",
            slices[0].start_fraction
        );
    }

    #[test]
    fn test_rebase_tier1_consumed_prevents_tier3_reuse() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> = vec![
            (50, 60, 10.0, 100.0, 0.3, Some(sid_dup.clone()), 0.3),
            (40, 80, 20.0, 200.0, 0.5, Some(sid_dup.clone()), 0.5),
        ];

        let mut slices = vec![AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            0.0,
            0.0,
            50,
            60,
            Some(sid_dup.clone()),
        )];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        match_rebase_frames(&rebase_frames, &mut slices, &offset_map);

        assert!(
            (slices[0].start_fraction - 0.3).abs() < 0.01,
            "slice 0 should get first rebase frame start_fraction 0.3 (not second 0.5), got {}",
            slices[0].start_fraction
        );
    }

    #[test]
    fn test_rebase_tier3_tiebreak_by_byte_start_then_index() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>, f64)> =
            vec![(10, 30, 10.0, 100.0, 0.3, Some(sid_dup.clone()), 0.3)];

        let mut slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                25,
                29,
                Some(sid_dup.clone()),
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        match_rebase_frames(&rebase_frames, &mut slices, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (25 + 29) as i64 / 2;
        let dist_0 = (center_0 - mapped_center).abs();
        let dist_1 = (center_1 - mapped_center).abs();
        assert_eq!(
            dist_0, dist_1,
            "test setup: both slices should have equal distance"
        );
        assert!(
            (slices[0].start_fraction - 0.3).abs() < 0.01,
            "slice 0 (lower byte_start) should win tiebreak, got start_fraction={}",
            slices[0].start_fraction
        );
        assert!(
            (slices[1].start_fraction - 0.0).abs() < 0.01,
            "slice 1 should not be matched, got start_fraction={}",
            slices[1].start_fraction
        );
    }

    fn make_test_snapshot(
        virtual_text: &str,
        line_clusters: Vec<(usize, usize, f64, f64, ShapingIdentity)>,
    ) -> EditorLayoutSnapshot {
        use crate::editor::layout::{CaretAffinity, LayoutSnapshot, VisualLine};
        use crate::sujian_editor_item::layout_snapshot::{
            LineClusterSnapshot, PreparedLineSnapshot,
        };
        let clusters: Vec<LineClusterSnapshot> = line_clusters
            .iter()
            .map(|(bs, be, x, _y, sid)| LineClusterSnapshot {
                byte_start: *bs,
                byte_end: *be,
                source_rect: SourceRect {
                    x: *x,
                    y: 0.0,
                    w: (*be - *bs) as f64 * 10.0,
                    h: 20.0,
                },
                shaping_identity: sid.clone(),
            })
            .collect();
        let line = PreparedLineSnapshot {
            id: LineSnapshotId::new(1, 0, 0),
            image: None,
            clusters,
            document_origin_y: 0.0,
            dpr: 1.0,
            byte_start: line_clusters.first().map(|c| c.0).unwrap_or(0),
            byte_end: line_clusters.last().map(|c| c.1).unwrap_or(0),
            visual_x: 0.0,
        };
        let layout_snapshot = LayoutSnapshot {
            text_revision: 0,
            text_ptr: 0,
            text_len: virtual_text.len(),
            width: 800.0,
            font_size: 16.0,
            font_family: "sans-serif".to_string(),
            line_spacing: 1.5,
            text_indent: 0.0,
            padding: 0.0,
            lines: vec![VisualLine {
                id: 0,
                byte_start: line.byte_start,
                byte_end: line.byte_end,
                qchar_start: 0,
                qchar_end: 0,
                hard_break: false,
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 20.0,
                para_text: virtual_text.to_string(),
                para_start: 0,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 0,
                line_wrap_width: 800.0,
                line_indent_x: 0.0,
                para_indent: 0.0,
                x_end_trailing: 800.0,
                qt_ascent: 16.0,
                qt_descent: 4.0,
                cache_slot: 0,
            }],
            layout_generation: 0,
        };
        EditorLayoutSnapshot::new(layout_snapshot, vec![line], None, CaretAffinity::Downstream)
            .with_virtual_text(virtual_text.to_string())
    }

    #[test]
    fn test_commit_same_shaping_different_geometry_creates_move() {
        let sid_common = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_old_preedit = ShapingIdentity {
            text_content_hash: 10,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 200,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_new_commit = ShapingIdentity {
            text_content_hash: 20,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 300,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot(
            "世界好abc",
            vec![
                (0, 3, 10.0, 0.0, sid_common.clone()),
                (3, 6, 40.0, 0.0, sid_common.clone()),
                (6, 9, 70.0, 0.0, sid_common.clone()),
                (9, 12, 100.0, 0.0, sid_old_preedit.clone()),
            ],
        );
        let new_snapshot = make_test_snapshot(
            "世界好xyz",
            vec![
                (0, 3, 50.0, 0.0, sid_common.clone()),
                (3, 6, 80.0, 0.0, sid_common.clone()),
                (6, 9, 110.0, 0.0, sid_common.clone()),
                (9, 12, 140.0, 0.0, sid_new_commit.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_commit_or_cancel(
            &old_snapshot,
            &new_snapshot,
            0,
            12,
            true,
            false,
            0,
            12,
            0,
            12,
            None,
            None,
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        let has_move = tx
            .slices
            .iter()
            .any(|s| s.kind == AnimatedSliceKind::ReflowMove);
        assert!(
            has_move,
            "commit with same shaping but different geometry should create ReflowMove slice"
        );
        assert!(
            !tx.static_patches.is_empty(),
            "Move slices should have corresponding StaticLinePatch::insert_patch"
        );
    }

    #[test]
    fn test_commit_different_shaping_creates_crossfade_with_static_patch() {
        let sid_common = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_common_diff = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font_other".into(),
            glyph_indexes_hash: 999,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_old_preedit = ShapingIdentity {
            text_content_hash: 10,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 200,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_new_commit = ShapingIdentity {
            text_content_hash: 20,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 300,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot(
            "世界好abc",
            vec![
                (0, 3, 10.0, 0.0, sid_common.clone()),
                (3, 6, 40.0, 0.0, sid_common.clone()),
                (6, 9, 70.0, 0.0, sid_common.clone()),
                (9, 12, 100.0, 0.0, sid_old_preedit.clone()),
            ],
        );
        let new_snapshot = make_test_snapshot(
            "世界好xyz",
            vec![
                (0, 3, 10.0, 0.0, sid_common.clone()),
                (3, 6, 40.0, 0.0, sid_common_diff.clone()),
                (6, 9, 70.0, 0.0, sid_common.clone()),
                (9, 12, 140.0, 0.0, sid_new_commit.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_commit_or_cancel(
            &old_snapshot,
            &new_snapshot,
            0,
            12,
            true,
            false,
            0,
            12,
            0,
            12,
            None,
            None,
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        let crossfade_count = tx
            .slices
            .iter()
            .filter(|s| s.kind == AnimatedSliceKind::ReflowCrossFade)
            .count();
        assert!(
            crossfade_count >= 2,
            "commit with different shaping should create paired Crossfade slices (old+new), got {}",
            crossfade_count
        );
        assert!(
            !tx.static_patches.is_empty(),
            "Crossfade new should have StaticLinePatch::insert_patch to prevent double-draw"
        );
    }

    #[test]
    fn test_commit_same_shaping_same_geometry_is_static() {
        let sid_common = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_old_preedit = ShapingIdentity {
            text_content_hash: 10,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 200,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_new_commit = ShapingIdentity {
            text_content_hash: 20,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 300,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot(
            "世界好abc",
            vec![
                (0, 3, 10.0, 0.0, sid_common.clone()),
                (3, 6, 40.0, 0.0, sid_common.clone()),
                (6, 9, 70.0, 0.0, sid_common.clone()),
                (9, 12, 100.0, 0.0, sid_old_preedit.clone()),
            ],
        );
        let new_snapshot = make_test_snapshot(
            "世界好xyz",
            vec![
                (0, 3, 10.0, 0.0, sid_common.clone()),
                (3, 6, 40.0, 0.0, sid_common.clone()),
                (6, 9, 70.0, 0.0, sid_common.clone()),
                (9, 12, 100.0, 0.0, sid_new_commit.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_commit_or_cancel(
            &old_snapshot,
            &new_snapshot,
            0,
            12,
            true,
            false,
            0,
            12,
            0,
            12,
            None,
            None,
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        let first_cluster_slices: Vec<&AnimatedSlice> = tx
            .slices
            .iter()
            .filter(|s| s.byte_start == 0 && s.byte_end == 3)
            .collect();
        assert!(
            first_cluster_slices.is_empty(),
            "same shaping + same geometry should be Static (no slice), got {} slices",
            first_cluster_slices.len()
        );
    }

    #[test]
    fn test_commit_separate_preedit_and_committed_replace_ranges() {
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_b = ShapingIdentity {
            text_content_hash: 2,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 20,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_c = ShapingIdentity {
            text_content_hash: 3,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 30,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_preedit = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 99,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot(
            "abc_preedit_xyz",
            vec![
                (0, 3, 10.0, 0.0, sid_a.clone()),
                (3, 10, 50.0, 0.0, sid_preedit.clone()),
                (10, 13, 120.0, 0.0, sid_c.clone()),
            ],
        );
        let new_snapshot = make_test_snapshot(
            "abc_QQ_xyz",
            vec![
                (0, 3, 10.0, 0.0, sid_a.clone()),
                (3, 5, 50.0, 0.0, sid_b.clone()),
                (5, 8, 120.0, 0.0, sid_c.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_commit_or_cancel(
            &old_snapshot,
            &new_snapshot,
            3,
            10,
            true,
            false,
            3,
            5,
            3,
            10,
            None,
            None,
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        let old_preedit_slices: Vec<&AnimatedSlice> = tx
            .slices
            .iter()
            .filter(|s| s.byte_start >= 3 && s.byte_end <= 10)
            .collect();
        assert!(
            !old_preedit_slices.is_empty(),
            "preedit range should have animated slices"
        );
        let new_candidate_slices: Vec<&AnimatedSlice> = tx
            .slices
            .iter()
            .filter(|s| s.byte_start >= 3 && s.byte_end <= 5)
            .collect();
        assert!(
            !new_candidate_slices.is_empty(),
            "candidate range should have animated slices"
        );
    }

    #[test]
    fn test_commit_cancel_uses_preedit_range_for_old_clusters() {
        let sid_preedit = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 99,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_after = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot(
            "abc_preedit_after",
            vec![
                (0, 3, 10.0, 0.0, sid_after.clone()),
                (3, 10, 50.0, 0.0, sid_preedit.clone()),
                (10, 15, 120.0, 0.0, sid_after.clone()),
            ],
        );
        let new_snapshot = make_test_snapshot(
            "abc_after",
            vec![
                (0, 3, 10.0, 0.0, sid_after.clone()),
                (3, 8, 120.0, 0.0, sid_after.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_commit_or_cancel(
            &old_snapshot,
            &new_snapshot,
            3,
            10,
            false,
            false,
            3,
            3,
            3,
            3,
            None,
            None,
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        let delete_slices: Vec<&AnimatedSlice> = tx
            .slices
            .iter()
            .filter(|s| s.kind == AnimatedSliceKind::DeleteConceal)
            .collect();
        assert!(
            !delete_slices.is_empty(),
            "cancel should create DeleteConceal for preedit range"
        );
    }

    #[test]
    fn test_many_to_one_reflow_one_old_splits_to_two_new() {
        // Issue #658 评论 5630181473 问题 3: one old cluster [0,3) maps to
        // two new clusters [0,1) + [4,6) via OffsetMap (insert "XYZ" at position 1).
        // new cluster [1,4) is inserted text with no old counterpart → InsertReveal.
        // old cluster [0,3) connects to both [0,1) and [4,6) → N→M crossfade run.
        let sid_common = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_preedit = ShapingIdentity {
            text_content_hash: 10,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 200,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_new_a = ShapingIdentity {
            text_content_hash: 50,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 300,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_new_b = ShapingIdentity {
            text_content_hash: 51,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 301,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        // old: "abc" → one cluster covering [0,3)
        let old_snapshot = make_test_snapshot("abc", vec![(0, 3, 10.0, 0.0, sid_preedit.clone())]);
        // new: "aXYZbc" → three clusters: [0,1) "a", [1,4) "XYZ", [4,6) "bc"
        let new_snapshot = make_test_snapshot(
            "aXYZbc",
            vec![
                (0, 1, 10.0, 0.0, sid_new_a.clone()),
                (1, 4, 20.0, 0.0, sid_common.clone()),
                (4, 6, 40.0, 0.0, sid_new_b.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_update(&old_snapshot, &new_snapshot, 0, 3, None, None);
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        // N→M run: 1 old [0,3) + 2 new [0,1),[4,6) → crossfade slices
        // old cluster [0,3) produces crossfade_old (uses first new's byte range [0,1))
        let crossfade_count = tx
            .slices
            .iter()
            .filter(|s| s.kind == AnimatedSliceKind::ReflowCrossFade)
            .count();
        // 1 crossfade_old (old [0,3)) + 2 crossfade_new (new [0,1) + new [4,6)) = 3
        assert_eq!(
            crossfade_count, 3,
            "expected 3 crossfade slices (1 old + 2 new), got {}",
            crossfade_count
        );
        // new cluster [1,4) is inserted text (no old counterpart) → InsertReveal
        let insert_count = tx
            .slices
            .iter()
            .filter(|s| s.kind == AnimatedSliceKind::InsertReveal)
            .filter(|s| s.byte_start == 1 && s.byte_end == 4)
            .count();
        assert_eq!(
            insert_count, 1,
            "inserted cluster [1,4) should produce exactly 1 InsertReveal, got {}",
            insert_count
        );
        // No DeleteConceal for these ranges
        let delete_count = tx
            .slices
            .iter()
            .filter(|s| s.kind == AnimatedSliceKind::DeleteConceal)
            .count();
        assert_eq!(delete_count, 0, "should not produce DeleteConceal");
    }

    /// Issue #686 评论 5667184642：回归测试——吞字方向必须与光标位置匹配。
    ///
    /// `conceal_from_left = true` 表示保留左段（Backspace，光标在文字右侧）；
    /// `conceal_from_left = false` 表示保留右段（Delete 键，光标在文字左侧）。
    /// 上一轮把比较式写反了（靠左算成 true），这里锁定正确语义。
    ///
    /// 测试布局：old cluster [0,3) source_rect x=10 w=30，dpr=1 visual_x=0
    /// → document rect x=10 w=30 → left=10, right=40。
    fn make_delete_direction_snapshots() -> (EditorLayoutSnapshot, EditorLayoutSnapshot, OffsetMap)
    {
        let sid = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot("abc", vec![(0, 3, 10.0, 0.0, sid)]);
        // new 为空 → old cluster 成为纯 old run → delete_conceal
        let new_snapshot = make_test_snapshot("", vec![]);
        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
        (old_snapshot, new_snapshot, offset_map)
    }

    #[test]
    fn test_delete_conceal_direction_cursor_near_right_is_backspace() {
        let (old_snapshot, _new_snapshot, _offset_map) = make_delete_direction_snapshots();
        let key = VisualTransactionKey::new(1, 1);
        // 新光标靠近右端 (x=39, right=40) → Backspace → conceal_from_left=true
        let new_cursor = CursorRect {
            x: 39.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        };
        // Issue #687: changed range 由 build_delete_conceal_slices 显式拥有。
        // old cluster [0,3) 是被删除的范围。
        let slices = build_delete_conceal_slices(
            key,
            &old_snapshot,
            (0, 3),
            Some(&new_cursor),
        );
        let delete_slices: Vec<_> = slices
            .iter()
            .filter(|s| s.kind == AnimatedSliceKind::DeleteConceal)
            .collect();
        assert_eq!(
            delete_slices.len(),
            1,
            "deleted range [0,3) should produce exactly one DeleteConceal"
        );
        assert!(
            delete_slices[0].conceal_from_left,
            "cursor near right (x=39, right=40) should be Backspace → conceal_from_left=true"
        );
    }

    #[test]
    fn test_delete_conceal_direction_cursor_near_left_is_delete() {
        let (old_snapshot, _new_snapshot, _offset_map) = make_delete_direction_snapshots();
        let key = VisualTransactionKey::new(1, 1);
        // 新光标靠近左端 (x=11, left=10) → Delete 键 → conceal_from_left=false
        let new_cursor = CursorRect {
            x: 11.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        };
        // Issue #687: changed range 由 build_delete_conceal_slices 显式拥有。
        // old cluster [0,3) 是被删除的范围。
        let slices = build_delete_conceal_slices(
            key,
            &old_snapshot,
            (0, 3),
            Some(&new_cursor),
        );
        let delete_slices: Vec<_> = slices
            .iter()
            .filter(|s| s.kind == AnimatedSliceKind::DeleteConceal)
            .collect();
        assert_eq!(
            delete_slices.len(),
            1,
            "deleted range [0,3) should produce exactly one DeleteConceal"
        );
        assert!(
            !delete_slices[0].conceal_from_left,
            "cursor near left (x=11, left=10) should be Delete → conceal_from_left=false"
        );
    }
}
