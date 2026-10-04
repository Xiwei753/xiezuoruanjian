//! Issue #826 评论 2：独立的 Reflow 层。
//!
//! 单一遮罩前沿只负责「改掉的字」（Reveal / Conceal）。光标后方那些**没被改、
//! 但因为换行/插入/删除而移动了位置**的文字，必须走这一层。
//!
//! 本模块：
//! - 从 old / new `EditorLayoutSnapshot` + Core `OffsetMap` 匹配**同一段未改文字**；
//! - 只做 `old_rect -> new_rect` 的位置插值；
//! - 不创建 Reveal/Conceal，不进 EditFrontier；
//! - Changed range（真正新增/删除的字）必须排除，同一个 glyph 不允许同时进
//!   Reveal/Conceal 和 Reflow。
//!
//! 断行几何复用现有 Linux_Qt layout snapshot 的行/cluster 几何，不自己重写断行。

use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineSnapshotId, SourceRect,
};

/// Issue #826: 一段未改文字的旧位置 → 新位置。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ReflowSpan {
    /// 旧正文坐标系里的这段文字。
    pub old_range: (usize, usize),
    /// 最新正文坐标系里的同一段文字。
    pub new_range: (usize, usize),
    /// 旧位置（文档坐标）。
    pub old_rect: SourceRect,
    /// 新位置（文档坐标）。
    pub new_rect: SourceRect,
    /// 取哪张行纹理画这一段。
    pub snapshot_id: LineSnapshotId,
    /// 行内局部物理像素坐标（与 `snapshot_id` 对应的新行）。
    pub source_rect: SourceRect,
}

/// Issue #826: Reflow 层的唯一状态。
///
/// 与 EditFrontier 共享同一次 old/new layout，但状态互相独立：前沿不拥有
/// Reflow，Reflow 不拥有前沿。
#[derive(Clone, Debug)]
pub(crate) struct ReflowState {
    pub spans: Vec<ReflowSpan>,
    pub started_at: Instant,
    pub duration_ms: u64,
}

/// 一段未改文字在一帧里的位置。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ReflowSpanFrame {
    pub snapshot_id: LineSnapshotId,
    pub source_rect: SourceRect,
    /// 本帧文档坐标（old_rect → new_rect 插值结果）。
    pub dest_rect: SourceRect,
}

fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

