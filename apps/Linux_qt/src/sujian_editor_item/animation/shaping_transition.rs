//! Issue #826 评论 24：不可拆 shaping cluster 的原子视觉交接。
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
//!   `2..3` 留给 scalar Reveal —— **同一块视觉 cluster 被两个 owner 同时控制**。
//!
//! ## 本层的职责
//!
//! 从「逻辑 changed range」派生「视觉 affected cluster」层：
//!
//! - 改动**完整覆盖**的 cluster：照旧交给 `EditFrontier` 的 Reveal / Conceal
//!   （绝大多数中文单字、独立 glyph 都属于这一类，观感完全不变）；
//! - 改动只覆盖 cluster 一部分的 **mixed cluster**：整块退出 EditFrontier，
//!   作为一对 old/new cluster 的 [`ShapingTransitionSpan`] 进入本层。
//!
//! 本层只保存**当前这一笔**的 old/new cluster 对，共用当前 progress：
//! old cluster 的 opacity `1 -> 0`、new cluster 的 opacity `0 -> 1`，位置按需要补间。
//! 它不排历史队列、不 per-key 累积、没有第二个动画时钟 —— 与 `ReflowState` 同构。
//!
//! 架构归属关系：`EditFrontier` 管 changed logical fact + mask timing；`Reflow` 管
//! same-shaping unchanged move；本层只管当前这一笔不可拆 cluster 的 old/new 视觉交接。
//! 三者都只保存「当前屏幕事实」。

use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::animation::edit_frontier::{ease_out_cubic, ConcealSourceLine};
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, LineSnapshotId, PreparedLineSnapshot, SourceRect,
};

/// Issue #826 评论 24：一个 mixed visual cluster 的 old/new 原子对。
///
/// 这是**视觉 cluster 对**，不是逻辑 range 对：`old_cluster` / `new_cluster` 都是
/// Qt shaping 给出的完整 cluster 边界，两个坐标系里各自的 cluster 边界可能完全不同
/// （`f 1..2` -> `fi 1..3`）。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShapingTransitionSpan {
    /// 旧正文坐标系里那一整块被替换掉的视觉 cluster。
    pub old_cluster: (usize, usize),
    /// 最新正文坐标系里成形的那一整块视觉 cluster。
    pub new_cluster: (usize, usize),
    /// 旧 cluster 的行纹理。
    pub old_snapshot_id: LineSnapshotId,
    /// 新 cluster 的行纹理。
    pub new_snapshot_id: LineSnapshotId,
    /// 旧行纹理里的源矩形。
    pub old_source_rect: SourceRect,
    /// 新行纹理里的源矩形。
    pub new_source_rect: SourceRect,
    /// 旧位置（文档坐标）。
    pub old_rect: SourceRect,
    /// 新位置（文档坐标）。
    pub new_rect: SourceRect,
}

/// 交接过程中某一侧（old 或 new）的一帧画面。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShapingTransitionSide {
    pub snapshot_id: LineSnapshotId,
    pub source_rect: SourceRect,
    pub rect: SourceRect,
    /// `old` 侧 `1 -> 0`，`new` 侧 `0 -> 1`。
    pub opacity: f64,
}

/// 一个 span 在某一帧的画面。某一侧 opacity 归零时不再画它。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShapingTransitionFrame {
    pub old: Option<ShapingTransitionSide>,
    pub new: Option<ShapingTransitionSide>,
}

/// Issue #826 评论 24：不可拆 cluster 的当前态交接层。
///
/// 与 [`crate::sujian_editor_item::animation::reflow_motion::ReflowState`] 同构：
/// 只有当前这一笔的 cluster 对、共用一个 `progress`，没有历史队列。
#[derive(Clone, Debug, Default)]
pub(crate) struct ShapingTransitionState {
    pub spans: Vec<ShapingTransitionSpan>,
    pub started_at: Option<Instant>,
    pub duration_ms: u64,
    /// 旧侧 cluster 真正引用的行图（画旧 cluster 必须有这张图）。
    pub old_sources: Vec<ConcealSourceLine>,
    /// 被本层整块占用的**旧坐标** cluster 范围。
    ///
    /// `EditFrontier` 的吞字侧必须把它们排除：普通 Conceal 绝不能声称
    /// `owner = 2..3` 却拿整块 old `fi` 的 glyph 来吞。
    pub owned_old_clusters: Vec<(usize, usize)>,
    /// 被本层整块占用的**新坐标** cluster 范围。
    ///
    /// `EditFrontier` 的吐字侧（scalar region、carry、settled）必须把它们排除：
    /// 否则同一块视觉 cluster 会同时被 carry 与 scalar Reveal 控制。
    pub owned_new_clusters: Vec<(usize, usize)>,
}

