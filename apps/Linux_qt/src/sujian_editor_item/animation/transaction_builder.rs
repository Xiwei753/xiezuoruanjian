use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::edit_motion::{diff_plain_text, CursorRect, EditorAnimationKind, PreparedEditMotion};
use crate::sujian_editor_item::animation_mode::AnimationMode;
use crate::sujian_editor_item::layout_snapshot::{
    ClusterInsertRelation, EditorLayoutSnapshot, SourceRect,
};
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::animation::{
    PreparedTextVisualTransaction, PreparedVisualUnit,
    TextVisualOperationKind, TextVisualTransactionState, TransactionTimeline,
};
use crate::sujian_editor_item::animation::rebase::{
    PreparedRebaseHandoff,
    match_rebase_frames,
};
use crate::sujian_editor_item::animation::cursor_motion::build_cursor_visual_track;
use super::coordinator::LinuxEditorAnimationCoordinator;
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
use crate::editor::layout::compute_affected_paragraph_ranges;
use crate::sujian_editor_item::editor_animation_debug_log;

pub(crate) fn operation_kind_label(kind: TextVisualOperationKind) -> &'static str {
    match kind {
        TextVisualOperationKind::Insert => "Insert",
        TextVisualOperationKind::Delete => "Delete",
        TextVisualOperationKind::CompositionUpdate => "CompositionUpdate",
        TextVisualOperationKind::CompositionCommitOrCancel => "CompositionCommitOrCancel",
    }
}

pub(crate) fn unit_kind_labels(units: &[PreparedVisualUnit]) -> Vec<String> {
    units
        .iter()
        .map(|u| format!("{:?}", u.slice.kind))
        .collect()
}

pub(crate) fn emit_transaction_diagnostic(tx: &PreparedTextVisualTransaction, event: &str, reason: &str) {
    crate::sujian_editor_item::editor_animation_diagnostic_event(
        event,
        &tx.key,
        operation_kind_label(tx.operation_kind),
        tx.old_cursor_rect.as_ref().map(|r| (r.x, r.top)),
        tx.new_cursor_rect.as_ref().map(|r| (r.x, r.top)),
        &unit_kind_labels(&tx.units).join(","),
        tx.timeline.first_render_wall_ms,
        reason,
    );
}

struct ReflowClusterRef {
    line_idx: usize,
    cluster_idx: usize,
    byte_start: usize,
    byte_end: usize,
}

pub(crate) fn build_insert_reveal_slices(
    key: VisualTransactionKey,
    new_snapshot: &EditorLayoutSnapshot,
    inserted_range: (usize, usize),
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    let (range_start, range_end) = inserted_range;

    for (_line_idx, new_line) in new_snapshot.line_snapshots.iter().enumerate() {
        for (_cluster_idx, new_cluster) in new_line.clusters.iter().enumerate() {
            // Issue #724 评论 5751268664 缺口1: 用 Inside/Partial 分类替代 overlap 整块消费。
            // - Inside：cluster 完全在 inserted 范围内，整个 cluster 进入 InsertReveal + static hide。
            // - Partial：cluster 部分在 inserted 范围内（ligature/cluster 跨越 inserted 边界），
            //   只把属于 inserted 的子片段（clipped source rect）交给 InsertReveal + static hide，
            //   旧邻字部分保持原样不进入 static hide，避免整个 cluster 被当成新插入文字。
            // - 不相交：跳过，不参与 InsertReveal，不进入 static hide。
            let relation = match new_cluster.relate_to_inserted_range(range_start, range_end) {
                Some(r) => r,
                None => continue,
            };
            // Issue #722 评论 5748596920 问题5: 跳过纯空格/tab/换行/控制字符。
            // 这些非可见字符不应创建 InsertReveal 和 static patch，
            // 避免文字前插空格闪一下/手动换行闪一下。
            // 已有文字位移交给 ReflowMove，caret 走 canonical track。
            //
            // Issue #736 评论 5777408243 问题1: 不再把 range 取不到正文静默解释成空字符串。
            // cluster 的 document byte range 必须属于 snapshot.virtual_text（同一 revision），
            // 否则属于快照不变量被破坏，记明确的 invariant diagnostic 后跳过该 cluster。
            let cluster_text = match new_snapshot
                .virtual_text
                .get(new_cluster.byte_start..new_cluster.byte_end)
            {
                Some(text) => text,
                None => {
                    crate::backend::app_backend::debug_warn_static(
                        "animation_coordinator",
                        "insert_reveal_cluster_text_range_out_of_virtual_text",
                        &format!(
                            "snapshot revision={} cluster byte_start={} byte_end={} \
                             virtual_text.len={} — cluster byte range not in virtual_text, \
                             skip InsertReveal for this cluster (snapshot invariant broken)",
                            new_snapshot.revision.0,
                            new_cluster.byte_start,
                            new_cluster.byte_end,
                            new_snapshot.virtual_text.len(),
                        ),
                    );
                    continue;
                }
            };
            if cluster_text
                .chars()
                .all(|c| c.is_whitespace() || c.is_control())
            {
                continue;
            }
            // Issue #724 评论 5751268664 缺口1: Inside 用整个 source_rect，
            // Partial 用精确 glyph 几何 + clipped_byte_range。
            // 不再用 UTF-8 byte 比例猜视觉宽度——字节长度不是 glyph 宽度，
            // 中文 UTF-8 3 字节/拉丁 1 字节/ligature/组合字符/fallback font/
            // 比例字体/RTL 都不能按 byte ratio 对应到 source rect 的 x/w。
            // Partial 的精确 source rect 从 QTextLayout 侧取（支持 split ligature）。
            let (new_sr, slice_byte_start, slice_byte_end) = match relation {
                ClusterInsertRelation::Inside => (
                    new_cluster.source_rect.clone(),
                    new_cluster.byte_start,
                    new_cluster.byte_end,
                ),
                ClusterInsertRelation::Partial {
                    clipped_byte_start,
                    clipped_byte_end,
                } => {
                    // Issue #724 评论 5751573705 问题1: 从 QTextLayout 取精确 glyph 几何。
                    // 失败时（layout 缺失/范围越界）回退到完整 cluster source_rect，
                    // 至少保证视觉不崩——宁可多画一帧旧邻字，也不猜错位置。
                    let precise_sr = new_line.get_precise_glyph_rect_for_byte_range(
                        new_snapshot.revision.0,
                        &new_snapshot.virtual_text,
                        clipped_byte_start,
                        clipped_byte_end,
                    );
                    if let Some(sr) = precise_sr {
                        (sr, clipped_byte_start, clipped_byte_end)
                    } else {
                        (
                            new_cluster.source_rect.clone(),
                            clipped_byte_start,
                            clipped_byte_end,
                        )
                    }
                }
            };
            let new_doc = new_line.source_rect_to_document_rect(&new_sr);
            let mut slice = AnimatedSlice::insert_reveal(
                key,
                new_line.id,
                new_sr.clone(),
                new_doc.clone(),
                0.0,
                0.0,
                slice_byte_start,
                slice_byte_end,
                Some(new_cluster.shaping_identity.clone()),
                // Issue #722 评论 5749572808 问题1: 传全文视觉行 id，
                // 不是 line_snapshots 的局部数组下标。line_idx 仍用于
                // managed_new_clusters/patches_by_line 的局部索引。
                Some(new_line.visual_line_id),
            );
            // Issue #727 评论 5755858583 问题2: 直接在 slice 上写 canonical 独占区域，
            // 不再生成 StaticLinePatch。AnimatedSlice 成为唯一事实源。
            slice.static_hidden_document_rects = vec![new_doc];
            slices.push(slice);
        }
    }

    let slices = merge_adjacent_slices(slices);
    slices
}

