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
    /// `spans[].new_range` 所属的正文纯文本（self 的 new 坐标系）。
    ///
    /// `retarget` 用它和「本次 target 文本」构造 OffsetMap，把已播到一半的
    /// span 对应到最新 layout 的同一段文字。
    pub target_text: String,
}

/// Issue #826 评论 13 阻塞：Reflow -> Conceal 的当前帧几何交接。
///
/// 「这一帧这个字现在在哪」的一次性快照，**不是**历史动画状态
/// （不带 started_at / remaining duration / historical stage / carried unit）。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ReflowCurrentGeometry {
    /// 该 span 的 target 坐标系 byte 范围（上一轮 target == 本次 request.base）。
    pub current_range: (usize, usize),
    /// 本帧它实际在屏幕上的目标矩形（文档坐标）。
    pub dest_rect: SourceRect,
    /// 贴图来自哪张行纹理 —— 交接后的 overlay 直接用这张图，
    /// 不能再回 burst base snapshot 去找（那是另一个坐标系、另一个 revision）。
    pub snapshot_id: LineSnapshotId,
    /// 上面那张行纹理里的源矩形。
    pub source_rect: SourceRect,
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
            target_text: String::new(),
        }
    }
    /// Issue #826 评论 4 问题 2：`retarget` 不能只保留"上一帧已经在 Reflow 的字"。
    ///
    /// 对最新 new layout 的每个 unchanged cluster，分两种情况：
    ///
    /// - **A. 上一份 Reflow 里已经有这个 cluster**（上一笔已经在动它）：
    ///   起点取 `previous.sample(now).dest_rect`，即当前屏幕上的真实位置。
    /// - **B. 上一份 Reflow 里没有，但这次 old -> new 几何变了**（这次新进入 Reflow
    ///   的字，典型场景：第一笔没撑满行所以 `HIJ` 没动，第二笔刚好撑满行让 `HIJ`
    ///   第一次掉到下一行）：退到"本次 `base_snapshot` 里的 old rect -> 最新 new rect"
    ///   建立新 span。绝不能 `continue` 跳过 —— 那会让 canonical 直接瞬移，没有重排动画。
    ///
    /// 这里仍然只有一份 `ReflowState`，不是重新引入历史 carried。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retarget(
        &self,
        now: Instant,
        old_snapshot: &EditorLayoutSnapshot,
        old_to_new: &OffsetMap,
        new_snapshot: &EditorLayoutSnapshot,
        prev_target_to_new: &OffsetMap,
        excluded_old: &[(usize, usize)],
        excluded_new: &[(usize, usize)],
        duration_ms: u64,
    ) -> Self {
        let sampled = self.sample(now);
        // self.spans 与 sampled 逐项对齐，按 new_range 建索引。
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
                let new_rect = new_line.source_rect_to_document_rect(&cluster.source_rect);

                // A. 上一份 Reflow 已经在动这个 cluster：起点 = 当前屏幕真实位置。
                let carried = prev_target_to_new
                    .map_new_range_to_old(new_start, new_end)
                    .and_then(|prev_range| {
                        self.spans
                            .iter()
                            .position(|span| span.new_range == prev_range)
                    })
                    .and_then(|index| {
                        sampled
                            .get(index)
                            .map(|frame| (self.spans[index].old_range, frame.dest_rect.clone()))
                    });
                if let Some((old_range, old_rect)) = carried {
                    spans.push(ReflowSpan {
                        old_range,
                        new_range: (new_start, new_end),
                        old_rect,
                        new_rect,
                        snapshot_id: new_line.id,
                        source_rect: cluster.source_rect.clone(),
                    });
                    continue;
                }

                // B. 这次新进入 Reflow 的字：起点取本次 base_snapshot 里的 old rect。
                let Some((old_start, old_end)) =
                    old_to_new.map_new_range_to_old(new_start, new_end)
                else {
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
                    continue;
                }
                let old_rect = old_line.source_rect_to_document_rect(&old_cluster.source_rect);
                if same_rect(&old_rect, &new_rect) {
                    // 几何没变，本来就不需要 Reflow。
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
        Self {
            spans,
            started_at: now,
            duration_ms: duration_ms.max(1),
            target_text: String::new(),
        }
    }

    /// Issue #826 评论 4 问题 3：Reflow 接管期间必须从静态正文层挖掉的目标位置。
    ///
    /// 动画层正在画"正在移动的那一份"，但最新 canonical 静态层的**最终位置**同时
    /// 也画了一份同样的字 -> 双影。所以每个 active span 用它的 `new_rect`（canonical
    /// 目标位置）+ `snapshot_id` 生成一条静态层 exclusion clip。
    ///
    /// 返回 `(x, y, w, h, snapshot_id)`，与
    /// `hidden_canonical_rects_for` 同一形状，方便 render plan 合并。
    pub(crate) fn target_clip_rects(&self) -> Vec<(f64, f64, f64, f64, LineSnapshotId)> {
        self.spans
            .iter()
            .map(|span| {
                (
                    span.new_rect.x,
                    span.new_rect.y,
                    span.new_rect.w,
                    span.new_rect.h,
                    span.snapshot_id,
                )
            })
            .collect()
    }

    /// 本 state 的 new 坐标系对应的正文纯文本。
    pub(crate) fn target_text(&self) -> &str {
        &self.target_text
    }

    pub(crate) fn set_target_text(&mut self, text: String) {
        self.target_text = text;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// 采样本帧每段未改文字的位置。
    /// Issue #826 评论 13 阻塞：当前帧几何快照。
    ///
    /// 一个 glyph 从 Reflow 所有权切到 Conceal 所有权时，必须把「这一帧它实际
    /// 在哪」传过去，否则会瞬移回 canonical 位置：
    ///
    /// ```text
    /// A|B  ->  输入 X  ->  AX|B
    /// B 正在 Reflow：20 -> 60，80ms 时屏幕位置 x = 55
    /// 此时按 Delete 把 B 删掉
    /// ```
    ///
    /// 此时最新 Reflow retarget 会正确把 B 排除（B 已 changed），新的 Delete
    /// Frontier 给 B 建 ConcealTrack。但 ConcealTrack 如果只从 `base_snapshot`
    /// 取几何，base 是 `AXB`、B 在 x=60，于是屏幕上出现
    /// `55 -> 60 瞬移一下 -> 再开始吞字`。自动换行时这跳变可能跨整行。
    ///
    /// 这里只返回「当前屏幕位置」，**不带** started_at / remaining duration /
    /// historical stage / carried unit / 第二个动画对象 —— 那就是旧的历史动画
    /// 交棒，#826 已经删干净了。
    pub(crate) fn current_geometry(&self, now: Instant) -> Vec<ReflowCurrentGeometry> {
        self.sample(now)
            .into_iter()
            .zip(self.spans.iter())
            .map(|(frame, span)| ReflowCurrentGeometry {
                current_range: span.new_range,
                dest_rect: frame.dest_rect,
                snapshot_id: frame.snapshot_id,
                source_rect: frame.source_rect.clone(),
            })
            .collect()
    }

    /// Issue #826 评论 34：把整条 Reflow 时间轴整体平移 `delta`。
    ///
    /// 滚动 pause / resume 用：resume 时把暂停期间的墙钟时长补回 `started_at`，
    /// 恢复后仍从 pause 那一刻的进度继续，不会按墙钟跳到终点。只平移起点，
    /// 不重建 spans、不排历史队列。
    pub(crate) fn shift_started_at(&mut self, delta: std::time::Duration) {
        self.started_at += delta;
    }

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
