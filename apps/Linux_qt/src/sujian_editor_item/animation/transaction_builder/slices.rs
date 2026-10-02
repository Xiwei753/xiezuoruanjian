//! 四类动画事务的 slice 构造：插入浮现、删除隐没、簇回流、输入法上屏交叉淡入。
//!
//! 从 `transaction_builder.rs` 拆出：四个 builder 各自对应一种编辑语义，共用
//! 同一套「先定位受影响 cluster，再按可见比例生成 slice」的骨架，放在一起方便
//! 互相印证口径是否一致。

use super::*;
use crate::sujian_editor_item::animated_slice::IngestBoundaryDriver;

struct ReflowClusterRef {
    line_idx: usize,
    cluster_idx: usize,
    byte_start: usize,
    byte_end: usize,
}

/// Issue #808 评论 5919641249 修改 1: 按 `visual_line_id` 给同一视觉行的
/// InsertReveal/DeleteConceal 计算行级共同吞吐边界（`line_mask_left/right`）。
///
/// 每个 slice 保留自己真实的 source/document rect（不再 union 成一个大 slice，
/// 因此中间保留的字符不会被卷进动画层）；这里只把同组共同的行级 mask extent
/// 写回每个成员，`AnimatedSlice::compute_frame` 再用「行级 boundary ∩ 本 slice rect」
/// 得到自己的 clip。
///
/// 分组键：kind + visual_line_id + is_caret_line + y（DeleteConceal 的非 caret 行
/// 再按 `conceal_to_left_edge` 分方向）。`visual_line_id` 未知（None）或 kind 不是
/// InsertReveal/DeleteConceal 的 slice 不分组，保持构造时的默认 mask（等于自己的 rect）。
///
/// 由 `build_prepared_transaction` 在全部文字 slice 收集完成后统一调用一次；
/// 单个 builder 也各自调用，保证直接使用 builder 的路径同样带有行级 mask。
/// 重复调用是幂等的（mask 每次从成员 rect 重新计算）。
pub(crate) fn assign_shared_line_masks(slices: &mut [AnimatedSlice]) {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..slices.len() {
        let slice = &slices[i];
        if !matches!(
            slice.kind,
            AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal
        ) {
            continue;
        }
        let Some(line_id) = slice.visual_line_id else {
            continue;
        };
        let mut target: Option<usize> = None;
        for (group_idx, members) in groups.iter().enumerate() {
            let head = &slices[members[0]];
            let same_kind = head.kind == slice.kind;
            let same_line = head.visual_line_id == Some(line_id);
            let same_caret_line = head.is_caret_line == slice.is_caret_line;
            let same_y = (line_mask_rect(head).y - line_mask_rect(slice).y).abs() < 0.5;
            let same_direction = slice.kind != AnimatedSliceKind::DeleteConceal
                || head.conceal_to_left_edge == slice.conceal_to_left_edge;
            if same_kind && same_line && same_caret_line && same_y && same_direction {
                target = Some(group_idx);
                break;
            }
        }
        match target {
            Some(group_idx) => groups[group_idx].push(i),
            None => groups.push(vec![i]),
        }
    }

    for members in &groups {
        let mut mask_left = f64::INFINITY;
        let mut mask_right = f64::NEG_INFINITY;
        for &i in members {
            let rect = line_mask_rect(&slices[i]);
            mask_left = mask_left.min(rect.x);
            mask_right = mask_right.max(rect.x + rect.w);
        }
        for &i in members {
            slices[i].line_mask_left = mask_left;
            slices[i].line_mask_right = mask_right;
        }
    }
}

/// 行级 mask 用哪个 document rect 参与分组和 extent 计算：
/// InsertReveal 用新侧位置（to），DeleteConceal 用旧侧位置（from）。
fn line_mask_rect(slice: &AnimatedSlice) -> &SourceRect {
    match slice.kind {
        AnimatedSliceKind::InsertReveal => &slice.to_document_rect,
        _ => &slice.from_document_rect,
    }
}