pub(crate) fn build_delete_conceal_slices(
    key: VisualTransactionKey,
    old_snapshot: &EditorLayoutSnapshot,
    deleted_range: (usize, usize),
    old_cursor_rect: Option<&CursorRect>,
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    let (range_start, range_end) = deleted_range;
    let old_cx = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
    let old_cy = old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

    for old_line in &old_snapshot.line_snapshots {
        for old_cluster in &old_line.clusters {
            // Issue #724 评论 5750911834 问题 1: cluster 匹配条件改为 overlap 判断，
            // 允许部分落在 deleted_range 边界的 cluster（ligature 拆分、跨行 cluster）。
            // 旧逻辑 `byte_start >= range_start && byte_end <= range_end` 会丢弃部分
            // 落在边界的 cluster。
            if old_cluster.byte_start < range_end && old_cluster.byte_end > range_start {
                let old_sr = old_cluster.source_rect.clone();
                let old_doc = old_line.source_rect_to_document_rect(&old_sr);
                // 按删除前光标位置（old_cursor_rect）决定收缩方向：
                // 光标在被删文字右侧 → Backspace → conceal_to_left_edge = true（向左边缘收缩，右段先消失，光标跟右边缘往左走）
                // 光标在被删文字左侧 → Delete 键 → conceal_to_left_edge = false（向右边缘收缩，左段先消失，光标固定不动）
                let left = old_doc.x;
                let right = old_doc.x + old_doc.w;
                let conceal_to_left_edge = (old_cx - right).abs() <= (old_cx - left).abs();
                slices.push(AnimatedSlice::delete_conceal(
                    key,
                    old_line.id,
                    old_sr,
                    old_doc,
                    old_cx,
                    old_cy,
                    old_cluster.byte_start,
                    old_cluster.byte_end,
                    Some(old_cluster.shaping_identity.clone()),
                    conceal_to_left_edge,
                    // Issue #722 评论 5749572808 问题1: 传全文视觉行 id，
                    // 不是 line_snapshots 的局部数组下标。
                    Some(old_line.visual_line_id),
                ));
            }
        }
    }

    // Delete 不生成 StaticLinePatch：删除后的 canonical new text 可以立即作为背景，
    // 旧字只由 overlay 吞掉。
    let slices = merge_adjacent_slices(slices);
    slices
}