impl ShapingTransitionState {
    /// 从「逻辑 changed range」派生当前这一笔的 mixed visual cluster 对。
    pub(crate) fn build(
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        deleted_ranges: &[(usize, usize)],
        inserted_ranges: &[(usize, usize)],
        old_to_new: &OffsetMap,
        now: Instant,
        duration_ms: u64,
    ) -> Self {
        let mut spans: Vec<ShapingTransitionSpan> = Vec::new();
        let mut old_sources: Vec<ConcealSourceLine> = Vec::new();

        // 旧侧：被部分删除的 cluster（`fi` 删掉 `i` -> `f`）。
        for old_line in &old_snapshot.line_snapshots {
            for old_cluster in &old_line.clusters {
                if old_cluster.byte_start >= old_cluster.byte_end {
                    continue;
                }
                let cluster = (old_cluster.byte_start, old_cluster.byte_end);
                let Some(deleted) = deleted_ranges
                    .iter()
                    .copied()
                    .find(|range| is_mixed(cluster, *range))
                else {
                    continue;
                };
                // cluster 里**没被删掉**的那一段：它才是这次改动在旧正文里的
                // 立足点，用它跨 OffsetMap 找回新正文里的对应 cluster。
                let Some(footprint_old) = untouched_part(cluster, deleted) else {
                    continue;
                };
                let Some(footprint_new) =
                    old_to_new.map_old_range_to_new(footprint_old.0, footprint_old.1)
                else {
                    continue;
                };
                let Some((new_line, new_cluster)) =
                    find_overlapping_cluster(new_snapshot, footprint_new)
                else {
                    continue;
                };
                if !old_sources
                    .iter()
                    .any(|source| source.snapshot_id == old_line.id)
                {
                    old_sources.push(ConcealSourceLine {
                        snapshot_id: old_line.id,
                        image: old_line.image.clone(),
                    });
                }
                spans.push(ShapingTransitionSpan {
                    old_cluster: cluster,
                    new_cluster: (new_cluster.byte_start, new_cluster.byte_end),
                    old_snapshot_id: old_line.id,
                    new_snapshot_id: new_line.id,
                    old_source_rect: old_cluster.source_rect.clone(),
                    new_source_rect: new_cluster.source_rect.clone(),
                    old_rect: old_line.source_rect_to_document_rect(&old_cluster.source_rect),
                    new_rect: new_line.source_rect_to_document_rect(&new_cluster.source_rect),
                });
            }
        }

        // 新侧：被部分插入的 cluster（`f` 旁边插 `i` -> 最新 shaping 得到 `fi`）。
        for new_line in &new_snapshot.line_snapshots {
            for new_cluster in &new_line.clusters {
                if new_cluster.byte_start >= new_cluster.byte_end {
                    continue;
                }
                let cluster = (new_cluster.byte_start, new_cluster.byte_end);
                let Some(inserted) = inserted_ranges
                    .iter()
                    .copied()
                    .find(|range| is_mixed(cluster, *range))
                else {
                    continue;
                };
                let Some(footprint_new) = untouched_part(cluster, inserted) else {
                    continue;
                };
                let Some(footprint_old) =
                    old_to_new.map_new_range_to_old(footprint_new.0, footprint_new.1)
                else {
                    continue;
                };
                let Some((old_line, old_cluster)) =
                    find_overlapping_cluster(old_snapshot, footprint_old)
                else {
                    continue;
                };
                if !old_sources
                    .iter()
                    .any(|source| source.snapshot_id == old_line.id)
                {
                    old_sources.push(ConcealSourceLine {
                        snapshot_id: old_line.id,
                        image: old_line.image.clone(),
                    });
                }
                let span = ShapingTransitionSpan {
                    old_cluster: (old_cluster.byte_start, old_cluster.byte_end),
                    new_cluster: cluster,
                    old_snapshot_id: old_line.id,
                    new_snapshot_id: new_line.id,
                    old_source_rect: old_cluster.source_rect.clone(),
                    new_source_rect: new_cluster.source_rect.clone(),
                    old_rect: old_line.source_rect_to_document_rect(&old_cluster.source_rect),
                    new_rect: new_line.source_rect_to_document_rect(&new_cluster.source_rect),
                };
                // 同一对 cluster 已经在旧侧那轮登记过就不再重复。
                if !spans.iter().any(|existing| {
                    existing.old_cluster == span.old_cluster
                        && existing.new_cluster == span.new_cluster
                }) {
                    spans.push(span);
                }
            }
        }

        let owned_old_clusters = spans.iter().map(|span| span.old_cluster).collect();
        let owned_new_clusters = spans.iter().map(|span| span.new_cluster).collect();
        Self {
            spans,
            started_at: Some(now),
            duration_ms: duration_ms.max(1),
            old_sources,
            owned_old_clusters,
            owned_new_clusters,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    pub(crate) fn is_finished(&self, now: Instant) -> bool {
        let Some(started_at) = self.started_at else {
            return true;
        };
        if self.spans.is_empty() {
            return true;
        }
        let elapsed_ms = now.saturating_duration_since(started_at).as_millis() as f64;
        self.duration_ms == 0 || elapsed_ms >= self.duration_ms as f64
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
        self.spans
            .iter()
            .map(|span| {
                let lerp = |from: f64, to: f64| from + (to - from) * t;
                let rect = SourceRect {
                    x: lerp(span.old_rect.x, span.new_rect.x),
                    y: lerp(span.old_rect.y, span.new_rect.y),
                    w: lerp(span.old_rect.w, span.new_rect.w),
                    h: lerp(span.old_rect.h, span.new_rect.h),
                };
                let old_opacity = 1.0 - t;
                let new_opacity = t;
                ShapingTransitionFrame {
                    old: (old_opacity > 1e-6).then(|| ShapingTransitionSide {
                        snapshot_id: span.old_snapshot_id,
                        source_rect: span.old_source_rect.clone(),
                        rect: rect.clone(),
                        opacity: old_opacity,
                    }),
                    new: (new_opacity > 1e-6).then(|| ShapingTransitionSide {
                        snapshot_id: span.new_snapshot_id,
                        source_rect: span.new_source_rect.clone(),
                        rect: rect.clone(),
                        opacity: new_opacity,
                    }),
                }
            })
            .collect()
    }

    /// 本层真正引用到的行纹理 id（旧侧 + 新侧）。
    pub(crate) fn active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        for span in &self.spans {
            for id in [span.old_snapshot_id, span.new_snapshot_id] {
                if !ids.contains(&id) {
                    ids.push(id);
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
        self.spans
            .iter()
            .map(|span| (span.new_rect.clone(), span.new_snapshot_id))
            .collect()
    }

    /// Issue #826 评论 24：连续编辑时不要把同一次交接的淡入淡出重置回 0。
    ///
    /// 只有在**旧坐标 cluster 集合完全不变**（还是同一批 mixed cluster）时才沿用
    /// 上一次的 `started_at`：这时动画语义是「同一次交接继续走」，重置会让
    /// already-fading 的旧 cluster 突然闪回全不透明。
    ///
    /// cluster 集合变了就是另一笔交接，`started_at` 必须重设，否则新 cluster 会
    /// 以别的 cluster 的剩余进度出场。
    pub(crate) fn carry_over_progress_from(&mut self, previous: &ShapingTransitionState) {
        if previous.owned_old_clusters == self.owned_old_clusters {
            self.started_at = previous.started_at;
        }
    }

    /// 测试用：当前被本层整块占用的 cluster 对。
    #[cfg(test)]
    pub(crate) fn owned_clusters_for_test(&self) -> Vec<((usize, usize), (usize, usize))> {
        self.spans
            .iter()
            .map(|span| (span.old_cluster, span.new_cluster))
            .collect()
    }
}

/// 本轮逻辑改动是否只覆盖了 `cluster` 的一部分。
///
/// 「完整覆盖」或「完全不碰」都不算 mixed —— 前者照旧走 Reveal / Conceal，
/// 后者照旧走 Reflow。只有落在中间这一档才是不可拆的 mixed visual cluster。
fn is_mixed(cluster: (usize, usize), changed: (usize, usize)) -> bool {
    let overlaps = cluster.0 < changed.1 && changed.0 < cluster.1;
    let fully_covered = changed.0 <= cluster.0 && cluster.1 <= changed.1;
    overlaps && !fully_covered
}

/// cluster 去掉 `changed` 之后**仍然存在**的那一段（取第一个非空片段）。
fn untouched_part(cluster: (usize, usize), changed: (usize, usize)) -> Option<(usize, usize)> {
    let head = (cluster.0, changed.0.min(cluster.1));
    let tail = (changed.1.max(cluster.0), cluster.1);
    if head.1 > head.0 {
        return Some(head);
    }
    if tail.1 > tail.0 {
        return Some(tail);
    }
    None
}

fn find_overlapping_cluster(
    snapshot: &EditorLayoutSnapshot,
    range: (usize, usize),
) -> Option<(&PreparedLineSnapshot, &LineClusterSnapshot)> {
    if range.0 >= range.1 {
        return None;
    }
    for line in &snapshot.line_snapshots {
        for cluster in line.clusters_overlapping_range(range.0, range.1) {
            return Some((line, cluster));
        }
    }
    None
}

#[cfg(test)]
mod tests;