/// Issue #815 评论 5946701331 问题2: 在**同一份 snapshot 内**按文档 byte 找视觉行序。
///
/// 吞吐路径的行序必须来自同一侧 canonical 快照：InsertReveal 的 slice 来自 new
/// snapshot，DeleteConceal 的 slice 来自 old snapshot。两次排版的 `VisualLine.id`
/// 都从 0 重新编号，混着比大小必然判断反（Backspace 跨行时方向正好相反）。
/// 所以这里返回的是同一份 `line_snapshots` 数组内的下标——切片侧和边界侧都用
/// 同一个函数、同一个数组，行序之间可以直接比较。
///
/// 返回 `line_snapshots` 中的下标（视口内局部序，但两侧都取自同一份快照，
/// 因此自洽）。`byte_offset` 落在所有行之外时返回 `None`（调用方退回同行 x 裁切）。
fn line_ordinal_for_byte(snapshot: &EditorLayoutSnapshot, byte_offset: usize) -> Option<usize> {
    snapshot
        .line_snapshots
        .iter()
        .position(|line| byte_offset >= line.byte_start && byte_offset < line.byte_end)
}

/// Issue #815 评论 5946701331 问题2: 在**同一份 snapshot 内**按视觉行 top 找行序。
///
/// Delete 的吞吐路径起点是"删除前的 old caret 所在视觉行"。caret 的几何在
/// `old_cursor_rect.top` 上，直接按行几何匹配就能落在 old snapshot 自己的行序里，
/// 不需要也不应该借用 new snapshot 的 `visual_line_id`。
///
/// 先按 `[visual_line_top, visual_line_bottom)` 精确命中；未命中时退到 top 最近的行
/// （空行/行高差异下 caret top 可能落在相邻行边界上）。都找不到返回 `None`。
fn line_ordinal_for_line_top(snapshot: &EditorLayoutSnapshot, line_top: f64) -> Option<usize> {
    let hit = snapshot.line_snapshots.iter().position(|line| {
        line_top >= line.visual_line_top - 0.5 && line_top < line.visual_line_bottom + 0.5
    });
    if hit.is_some() {
        return hit;
    }
    snapshot
        .line_snapshots
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da = (a.visual_line_top - line_top).abs();
            let db = (b.visual_line_top - line_top).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(idx, _)| idx)
}

pub(crate) fn build_insert_reveal_slices(
    key: VisualTransactionKey,
    new_snapshot: &EditorLayoutSnapshot,
    inserted_range: (usize, usize),
    old_cursor_rect: Option<&CursorRect>,
    coordinated: bool,
    caret_visual_line_id: Option<usize>,
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    let (range_start, range_end) = inserted_range;
    // Issue #808: 旧 caret 位置是吐字起点（遮罩锚点）。
    let caret_x = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
    let caret_y = old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);
    // Issue #815 评论 5946701331 问题2: 吐字的吞吐路径在 **new snapshot 内**建立。
    // 起点 = inserted_range.start 所在的新侧视觉行（编辑前 caret 在新排版里的行），
    // 终点 = inserted_range.end 所在的新侧视觉行（新 caret 所在行）。
    // 只在 new snapshot 内取行序，不和 old snapshot 的任何行号比较。
    let ingest_from_line_ord = line_ordinal_for_byte(new_snapshot, range_start);
    let ingest_to_line_ord =
        line_ordinal_for_byte(new_snapshot, range_end.saturating_sub(1).max(range_start));

    for (line_ord, new_line) in new_snapshot.line_snapshots.iter().enumerate() {
        for new_cluster in new_line.clusters.iter() {
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
            // Issue #808 评论 5916391891 修改 1+4: 按 coordinated 和 visual_line_id
            // 决定遮罩锚点。coordinated=true 且本行是 caret 所在行时用真实 caret x；
            // 否则用行首 text_left（纯文字动画或跨行其他行从行首展开）。
            let is_caret_line = coordinated
                && caret_visual_line_id.is_none_or(|cid| new_line.visual_line_id == cid);
            let anchor_x = if is_caret_line { caret_x } else { new_doc.x };
            let anchor_y = if is_caret_line { caret_y } else { new_doc.y };
            let mut slice = AnimatedSlice::insert_reveal(
                key,
                new_line.id,
                new_sr.clone(),
                new_doc.clone(),
                anchor_x,
                anchor_y,
                slice_byte_start,
                slice_byte_end,
                Some(new_cluster.shaping_identity.clone()),
                // Issue #722 评论 5749572808 问题1: 传全文视觉行 id，
                // 不是 line_snapshots 的局部数组下标。line_idx 仍用于
                // managed_new_clusters/patches_by_line 的局部索引。
                Some(new_line.visual_line_id),
            );
            slice.is_caret_line = is_caret_line;
            // Issue #815 评论 5946701331 问题1: Insert 的边界就是本帧真实 caret.x
            // （编辑前 caret 向新 caret 展开），不需要单独的吞字边界轨迹。
            slice.ingest_boundary_driver = IngestBoundaryDriver::CaretPosition;
            // Issue #815 评论 5946701331 问题2: 写 new snapshot 内的同侧行序。
            slice.ingest_line_ord = Some(line_ord);
            slice.ingest_from_line_ord = ingest_from_line_ord;
            slice.ingest_to_line_ord = ingest_to_line_ord;
            // Issue #727 评论 5755858583 问题2: 直接在 slice 上写 canonical 独占区域，
            // 不再生成 StaticLinePatch。AnimatedSlice 成为唯一事实源。
            slice.static_hidden_document_rects = vec![new_doc];
            slices.push(slice);
        }
    }

    // Issue #808 评论 5919641249 修改 1: 不再 union 合并成一个大 slice。
    // 同一 visual_line_id 的 InsertReveal 只共享行级共同 mask（line_mask_left/right），
    // 每个 slice 保留自己真实的 source/document rect——中间保留的字符不会被卷进来。
    assign_shared_line_masks(&mut slices);
    slices
}