pub(crate) fn build_cluster_reflow_slices(
    key: VisualTransactionKey,
    old_snapshot: &EditorLayoutSnapshot,
    new_snapshot: &EditorLayoutSnapshot,
    offset_map: &OffsetMap,
    excluded_old_ranges: &[(usize, usize)],
    excluded_new_ranges: &[(usize, usize)],
    old_cursor_rect: Option<&CursorRect>,
    new_cursor_rect: Option<&CursorRect>,
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    // Issue #738 评论 5789470425 问题3: CrossFade group id 分配器（事务内唯一）。
    // 每对 ReflowCrossFadeOld/New 共享同一 group_id，reconcile 以 group 为单位成对重绑。
    let mut next_crossfade_group_id: u64 = 1;

    // Issue #687: old_cx/old_cy/new_cx/new_cy 不再需要——changed range 由
    // build_insert_reveal_slices / build_delete_conceal_slices 显式拥有，
    // reflow 只处理 unchanged material。
    let _ = (old_cursor_rect, new_cursor_rect);

    // ── 阶段 1：收集所有未 excluded 的 old/new cluster refs ──
    // 被 excluded 的 old cluster 保留在 old_refs 中（标记 excluded=true），
    // 不参与一对一匹配和多对多处理。被 excluded 的 new cluster 直接跳过。
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

    // ── 阶段 2：一对一精确匹配 ──
    // Issue #712: 先稳定一对一配对，消除普通输入/删除/Enter 的错误 CrossFade。
    // 对每个未 excluded 的 new cluster，用 offset_map 映射回 old 坐标，
    // 找 byte range 精确对应的唯一 old cluster。
    // 匹配条件：mapped_old_start == old_cluster.byte_start && mapped_old_end == old_cluster.byte_end
    let n_old = old_refs.len();
    let n_new = new_refs.len();
    let mut old_matched: Vec<bool> = vec![false; n_old];
    let mut new_matched: Vec<bool> = vec![false; n_new];

    for (ni, nref) in new_refs.iter().enumerate() {
        // 用 offset_map 将 new cluster 的 byte range 映射回 old 坐标
        let mapped_old_range = offset_map.map_new_range_to_old(nref.byte_start, nref.byte_end);

        // 映射失败（None）的 new cluster 无法一对一匹配，跳过进入多对多处理
        let (mapped_old_start, mapped_old_end) = match mapped_old_range {
            Some(r) => r,
            None => continue,
        };

        // 查找精确匹配的 old cluster：byte range 完全对应
        let matching_oi = old_refs
            .iter()
            .enumerate()
            .filter(|(oi, _)| !old_matched[*oi] && !old_excluded_flags[*oi])
            .find(|(_, oref)| {
                oref.byte_start == mapped_old_start && oref.byte_end == mapped_old_end
            })
            .map(|(oi, _)| oi);

        // 没找到唯一匹配的 new cluster，跳过进入多对多处理
        let oi = match matching_oi {
            Some(idx) => idx,
            None => continue,
        };

        let oref = &old_refs[oi];
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
                // 几何变了：只生成 ReflowMove
                // Issue #727 评论 5755858583 问题2: 直接在 slice 上写 canonical 独占区域。
                let mut slice = AnimatedSlice::reflow_move(
                    key,
                    old_line.id,
                    old_sr,
                    old_doc,
                    new_line.id,
                    new_sr.clone(),
                    new_doc.clone(),
                    new_cluster.byte_start,
                    new_cluster.byte_end,
                    Some(old_cluster.shaping_identity.clone()),
                );
                slice.static_hidden_document_rects = vec![new_doc];
                slices.push(slice);
            }
            // 几何没变：不生成任何动画（关键改进——消除普通输入/删除/Enter 的错误 CrossFade）
        } else {
            // byte identity 对得上但 shaping 真变了：生成一对 ReflowCrossFade
            // Issue #738 评论 5788513592 问题3: old/new 两侧写入各自真实 shaping identity，
            // rebind 时 is_same_shaping 能返回 true，CrossFade 可按新布局继续而非必然 Remove。
            // Issue #738 评论 5789470425 问题3: old/new 两侧共享同一 crossfade_group_id，
            // reconcile 以 group 为单位成对重绑，不再把 old/new 各自独立判死。
            let group_id = next_crossfade_group_id;
            next_crossfade_group_id += 1;
            slices.push(AnimatedSlice::reflow_crossfade_old(
                key,
                old_line.id,
                old_sr,
                old_doc.clone(),
                new_doc.clone(),
                new_cluster.byte_start,
                new_cluster.byte_end,
                Some(old_cluster.shaping_identity.clone()),
                Some(group_id),
            ));
            // Issue #727 评论 5755858583 问题2: ReflowCrossFadeNew 直接写 canonical 独占区域。
            let mut new_slice = AnimatedSlice::reflow_crossfade_new(
                key,
                new_line.id,
                new_sr.clone(),
                old_doc,
                new_doc.clone(),
                new_cluster.byte_start,
                new_cluster.byte_end,
                Some(new_cluster.shaping_identity.clone()),
                Some(group_id),
            );
            new_slice.static_hidden_document_rects = vec![new_doc];
            slices.push(new_slice);
        }

        // 标记已配对的 old/new cluster，不再参与后续多对多处理
        old_matched[oi] = true;
        new_matched[ni] = true;
    }

    // ── 阶段 3：多对多处理（未配对的 cluster）──
    // 剩下确实无法唯一对应的 cluster（未配对的 old 和 new），进入多对多处理。
    // 每个 old 在原位 fade-out，每个 new 在原位 fade-in。
    let unmatched_old: Vec<usize> = (0..n_old)
        .filter(|&oi| !old_matched[oi] && !old_excluded_flags[oi])
        .collect();
    let unmatched_new: Vec<usize> = (0..n_new).filter(|&ni| !new_matched[ni]).collect();

    // 只有同时存在未配对的 old 和 new 时才生成 CrossFade
    if !unmatched_old.is_empty() && !unmatched_new.is_empty() {
        // Issue #738 评论 5792244119 问题 3: 多对多 CrossFade old/new 共享同一个
        // group_id，rebind 按 group_id 配对时能真正成组（一组包含多 old + 多 new）。
        // 不再给 old/new 各自独立发 group_id（那会导致 crossfade_pairs 永远配不上，
        // 所有 CrossFade units 掉进"未配对独立处理"路径，出现只续一边/只删一边）。
        let group_id = next_crossfade_group_id;
        next_crossfade_group_id += 1;
        for &oi in &unmatched_old {
            let oref = &old_refs[oi];
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
                Some(old_cluster.shaping_identity.clone()),
                Some(group_id),
            ));
        }

        for &ni in &unmatched_new {
            let nref = &new_refs[ni];
            let new_line = &new_snapshot.line_snapshots[nref.line_idx];
            let new_cluster = &new_line.clusters[nref.cluster_idx];
            let new_sr = new_cluster.source_rect.clone();
            let new_doc = new_line.source_rect_to_document_rect(&new_sr);
            let new_doc_for_hide = new_doc.clone();

            // Issue #727 评论 5755858583 问题2: ReflowCrossFadeNew 直接写 canonical 独占区域。
            // Issue #738 评论 5788513592 问题3: 写入真实 shaping identity。
            // Issue #738 评论 5792244119 问题3: 写入共享的 group_id（old/new 同组）。
            let mut new_slice = AnimatedSlice::reflow_crossfade_new(
                key,
                new_line.id,
                new_sr.clone(),
                new_doc.clone(),
                new_doc,
                new_cluster.byte_start,
                new_cluster.byte_end,
                Some(new_cluster.shaping_identity.clone()),
                Some(group_id),
            );
            new_slice.static_hidden_document_rects = vec![new_doc_for_hide];
            slices.push(new_slice);
        }
    }

    // Issue #727 评论 5755858583 问题2: 不再生成 StaticLinePatches。
    // static_hidden_document_rects 已在创建 slice 时直接写入。
    // run_managed_new_clusters 不再需要——裁剪信息已在 slice 上。

    let slices = merge_adjacent_slices(slices);
    slices
}