impl ReflowState {
    /// 从 old/new layout + OffsetMap 建立 Reflow 层。
    ///
    /// `excluded_old` / `excluded_new` 是本轮**真正改掉**的范围（deleted / inserted）。
    /// 落在这些范围里的文字归 EditFrontier 管，这里必须排除。
    pub(crate) fn build(
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        offset_map: &OffsetMap,
        excluded_old: &[(usize, usize)],
        excluded_new: &[(usize, usize)],
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        let spans = collect_reflow_spans(
            old_snapshot,
            new_snapshot,
            offset_map,
            excluded_old,
            excluded_new,
        );
        Self {
            spans,
            started_at,
            duration_ms: duration_ms.max(1),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// 采样本帧每段未改文字的位置。
    pub(crate) fn sample(&self, now: Instant) -> Vec<ReflowSpanFrame> {
        let elapsed_ms = now.saturating_duration_since(self.started_at).as_millis() as f64;
        let progress = if self.duration_ms == 0 {
            1.0
        } else {
            (elapsed_ms / self.duration_ms as f64).clamp(0.0, 1.0)
        };
        let t = ease_out_cubic(progress);
        self.spans
            .iter()
            .map(|span| ReflowSpanFrame {
                snapshot_id: span.snapshot_id,
                source_rect: span.source_rect.clone(),
                dest_rect: SourceRect {
                    x: span.old_rect.x + (span.new_rect.x - span.old_rect.x) * t,
                    y: span.old_rect.y + (span.new_rect.y - span.old_rect.y) * t,
                    w: span.old_rect.w + (span.new_rect.w - span.old_rect.w) * t,
                    h: span.old_rect.h + (span.new_rect.h - span.old_rect.h) * t,
                },
            })
            .collect()
    }

    pub(crate) fn is_finished(&self, now: Instant) -> bool {
        let elapsed_ms = now.saturating_duration_since(self.started_at).as_millis() as f64;
        self.duration_ms == 0 || elapsed_ms >= self.duration_ms as f64
    }
}

/// 匹配 old/new 两边**同一段未改文字**，几何变了才产生 ReflowSpan。
///
/// 匹配方式：先用 `OffsetMap::map_new_range_to_old` 把 new 侧 cluster 的 byte range
/// 映射回 old 侧，两边 byte range 完全一致、且都没落在 excluded 里，才算同一段文字。
/// 这样 unmatched 的新字/旧字（真被改掉的）自然不会进 Reflow。
#[allow(clippy::too_many_arguments)]
fn collect_reflow_spans(
    old_snapshot: &EditorLayoutSnapshot,
    new_snapshot: &EditorLayoutSnapshot,
    offset_map: &OffsetMap,
    excluded_old: &[(usize, usize)],
    excluded_new: &[(usize, usize)],
) -> Vec<ReflowSpan> {
    let mut spans = Vec::new();
    for new_line in &new_snapshot.line_snapshots {
        for cluster in &new_line.clusters {
            let new_start = cluster.byte_start;
            let new_end = cluster.byte_end;
            if new_start >= new_end {
                continue;
            }
            if overlaps_any(new_start, new_end, excluded_new) {
                continue;
            }
            let Some((old_start, old_end)) = offset_map.map_new_range_to_old(new_start, new_end)
            else {
                // 映射不到旧坐标 → 不是一段没改过的文字。
                continue;
            };
            if overlaps_any(old_start, old_end, excluded_old) {
                continue;
            }
            let Some(old_line) = find_line_for_cluster(old_snapshot, old_start, old_end) else {
                continue;
            };
            let Some(old_cluster) = find_cluster(old_line, old_start, old_end) else {
                continue;
            };
            if !old_cluster
                .shaping_identity
                .is_same_shaping(&cluster.shaping_identity)
            {
                // shaping 变了说明这段字被重新排过，不做位置插值。
                continue;
            }
            let old_rect = old_line.source_rect_to_document_rect(&old_cluster.source_rect);
            let new_rect = new_line.source_rect_to_document_rect(&cluster.source_rect);
            if same_rect(&old_rect, &new_rect) {
                // 位置没变 → 不需要 ReflowMove。
                continue;
            }
            spans.push(ReflowSpan {
                old_range: (old_start, old_end),
                new_range: (new_start, new_end),
                old_rect,
                new_rect,
                snapshot_id: new_line.id,
                source_rect: cluster.source_rect.clone(),
            });
        }
    }
    spans
}

fn overlaps_any(start: usize, end: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|&(s, e)| start < e && s < end)
}

fn find_line_for_cluster(
    snapshot: &EditorLayoutSnapshot,
    byte_start: usize,
    byte_end: usize,
) -> Option<&crate::sujian_editor_item::layout_snapshot::PreparedLineSnapshot> {
    snapshot
        .line_snapshots
        .iter()
        .find(|line| line.byte_start <= byte_start && byte_end <= line.byte_end)
}

fn find_cluster(
    line: &crate::sujian_editor_item::layout_snapshot::PreparedLineSnapshot,
    byte_start: usize,
    byte_end: usize,
) -> Option<&crate::sujian_editor_item::layout_snapshot::LineClusterSnapshot> {
    line.clusters
        .iter()
        .find(|c| c.byte_start == byte_start && c.byte_end == byte_end)
}

fn same_rect(a: &SourceRect, b: &SourceRect) -> bool {
    const EPS: f64 = 1e-6;
    (a.x - b.x).abs() <= EPS
        && (a.y - b.y).abs() <= EPS
        && (a.w - b.w).abs() <= EPS
        && (a.h - b.h).abs() <= EPS
}

#[cfg(test)]
mod tests;