pub(crate) fn build_delete_conceal_slices(
    key: VisualTransactionKey,
    old_snapshot: &EditorLayoutSnapshot,
    deleted_range: (usize, usize),
    old_cursor_rect: Option<&CursorRect>,
    new_cursor_rect: Option<&CursorRect>,
    coordinated: bool,
    caret_visual_line_id: Option<usize>,
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    let (range_start, range_end) = deleted_range;
    // Issue #808: old caret 用于决定 conceal_to_left_edge（收拢方向），
    // new caret 是吞字终点（遮罩锚点）。
    let old_cx = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
    let new_cx = new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
    let new_cy = new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);
    // Issue #815 评论 5946701331 问题2: 吞字的吞吐路径在 **old snapshot 内**建立。
    // 起点 = 删除前 old caret 所在视觉行（按 old_cursor_rect.top 在 old snapshot
    // 里定位，绝不借用 new snapshot 的 visual_line_id）；终点 = deleted_range.start
    // 所在视觉行。Backspace 跨行时 from > to，方向由符号给出而不是写死"向前走"。
    let ingest_from_line_ord = old_cursor_rect
        .as_ref()
        .and_then(|caret| line_ordinal_for_line_top(old_snapshot, caret.top));
    let ingest_to_line_ord = line_ordinal_for_byte(old_snapshot, range_start);
    // Issue #815 评论 5946701331 问题1: Delete 键的吞字起点 = 被删区间在 old snapshot
    // 内、**本 slice 所在行**的右端。逐行算，不共用事务级最大值——跨行删除时每行
    // 都从自己的右端朝 caret 收拢，不会拿最右一行的右端去裁别的行。
    let deleted_right_by_line: Vec<Option<f64>> = old_snapshot
        .line_snapshots
        .iter()
        .map(|line| {
            line.clusters
                .iter()
                .filter(|cluster| cluster.byte_start < range_end && cluster.byte_end > range_start)
                .map(|cluster| {
                    let rect = line.source_rect_to_document_rect(&cluster.source_rect);
                    rect.x + rect.w
                })
                .fold(None::<f64>, |acc, right| {
                    Some(acc.map_or(right, |prev: f64| prev.max(right)))
                })
        })
        .collect();

    for (line_ord, old_line) in old_snapshot.line_snapshots.iter().enumerate() {
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
                // Issue #808 评论 5916391891 修改 1+4: 按 coordinated 和 visual_line_id
                // 决定遮罩锚点。coordinated=true 且本行是 caret 所在行时用真实 new caret x；
                // 否则用行首/行尾（纯文字动画或跨行其他行向行首/行尾收拢）。
                let is_caret_line = coordinated
                    && caret_visual_line_id.is_none_or(|cid| old_line.visual_line_id == cid);
                let anchor_x = if is_caret_line {
                    new_cx
                } else if conceal_to_left_edge {
                    left
                } else {
                    right
                };
                let anchor_y = if is_caret_line { new_cy } else { old_doc.y };
                let mut slice = AnimatedSlice::delete_conceal(
                    key,
                    old_line.id,
                    old_sr,
                    old_doc,
                    anchor_x,
                    anchor_y,
                    old_cluster.byte_start,
                    old_cluster.byte_end,
                    Some(old_cluster.shaping_identity.clone()),
                    conceal_to_left_edge,
                    // Issue #722 评论 5749572808 问题1: 传全文视觉行 id，
                    // 不是 line_snapshots 的局部数组下标。
                    Some(old_line.visual_line_id),
                );
                slice.is_caret_line = is_caret_line;
                // Issue #815 评论 5946701331 问题1: 用既有 `conceal_to_left_edge`
                // 语义区分两种删除，不把 Backspace 和 Delete 都强行解释成
                // "视觉边界一定等于真实 caret 位置"。
                // - true（caret 在被删文字右侧 → Backspace）：真实 caret track 自己
                //   会往左走，直接驱动吞字边界。
                // - false（caret 在被删文字左侧 → Delete 键）：真实 caret 原地不动，
                //   需要单独的 ingest boundary 从被删区间右端朝 caret 收拢，
                //   不能拿静止 caret 当动画进度（否则第一帧宽度就是 0，动画直接消失）。
                slice.ingest_boundary_driver = if conceal_to_left_edge {
                    IngestBoundaryDriver::CaretPosition
                } else {
                    IngestBoundaryDriver::DeleteForwardBoundary
                };
                slice.ingest_boundary_from_x =
                    deleted_right_by_line.get(line_ord).copied().flatten();
                // Issue #815 评论 5946701331 问题2: 写 old snapshot 内的同侧行序。
                slice.ingest_line_ord = Some(line_ord);
                slice.ingest_from_line_ord = ingest_from_line_ord;
                slice.ingest_to_line_ord = ingest_to_line_ord;
                slices.push(slice);
            }
        }
    }

    // Delete 不生成 StaticLinePatch：删除后的 canonical new text 可以立即作为背景，
    // 旧字只由 overlay 吞掉。
    // Issue #808 评论 5919641249 修改 1: 同 Insert——同一 visual_line_id 的
    // DeleteConceal 只共享行级共同 mask，不 union 成一个大 slice（否则中间保留的
    // cluster 会被卷进旧文字吞字层）。
    assign_shared_line_masks(&mut slices);
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
    visual_affected_byte_range_old: Option<(usize, usize)>,
    visual_affected_byte_range_new: Option<(usize, usize)>,
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
    // Issue #785: visual_affected_byte_range_old/new 限制 reflow 范围——
    // affected range 外的 cluster 不参与 reflow（Enter 后 affected range 外的
    // 正文不生成 ReflowMove）。
    let mut old_refs: Vec<ReflowClusterRef> = Vec::new();
    let mut old_excluded_flags: Vec<bool> = Vec::new();
    for (line_idx, old_line) in old_snapshot.line_snapshots.iter().enumerate() {
        for (cluster_idx, old_cluster) in old_line.clusters.iter().enumerate() {
            // Issue #785: 跳过 affected range 外的 old cluster。
            if let Some((aff_s, aff_e)) = visual_affected_byte_range_old {
                if old_cluster.byte_end <= aff_s || old_cluster.byte_start >= aff_e {
                    continue;
                }
            }
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
            // Issue #785: 跳过 affected range 外的 new cluster。
            if let Some((aff_s, aff_e)) = visual_affected_byte_range_new {
                if new_cluster.byte_end <= aff_s || new_cluster.byte_start >= aff_e {
                    continue;
                }
            }
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

/// Issue #747 评论 5813540976: Composition commit 的 preedit→candidate 形变 slice 构造。
/// 从 composition.rs 移入，保证 composition 只归一化 spec 不自建 slice。
///
/// 仅在 `is_commit && !visual_text_unchanged` 时由 [`build_prepared_transaction`] 调用。
/// 扫描 old preedit clusters 与 new candidate clusters，按 offset_map 配对：
/// - old 未匹配 → DeleteConceal（按 new cursor 收进方向）
/// - old 匹配但 shaping 不同 → ReflowCrossFadeOld
/// - new 未匹配 → InsertReveal（从 old cursor 位置吐出）
/// - new 匹配但 shaping 不同 → ReflowCrossFadeNew
/// - new 匹配且同 shaping 但几何不同 → ReflowMove
pub(crate) fn build_composition_commit_crossfade_slices(
    key: VisualTransactionKey,
    old_snapshot: &EditorLayoutSnapshot,
    new_snapshot: &EditorLayoutSnapshot,
    offset_map: &OffsetMap,
    preedit_byte_start: usize,
    preedit_byte_end: usize,
    candidate_byte_start: usize,
    candidate_byte_end: usize,
    old_cursor_rect: Option<&CursorRect>,
    new_cursor_rect: Option<&CursorRect>,
    // Issue #808 评论 5917296533 问题4: Composition 路径统一协同模式参数。
    // coordinated=false 时 composition 的 reveal/conceal 只走独立文字动画语义；
    // coordinated=true 才启用 caret 空间锚点。
    coordinated: bool,
    caret_visual_line_id: Option<usize>,
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    // Issue #738 评论 5789470425 问题3: CrossFade group id 分配器（事务内唯一）。
    let mut next_crossfade_group_id: u64 = 1;
    let insert_cx = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
    let insert_cy = old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);
    let shrink_x = new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
    let shrink_y = new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

    for old_line in old_snapshot.lines_in_byte_range(preedit_byte_start, preedit_byte_end) {
        for old_cluster in old_line.clusters_in_byte_range(preedit_byte_start, preedit_byte_end) {
            let mapped_new_bs = offset_map.map_old_to_new(old_cluster.byte_start);
            let mapped_new_be = offset_map.map_old_to_new(old_cluster.byte_end);
            let matched_in_new = if let (Some(mbs), Some(mbe)) = (mapped_new_bs, mapped_new_be) {
                new_snapshot.line_snapshots.iter().any(|nl| {
                    nl.clusters
                        .iter()
                        .any(|nc| nc.byte_start == mbs && nc.byte_end == mbe)
                })
            } else {
                false
            };
            if !matched_in_new {
                if let Some(old_sr) = old_line
                    .source_rect_for_byte_range(old_cluster.byte_start, old_cluster.byte_end)
                {
                    let from_doc = old_line.source_rect_to_document_rect(&old_sr);
                    // Issue #686 评论 5666452462：cancel 时 preedit 文字
                    // 走 delete_conceal，按 old rect 两侧与旧光标距离
                    // 决定收进方向：靠近右端 → Backspace → conceal_to_left_edge=true，
                    // 靠近左端 → Delete 键 → conceal_to_left_edge=false。
                    let left = from_doc.x;
                    let right = from_doc.x + from_doc.w;
                    let conceal_to_left_edge = (shrink_x - right).abs() <= (shrink_x - left).abs();
                    // Issue #808 评论 5917296533 问题4: 按 coordinated 和 visual_line_id
                    // 决定遮罩锚点。coordinated=false 时 is_caret_line=false（独立文字动画）；
                    // coordinated=true 且本行是 caret 所在行时 is_caret_line=true。
                    let is_caret_line = coordinated
                        && caret_visual_line_id.is_none_or(|cid| old_line.visual_line_id == cid);
                    let mut slice = AnimatedSlice::delete_conceal(
                        key,
                        old_line.id,
                        old_sr,
                        from_doc,
                        shrink_x,
                        shrink_y,
                        old_cluster.byte_start,
                        old_cluster.byte_end,
                        Some(old_cluster.shaping_identity.clone()),
                        conceal_to_left_edge,
                        // Issue #722 评论 5749791161 问题2: 传真实 visual_line_id
                        Some(old_line.visual_line_id),
                    );
                    slice.is_caret_line = is_caret_line;
                    slices.push(slice);
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
                            let old_doc = old_line.source_rect_to_document_rect(&old_sr);
                            if let Some(new_sr) = new_line.source_rect_for_byte_range(
                                new_cluster.byte_start,
                                new_cluster.byte_end,
                            ) {
                                let new_doc = new_line.source_rect_to_document_rect(&new_sr);
                                let group_id = next_crossfade_group_id;
                                next_crossfade_group_id += 1;
                                slices.push(AnimatedSlice::reflow_crossfade_old(
                                    key,
                                    old_line.id,
                                    old_sr,
                                    old_doc,
                                    new_doc,
                                    old_cluster.byte_start,
                                    old_cluster.byte_end,
                                    Some(old_cluster.shaping_identity.clone()),
                                    Some(group_id),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    for new_line in new_snapshot.lines_in_byte_range(candidate_byte_start, candidate_byte_end) {
        for new_cluster in new_line.clusters_in_byte_range(candidate_byte_start, candidate_byte_end)
        {
            let mapped_old_bs = offset_map.map_new_to_old(new_cluster.byte_start);
            let mapped_old_be = offset_map.map_new_to_old(new_cluster.byte_end);
            let found_in_old = if let (Some(mbs), Some(mbe)) = (mapped_old_bs, mapped_old_be) {
                old_snapshot.line_snapshots.iter().any(|ol| {
                    ol.clusters
                        .iter()
                        .any(|oc| oc.byte_start == mbs && oc.byte_end == mbe)
                })
            } else {
                false
            };
            if !found_in_old {
                if let Some(new_sr) = new_line
                    .source_rect_for_byte_range(new_cluster.byte_start, new_cluster.byte_end)
                {
                    let to_doc = new_line.source_rect_to_document_rect(&new_sr);
                    let to_doc_for_hide = to_doc.clone();
                    // Issue #808 评论 5917296533 问题4: 按 coordinated 和 visual_line_id
                    // 决定遮罩锚点。coordinated=false 时 is_caret_line=false（独立文字动画）；
                    // coordinated=true 且本行是 caret 所在行时 is_caret_line=true。
                    let is_caret_line = coordinated
                        && caret_visual_line_id.is_none_or(|cid| new_line.visual_line_id == cid);
                    let mut reveal_slice = AnimatedSlice::insert_reveal(
                        key,
                        new_line.id,
                        new_sr.clone(),
                        to_doc,
                        insert_cx,
                        insert_cy,
                        new_cluster.byte_start,
                        new_cluster.byte_end,
                        Some(new_cluster.shaping_identity.clone()),
                        // Issue #722 评论 5749791161 问题2: 传真实 visual_line_id
                        Some(new_line.visual_line_id),
                    );
                    reveal_slice.is_caret_line = is_caret_line;
                    reveal_slice.static_hidden_document_rects = vec![to_doc_for_hide];
                    slices.push(reveal_slice);
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
                            let new_doc = new_line.source_rect_to_document_rect(&new_sr);
                            let new_doc_for_hide = new_doc.clone();
                            let group_id = next_crossfade_group_id;
                            let mut new_slice = AnimatedSlice::reflow_crossfade_new(
                                key,
                                new_line.id,
                                new_sr.clone(),
                                old_doc,
                                new_doc,
                                new_cluster.byte_start,
                                new_cluster.byte_end,
                                Some(new_cluster.shaping_identity.clone()),
                                Some(group_id),
                            );
                            new_slice.static_hidden_document_rects = vec![new_doc_for_hide];
                            slices.push(new_slice);
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
                            let old_doc = old_line.source_rect_to_document_rect(&old_sr);
                            let new_doc = new_line.source_rect_to_document_rect(&new_sr);
                            let geometry_same = (old_doc.x - new_doc.x).abs() < 0.5
                                && (old_doc.y - new_doc.y).abs() < 0.5
                                && (old_doc.w - new_doc.w).abs() < 0.5
                                && (old_doc.h - new_doc.h).abs() < 0.5;
                            if !geometry_same {
                                let new_doc_for_hide = new_doc.clone();
                                let mut move_slice = AnimatedSlice::reflow_move(
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
                                );
                                move_slice.static_hidden_document_rects = vec![new_doc_for_hide];
                                slices.push(move_slice);
                            }
                        }
                    }
                }
            }
        }
    }

    // Issue #808 评论 5919641249 修改 1: Composition 多字候选也进入同一套按
    // visual_line_id 的行级共同 mask——但不再通过 union source rect 合并 slice。
    // assign_shared_line_masks 只写共同 mask extent，x/y 各自保留自己的
    // source/document rect；compute_frame 用行级 boundary 与各自 rect 求交。
    // 中间保留（不参与动画）的 cluster 仍由 canonical 静态正文画，动画层不会重画它。
    //
    // merge_adjacent_slices 只剩 ReflowMove/ReflowCrossFade 会命中（can_merge 对
    // InsertReveal/DeleteConceal 返回 false），保持 reflow 的既有相邻合并行为。
    assign_shared_line_masks(&mut slices);
    let slices = merge_adjacent_slices(slices);
    slices
}