pub(crate) fn merge_adjacent_slices(slices: Vec<AnimatedSlice>) -> Vec<AnimatedSlice> {
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
            // 同一行吞字且同方向：from_document_rect 的 y 相同，conceal_to_left_edge 相同
            (a.from_document_rect.y - b.from_document_rect.y).abs() < 0.5
                && a.conceal_to_left_edge == b.conceal_to_left_edge
        }
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
            // 移动向量相同：dx = to.x - from.x, dy = to.y - from.y
            let a_dx = a.to_document_rect.x - a.from_document_rect.x;
            let a_dy = a.to_document_rect.y - a.from_document_rect.y;
            let b_dx = b.to_document_rect.x - b.from_document_rect.x;
            let b_dy = b.to_document_rect.y - b.from_document_rect.y;
            let same_vector = (a_dx - b_dx).abs() < 0.5 && (a_dy - b_dy).abs() < 0.5;
            // Issue #738 评论 5789470425 问题3: CrossFade 只能合并同 group 同 side。
            // old 侧和 new 侧不能互相合并；不同 group 不能合并。
            let same_crossfade_group = a.crossfade_group_id == b.crossfade_group_id
                && a.crossfade_side == b.crossfade_side;
            same_vector && same_crossfade_group
        }
    }
}

fn merged_byte_range(a: (usize, usize), b: (usize, usize)) -> (usize, usize) {
    (a.0.min(b.0), a.1.max(b.1))
}

fn merge_two(a: &AnimatedSlice, b: &AnimatedSlice) -> AnimatedSlice {
    let (byte_start, byte_end) =
        merged_byte_range((a.byte_start, a.byte_end), (b.byte_start, b.byte_end));
    // Issue #738 评论 5789470425 问题2: 合并 reflow_anchors 列表，不丢子 cluster 身份。
    let merged_anchors: Vec<crate::sujian_editor_item::animated_slice::ReflowAnchor> = a
        .reflow_anchors
        .iter()
        .chain(&b.reflow_anchors)
        .cloned()
        .collect();
    // merged unit 的 from/to/source 取各 anchor 的 union（连续矩形做整体插值）。
    // 用 inline min/max 计算而非 bounding_box helper，强调 reflow_anchors 才是逐 cluster 真相。
    let merged_from = union_source_rect(&a.from_document_rect, &b.from_document_rect);
    let merged_to = union_source_rect(&a.to_document_rect, &b.to_document_rect);
    let merged_source = union_source_rect(&a.source_rect, &b.source_rect);
    // shaping_identity 取首个 anchor 的代表值；逐 cluster 真实 shaping 在 reflow_anchors。
    let head_shaping = a.shaping_identity.clone();
    AnimatedSlice {
        kind: a.kind,
        snapshot_id: a.snapshot_id,
        source_rect: merged_source,
        from_document_rect: merged_from,
        to_document_rect: merged_to,
        opacity_from: a.opacity_from,
        opacity_to: a.opacity_to,
        scale_from: a.scale_from,
        scale_to: a.scale_to,
        byte_start,
        byte_end,
        shaping_identity: head_shaping,
        conceal_to_left_edge: a.conceal_to_left_edge,
        visual_line_id: a.visual_line_id,
        start_fraction: a.start_fraction.min(b.start_fraction),
        static_hidden_document_rects: a
            .static_hidden_document_rects
            .iter()
            .chain(&b.static_hidden_document_rects)
            .cloned()
            .collect(),
        crossfade_group_id: a.crossfade_group_id,
        crossfade_side: a.crossfade_side,
        reflow_anchors: merged_anchors,
    }
}

fn union_source_rect(a: &SourceRect, b: &SourceRect) -> SourceRect {
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


impl LinuxEditorAnimationCoordinator {
    pub(crate) fn create_transaction_from_prepared_handoff(
        &mut self,
        prepared: Option<PreparedRebaseHandoff>,
        vt: &PreparedEditMotion,
        smooth_cursor_enabled: bool,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_cursor_visual_line_id: Option<usize>,
        new_cursor_visual_line_id: Option<usize>,
        old_cursor_line_top: f64,
        old_cursor_line_bottom: f64,
        new_cursor_line_top: f64,
        new_cursor_line_bottom: f64,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> Option<VisualTransactionKey> {
        let prepared = prepared?;
        match prepared {
            PreparedRebaseHandoff::Insert {
                rebase_frames,
                caret_handoff,
                range_start,
                range_end,
                insert_offset_map,
                visual_affected_byte_range_old,
                visual_affected_byte_range_new,
            } => {
                let key = self.alloc_key();
                let mut slices = Vec::new();

                // Issue #687: Insert 事务先生成显式 InsertReveal，再调用 reflow builder；
                // reflow 必须排除 inserted_range。changed range 由 Core 显式拥有。
                // Issue #727 评论 5755858583 问题5: !smooth_cursor_enabled 时跳过
                // InsertReveal（CaretDriven unit），只保留 Reflow。
                let inserted_range_tuple = (range_start, range_end);
                if smooth_cursor_enabled {
                    let reveal_slices =
                        build_insert_reveal_slices(key, new_snapshot, inserted_range_tuple);
                    slices.extend(reveal_slices);
                }

                let reflow_slices = build_cluster_reflow_slices(
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

                let mut units: Vec<PreparedVisualUnit> = slices
                    .into_iter()
                    .map(|s| PreparedVisualUnit::wrap(s, vt.duration_ms))
                    .collect();
                match_rebase_frames(&rebase_frames, &mut units, &insert_offset_map);

                // Issue #690 评论 5681206040: 构建 caret track。
                // 有 handoff 时 from = sampled caret, duration = 旧 track 剩余时长；
                // 无 handoff 时 from = old_cursor_rect, duration = 事务时长。
                // Issue #690 评论 5682867529: 不再传 now，started_at 留 None，等 Rendering 再启动。
                let cursor_visual_track = build_cursor_visual_track(
                    old_cursor_rect.as_ref(),
                    new_cursor_rect.as_ref(),
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    caret_handoff,
                    vt.duration_ms,
                );
                // Issue #710 评论 5731145076 症状六: visual_affected_byte_range 已在
                // 查冲突之前提前计算（current-old 坐标系逐事务映射需要 old-side range）。
                // Issue #710 评论 5732160521 问题 1/3: Insert 事务 old 侧是插入点
                // (range_start, range_start)，new 侧是 inserted_range。
                let prepared_tx = PreparedTextVisualTransaction {
                    key,
                    state: TextVisualTransactionState::Pending,
                    operation_kind: TextVisualOperationKind::Insert,
                    timeline: TransactionTimeline::new(vt.duration_ms),
                    units,
                    old_cursor_rect,
                    new_cursor_rect,
                    cursor_visual_track,
                    cancel_reason: None,
                    texture_prepared: false,
                    old_snapshot: Some(old_snapshot.clone()),
                    new_snapshot: Some(new_snapshot.clone()),
                    cursor_owner_epoch,
                    caret_motion_retired: false,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    layout_basis_revision,
                };

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared_tx, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Insert inserted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    inserted_range_tuple,
                    unit_kind_labels(&prepared_tx.units),
                    rebase_frames.len(),
                ));

                self.prepared_queue.enqueue(prepared_tx);

                Some(key)
            }
            PreparedRebaseHandoff::Delete {
                rebase_frames,
                caret_handoff,
                deleted_ranges,
                delete_offset_map,
                visual_affected_byte_range_old,
                visual_affected_byte_range_new,
            } => {
                let key = self.alloc_key();

                let mut slices = Vec::new();

                // Issue #687: Delete 事务先生成显式 DeleteConceal，再调用 reflow builder；
                // reflow 必须排除 deleted_range。changed range 由 Core 显式拥有。
                // 对每个 deleted range 生成显式 DeleteConceal 切片。
                // Issue #727 评论 5755858583 问题5: !smooth_cursor_enabled 时跳过
                // DeleteConceal（CaretDriven unit），只保留 Reflow。
                if smooth_cursor_enabled {
                    for &(d_start, d_end) in &deleted_ranges {
                        let conceal_slices = build_delete_conceal_slices(
                            key,
                            old_snapshot,
                            (d_start, d_end),
                            old_cursor_rect.as_ref(),
                        );
                        slices.extend(conceal_slices);
                    }
                }

                let reflow_slices = build_cluster_reflow_slices(
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

                let mut units: Vec<PreparedVisualUnit> = slices
                    .into_iter()
                    .map(|s| PreparedVisualUnit::wrap(s, vt.duration_ms))
                    .collect();
                match_rebase_frames(&rebase_frames, &mut units, &delete_offset_map);

                // Issue #690 评论 5681206040 + 5682867529: 构建 caret track（不传 now，等 Rendering 再启动）。
                let cursor_visual_track = build_cursor_visual_track(
                    old_cursor_rect.as_ref(),
                    new_cursor_rect.as_ref(),
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    caret_handoff,
                    vt.duration_ms,
                );
                // Issue #710 评论 5731145076 症状六: visual_affected_byte_range 已在
                // 查冲突之前提前计算（current-old 坐标系逐事务映射需要 old-side range）。
                // Issue #710 评论 5732160521 问题 1/3: Delete 事务 old 侧是 deleted_range，
                // new 侧是删除后落点 (rebase_byte_start, rebase_byte_start)。
                let prepared_tx = PreparedTextVisualTransaction {
                    key,
                    state: TextVisualTransactionState::Pending,
                    operation_kind: TextVisualOperationKind::Delete,
                    timeline: TransactionTimeline::new(vt.duration_ms),
                    units,
                    old_cursor_rect,
                    new_cursor_rect,
                    cursor_visual_track,
                    cancel_reason: None,
                    texture_prepared: false,
                    old_snapshot: Some(old_snapshot.clone()),
                    new_snapshot: Some(new_snapshot.clone()),
                    cursor_owner_epoch,
                    caret_motion_retired: false,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    layout_basis_revision,
                };

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared_tx, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Delete deleted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    deleted_ranges,
                    unit_kind_labels(&prepared_tx.units),
                    rebase_frames.len(),
                ));

                self.prepared_queue.enqueue(prepared_tx);

                Some(key)
            }
        }
    }

    pub fn process_transaction(
        &mut self,
        vt: &PreparedEditMotion,
        typing_animation_enabled: bool,
        smooth_cursor_enabled: bool,
        is_scrolling: bool,
        is_loading: bool,
        is_applying_format: bool,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_cursor_visual_line_id: Option<usize>,
        new_cursor_visual_line_id: Option<usize>,
        old_cursor_line_top: f64,
        old_cursor_line_bottom: f64,
        new_cursor_line_top: f64,
        new_cursor_line_bottom: f64,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> Option<VisualTransactionKey> {
        // Issue #727 评论 5755858583 问题5: !smooth_cursor_enabled 不再整笔 return None。
        // 只去掉 CaretDriven units（InsertReveal/DeleteConceal），Reflow 是否保留由
        // typing_animation_enabled 决定，不要把两类动画重新绑死。
        if !typing_animation_enabled || is_scrolling || is_loading || is_applying_format {
            return None;
        }

        // Issue #727 约束 5: valid_caret_motion_track 检查。
        // 没有 old/new cursor rect 就没有有效 caret motion track，不创建吞吐字事务。
        // Issue #727 评论 5755858583 问题5: 仅在 smooth_cursor_enabled 时才要求
        // valid_caret_motion_track——!smooth_cursor_enabled 时不创建 CaretDriven units，
        // 只创建 Reflow，不需要 caret motion track。
        let valid_caret_motion_track = old_cursor_rect.is_some() && new_cursor_rect.is_some();
        if smooth_cursor_enabled && !valid_caret_motion_track {
            return None;
        }

        let mode = AnimationMode::from_context(is_scrolling, is_loading, is_applying_format);
        if !mode.should_create_transaction() {
            return None;
        }

        match vt.kind {
            EditorAnimationKind::Insert => {
                if let Some(range) = vt.inserted_range {
                    let range_start = range.start().value();
                    let range_end = range.end().value();
                    let insert_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                    // Issue #710 评论 5733109905: 冲突检测用 current-old 坐标系。
                    // 先计算 visual_affected_byte_range 得到 old-side range (old_s, old_e)，
                    // 再用 old_s/old_e 查冲突。insert_offset_map 仍保留用于 rebase。
                    let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
                        let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                            &vt.old_text,
                            &vt.new_text,
                            (range_start, range_start),
                            (range_start, range_end),
                        );
                        (Some((old_s, old_e)), Some((new_s, new_e)))
                    };
                    let (conflict_old_start, conflict_old_end) =
                        visual_affected_byte_range_old.unwrap_or((range_start, range_start));
                    let conflicting = self.prepared_queue.find_conflicting_transaction(
                        &vt.old_text,
                        conflict_old_start,
                        conflict_old_end,
                    );
                    // 纯插入在 old 文档里就是 range_start 这一个位置点。
                    let now = Instant::now();
                    let (rebase_frames, caret_handoff) = self.take_rebase_frames(
                        &conflicting,
                        "rebased_by_insert",
                        now,
                        Some((&[(range_start, range_start)], &insert_offset_map)),
                        &vt.old_text,
                        cursor_owner_epoch,
                    );

                    let key = self.alloc_key();
                    let mut slices = Vec::new();

                    // Issue #687: Insert 事务先生成显式 InsertReveal，再调用 reflow builder；
                    // reflow 必须排除 inserted_range。changed range 由 Core 显式拥有。
                    // Issue #727 评论 5755858583 问题5: !smooth_cursor_enabled 时跳过
                    // InsertReveal（CaretDriven unit），只保留 Reflow。
                    let inserted_range_tuple = (range_start, range_end);
                    if smooth_cursor_enabled {
                        let reveal_slices =
                            build_insert_reveal_slices(key, new_snapshot, inserted_range_tuple);
                        slices.extend(reveal_slices);
                    }

                    let reflow_slices = build_cluster_reflow_slices(
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

                    let mut units: Vec<PreparedVisualUnit> = slices
                        .into_iter()
                        .map(|s| PreparedVisualUnit::wrap(s, vt.duration_ms))
                        .collect();
                    match_rebase_frames(&rebase_frames, &mut units, &insert_offset_map);

                    // Issue #690 评论 5681206040: 构建 caret track。
                    // 有 handoff 时 from = sampled caret, duration = 旧 track 剩余时长；
                    // 无 handoff 时 from = old_cursor_rect, duration = 事务时长。
                    // Issue #690 评论 5682867529: 不再传 now，started_at 留 None，等 Rendering 再启动。
                    let cursor_visual_track = build_cursor_visual_track(
                        old_cursor_rect.as_ref(),
                        new_cursor_rect.as_ref(),
                        old_cursor_visual_line_id,
                        new_cursor_visual_line_id,
                        old_cursor_line_top,
                        old_cursor_line_bottom,
                        new_cursor_line_top,
                        new_cursor_line_bottom,
                        caret_handoff,
                        vt.duration_ms,
                    );
                    // Issue #710 评论 5731145076 症状六: visual_affected_byte_range 已在
                    // 查冲突之前提前计算（current-old 坐标系逐事务映射需要 old-side range）。
                    // Issue #710 评论 5732160521 问题 1/3: Insert 事务 old 侧是插入点
                    // (range_start, range_start)，new 侧是 inserted_range。
                    let prepared = PreparedTextVisualTransaction {
                        key,
                        state: TextVisualTransactionState::Pending,
                        operation_kind: TextVisualOperationKind::Insert,
                        timeline: TransactionTimeline::new(vt.duration_ms),
                        units,
                        old_cursor_rect,
                        new_cursor_rect,
                        cursor_visual_track,
                        cancel_reason: None,
                        texture_prepared: false,
                        old_snapshot: Some(old_snapshot.clone()),
                        new_snapshot: Some(new_snapshot.clone()),
                        cursor_owner_epoch,
                        caret_motion_retired: false,
                        visual_affected_byte_range_old,
                        visual_affected_byte_range_new,
                        layout_basis_revision,
                    };

                    // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                    emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                    editor_animation_debug_log(&format!(
                        "anim_event: key={:?} op=Insert inserted={:?} unit_kinds={:?} carried_rebase={}",
                        key,
                        inserted_range_tuple,
                        unit_kind_labels(&prepared.units),
                        rebase_frames.len(),
                    ));

                    self.prepared_queue.enqueue(prepared);

                    return Some(key);
                }
            }
            EditorAnimationKind::Delete => {
                let deleted_ranges: Vec<(usize, usize)> = if let Some(range) = vt.deleted_range {
                    vec![(range.start().value(), range.end().value())]
                } else {
                    let changes = diff_plain_text(&vt.old_text, &vt.new_text);
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
                let delete_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                // Issue #710 评论 5733109905: 冲突检测用 current-old 坐标系。
                // 先计算 visual_affected_byte_range 得到 old-side range (old_s, old_e)，
                // 再用 old_s/old_e 查冲突。delete_offset_map 仍保留用于 rebase。
                let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
                    let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                        &vt.old_text,
                        &vt.new_text,
                        (rebase_byte_start, rebase_byte_end),
                        (rebase_byte_start, rebase_byte_start),
                    );
                    (Some((old_s, old_e)), Some((new_s, new_e)))
                };
                let (conflict_old_start, conflict_old_end) =
                    visual_affected_byte_range_old.unwrap_or((rebase_byte_start, rebase_byte_end));
                let conflicting = self.prepared_queue.find_conflicting_transaction(
                    &vt.old_text,
                    conflict_old_start,
                    conflict_old_end,
                );
                let now = Instant::now();
                let (rebase_frames, caret_handoff) = self.take_rebase_frames(
                    &conflicting,
                    "rebased_by_delete",
                    now,
                    Some((&deleted_ranges, &delete_offset_map)),
                    &vt.old_text,
                    cursor_owner_epoch,
                );

                let key = self.alloc_key();

                let mut slices = Vec::new();

                // Issue #687: Delete 事务先生成显式 DeleteConceal，再调用 reflow builder；
                // reflow 必须排除 deleted_range。changed range 由 Core 显式拥有。
                // 对每个 deleted range 生成显式 DeleteConceal 切片。
                // Issue #727 评论 5755858583 问题5: !smooth_cursor_enabled 时跳过
                // DeleteConceal（CaretDriven unit），只保留 Reflow。
                if smooth_cursor_enabled {
                    for &(d_start, d_end) in &deleted_ranges {
                        let conceal_slices = build_delete_conceal_slices(
                            key,
                            old_snapshot,
                            (d_start, d_end),
                            old_cursor_rect.as_ref(),
                        );
                        slices.extend(conceal_slices);
                    }
                }

                let reflow_slices = build_cluster_reflow_slices(
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

                let mut units: Vec<PreparedVisualUnit> = slices
                    .into_iter()
                    .map(|s| PreparedVisualUnit::wrap(s, vt.duration_ms))
                    .collect();
                match_rebase_frames(&rebase_frames, &mut units, &delete_offset_map);

                // Issue #690 评论 5681206040 + 5682867529: 构建 caret track（不传 now，等 Rendering 再启动）。
                let cursor_visual_track = build_cursor_visual_track(
                    old_cursor_rect.as_ref(),
                    new_cursor_rect.as_ref(),
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    caret_handoff,
                    vt.duration_ms,
                );
                // Issue #710 评论 5731145076 症状六: visual_affected_byte_range 已在
                // 查冲突之前提前计算（current-old 坐标系逐事务映射需要 old-side range）。
                // Issue #710 评论 5732160521 问题 1/3: Delete 事务 old 侧是 deleted_range，
                // new 侧是删除后落点 (rebase_byte_start, rebase_byte_start)。
                let prepared = PreparedTextVisualTransaction {
                    key,
                    state: TextVisualTransactionState::Pending,
                    operation_kind: TextVisualOperationKind::Delete,
                    timeline: TransactionTimeline::new(vt.duration_ms),
                    units,
                    old_cursor_rect,
                    new_cursor_rect,
                    cursor_visual_track,
                    cancel_reason: None,
                    texture_prepared: false,
                    old_snapshot: Some(old_snapshot.clone()),
                    new_snapshot: Some(new_snapshot.clone()),
                    cursor_owner_epoch,
                    caret_motion_retired: false,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    layout_basis_revision,
                };

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Delete deleted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    deleted_ranges,
                    unit_kind_labels(&prepared.units),
                    rebase_frames.len(),
                ));

                self.prepared_queue.enqueue(prepared);

                return Some(key);
            }
            EditorAnimationKind::Cursor => {
                // Issue #702: 删除"纯光标移动创建空 Cursor 文字事务"的结构。
                // 纯光标移动直接维护 CursorAnimationState（由 rendering.rs
                // update_cursor_visual_position → build_cursor_plan → apply_plan
                // 构造），用 Scene Graph 当前帧 frame_now 推进 from→to 动画，
                // 不再伪装成文字事务（units=空）。
                // 此分支不再创建任何事务，返回 None。
                return None;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::coordinator::LinuxEditorAnimationCoordinator;
    use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
    use crate::sujian_editor_item::edit_motion::CursorRect;
    use crate::sujian_editor_item::layout_snapshot::{LineSnapshotId, ShapingIdentity, SourceRect};
    use crate::sujian_editor_item::render_plan::SelectionPreeditPlan;
    use crate::sujian_editor_item::animation::{
        PreparedTextVisualTransaction, PreparedVisualUnit,
        RebaseFrame, TextVisualOperationKind, TransactionTimeline, VisualUnitTiming,
    };
    use crate::sujian_editor_item::animation::rebase::match_rebase_frames;
    use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
    use writer_core::editor::{OffsetMap, Utf8ByteOffset};

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
            visual_line_id: 0,
            visual_line_top: 0.0,
            visual_line_bottom: 20.0,
            cache_slot: 0,
            qtextline_idx: 0,
            // Issue #724 评论 5752140048 问题 4a: 测试用段落起始偏移 0。
            paragraph_document_byte_start: 0,
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
        EditorLayoutSnapshot::new(
            layout_snapshot,
            vec![line],
            None,
            None,
            CaretAffinity::Downstream,
        )
        .with_virtual_text(virtual_text.to_string())
    }

    /// Issue #686 评论 5667184642：回归测试——吞字方向必须与光标位置匹配。
    ///
    /// `conceal_to_left_edge = true` 表示向左边缘收缩（Backspace，光标在文字右侧）；
    /// `conceal_to_left_edge = false` 表示向右边缘收缩（Delete 键，光标在文字左侧）。
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
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
            Instant::now(),
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
            .units
            .iter()
            .any(|u| u.slice.kind == AnimatedSliceKind::ReflowMove);
        assert!(
            has_move,
            "commit with same shaping but different geometry should create ReflowMove slice"
        );
        assert!(
            tx.units
                .iter()
                .any(|u| !u.slice.static_hidden_document_rects.is_empty()),
            "Move slices should have static_hidden_document_rects"
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
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
            Instant::now(),
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
            .units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::ReflowCrossFade)
            .count();
        assert!(
            crossfade_count >= 2,
            "commit with different shaping should create paired Crossfade slices (old+new), got {}",
            crossfade_count
        );
        assert!(
            tx.units
                .iter()
                .any(|u| !u.slice.static_hidden_document_rects.is_empty()),
            "Crossfade new should have static_hidden_document_rects to prevent double-draw"
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
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
            Instant::now(),
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
            .units
            .iter()
            .filter(|u| u.slice.byte_start == 0 && u.slice.byte_end == 3)
            .map(|u| &u.slice)
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
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
            Instant::now(),
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
            .units
            .iter()
            .filter(|u| u.slice.byte_start >= 3 && u.slice.byte_end <= 10)
            .map(|u| &u.slice)
            .collect();
        assert!(
            !old_preedit_slices.is_empty(),
            "preedit range should have animated slices"
        );
        let new_candidate_slices: Vec<&AnimatedSlice> = tx
            .units
            .iter()
            .filter(|u| u.slice.byte_start >= 3 && u.slice.byte_end <= 5)
            .map(|u| &u.slice)
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
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
            Instant::now(),
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
            .units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::DeleteConceal)
            .map(|u| &u.slice)
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
        let key = coord.handle_composition_update(
            &old_snapshot,
            &new_snapshot,
            0,
            3,
            0,
            3,
            None,
            None,
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
        );
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
            .units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::ReflowCrossFade)
            .count();
        // 1 crossfade_old (old [0,3)) + 2 crossfade_new (new [0,1) + new [4,6)) = 3
        assert_eq!(
            crossfade_count, 3,
            "expected 3 crossfade slices (1 old + 2 new), got {}",
            crossfade_count
        );
        // new cluster [1,4) is inserted text (no old counterpart) → InsertReveal
        let insert_count = tx
            .units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::InsertReveal)
            .filter(|u| u.slice.byte_start == 1 && u.slice.byte_end == 4)
            .count();
        assert_eq!(
            insert_count, 1,
            "inserted cluster [1,4) should produce exactly 1 InsertReveal, got {}",
            insert_count
        );
        // No DeleteConceal for these ranges
        let delete_count = tx
            .units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::DeleteConceal)
            .count();
        assert_eq!(delete_count, 0, "should not produce DeleteConceal");
    }

#[test]
    fn test_delete_conceal_direction_cursor_near_right_is_backspace() {
        let (old_snapshot, _new_snapshot, _offset_map) = make_delete_direction_snapshots();
        let key = VisualTransactionKey::new(1, 1);
        // 旧光标靠近右端 (x=39, right=40) → Backspace → conceal_to_left_edge=true
        let old_cursor = CursorRect {
            x: 39.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        };
        // Issue #687: changed range 由 build_delete_conceal_slices 显式拥有。
        // old cluster [0,3) 是被删除的范围。
        let slices = build_delete_conceal_slices(key, &old_snapshot, (0, 3), Some(&old_cursor));
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
            delete_slices[0].conceal_to_left_edge,
            "cursor near right (x=39, right=40) should be Backspace → conceal_to_left_edge=true"
        );
    }

#[test]
    fn test_delete_conceal_direction_cursor_near_left_is_delete() {
        let (old_snapshot, _new_snapshot, _offset_map) = make_delete_direction_snapshots();
        let key = VisualTransactionKey::new(1, 1);
        // 旧光标靠近左端 (x=11, left=10) → Delete 键 → conceal_to_left_edge=false
        let old_cursor = CursorRect {
            x: 11.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        };
        // Issue #687: changed range 由 build_delete_conceal_slices 显式拥有。
        // old cluster [0,3) 是被删除的范围。
        let slices = build_delete_conceal_slices(key, &old_snapshot, (0, 3), Some(&old_cursor));
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
            !delete_slices[0].conceal_to_left_edge,
            "cursor near left (x=11, left=10) should be Delete → conceal_to_left_edge=false"
        );
    }

}
