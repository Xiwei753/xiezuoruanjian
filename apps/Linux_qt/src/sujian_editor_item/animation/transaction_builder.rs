use std::time::Instant;

use writer_core::editor::OffsetMap;

use super::coordinator::LinuxEditorAnimationCoordinator;
use crate::editor::layout::compute_affected_paragraph_ranges;
use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::animation::cursor_motion::build_cursor_visual_track;
use crate::sujian_editor_item::animation::rebase::{
    match_rebase_frames, PreparedRebaseHandoff, RebaseCaretHandoff,
};
use crate::sujian_editor_item::animation::{
    PreparedTextVisualTransaction, PreparedVisualUnit, RebaseFrame, TextVisualOperationKind,
    TextVisualTransactionState, TransactionTimeline,
};
use crate::sujian_editor_item::animation_mode::AnimationMode;
use crate::sujian_editor_item::edit_motion::{
    diff_plain_text, CursorRect, EditorAnimationKind, PreparedEditMotion,
};
use crate::sujian_editor_item::editor_animation_debug_log;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{
    ClusterInsertRelation, EditorLayoutSnapshot, SourceRect,
};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

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

pub(crate) fn emit_transaction_diagnostic(
    tx: &PreparedTextVisualTransaction,
    event: &str,
    reason: &str,
) {
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

/// Issue #747 评论 5813540976: Composition commit 特殊 crossfade 规格（preedit→candidate 形变动画）。
/// 仅在 operation_kind == CompositionCommitOrCancel 且 is_commit 且 !visual_text_unchanged 时有值。
pub(crate) struct CompositionCommitCrossfadeSpec {
    pub(crate) preedit_byte_start: usize,
    pub(crate) preedit_byte_end: usize,
    pub(crate) candidate_byte_start: usize,
    pub(crate) candidate_byte_end: usize,
}

/// Issue #747 评论 5805324575 / 5813540976: 统一视觉事务构造的「归一化编辑事件」。
///
/// 所有编辑来源（普通 Insert/Delete、IME composition update/commit）先把原始编辑
/// 状态归一化成 `VisualEditSpec`，再交给 [`build_prepared_transaction`] 这唯一一处
/// 创建 `PreparedTextVisualTransaction`。`composition.rs` 只负责把 IME 状态归一化成
/// `VisualEditSpec` 并调用同一个事务构造器，不再维护第二套事务创建算法。
///
/// Issue #747 评论 5813540976: spec 只携带归一化输入（`offset_map`、cursor line info、
/// `text_animation_enabled` / `caret_animation_enabled`、`composition_commit_crossfade`），
/// 不再携带 `units` 与 `cursor_visual_track`——它们是 builder 的输出，
/// 由 [`build_prepared_transaction`] 内部统一构造。
pub(crate) struct VisualEditSpec {
    pub(crate) key: VisualTransactionKey,
    pub(crate) operation_kind: TextVisualOperationKind,
    pub(crate) old_snapshot: EditorLayoutSnapshot,
    pub(crate) new_snapshot: EditorLayoutSnapshot,
    pub(crate) inserted_ranges: Vec<(usize, usize)>,
    pub(crate) deleted_ranges: Vec<(usize, usize)>,
    pub(crate) offset_map: OffsetMap,
    pub(crate) old_cursor_rect: Option<CursorRect>,
    pub(crate) new_cursor_rect: Option<CursorRect>,
    pub(crate) old_cursor_visual_line_id: Option<usize>,
    pub(crate) new_cursor_visual_line_id: Option<usize>,
    pub(crate) old_cursor_line_top: f64,
    pub(crate) old_cursor_line_bottom: f64,
    pub(crate) new_cursor_line_top: f64,
    pub(crate) new_cursor_line_bottom: f64,
    pub(crate) cursor_owner_epoch: u64,
    pub(crate) layout_basis_revision: LayoutRevision,
    pub(crate) rebase_frames: Vec<RebaseFrame>,
    pub(crate) caret_handoff: Option<RebaseCaretHandoff>,
    pub(crate) visual_affected_byte_range_old: Option<(usize, usize)>,
    pub(crate) visual_affected_byte_range_new: Option<(usize, usize)>,
    pub(crate) unit_duration_ms: u64,
    /// Issue #756: 文字动画开关（ReflowMove/ReflowCrossFade + InsertReveal/DeleteConceal）。
    /// coordinated=true 或 typing_animation_enabled=true 时为 true。
    pub(crate) text_animation_enabled: bool,
    /// Issue #756: 光标动画开关（caret motion track）。
    /// coordinated=true 或 smooth_cursor_enabled=true 时为 true。
    pub(crate) caret_animation_enabled: bool,
    /// Issue #756: 协同动画显式模式。决定吞吐字（InsertReveal/DeleteConceal）是否由
    /// caret 驱动。coordinated=true 时吞吐字走 caret-driven（消费
    /// CoordinatedMotionFrame.caret），coordinated=false 时吞吐字用 typing timeline
    /// 自己推进（不消费 caret frame）。
    pub(crate) coordinated_animation_enabled: bool,
    pub(crate) composition_commit_crossfade: Option<CompositionCommitCrossfadeSpec>,
}

/// Issue #747 评论 5813540976: 全仓库唯一创建 `PreparedTextVisualTransaction` 的完整入口。
///
/// 接收归一化后的 [`VisualEditSpec`]，内部统一完成 slice 构造、unit wrap、rebase 匹配、
/// cursor track 构建、timeline 初始化。其它模块（含 `composition.rs` 与普通 Insert/Delete
/// 路径）都经由本函数创建事务，从而保证「只允许这里创建 `PreparedTextVisualTransaction`」。
pub(crate) fn build_prepared_transaction(spec: VisualEditSpec) -> PreparedTextVisualTransaction {
    let mut slices: Vec<AnimatedSlice> = Vec::new();

    // 1a. InsertReveal / DeleteConceal（文字动画）
    //
    // Issue #756: 吞吐字是否存在由 text_animation_enabled 决定（coordinated || typing）。
    // 吞吐字是否由 caret 驱动由 coordinated_animation_enabled 决定：
    // - coordinated=true：吞吐字走 caret-driven（CaretDriven timing，消费
    //   CoordinatedMotionFrame.caret），文字与光标绑死。
    // - coordinated=false：吞吐字用 typing timeline 自己推进（Timed timing，
    //   compute_frame(visible)），不消费 caret frame。这样 coordinated=false +
    //   typing=true + smooth=false 时仍有吐字（Issue #756 问题 2）。
    if spec.text_animation_enabled {
        for &(i_start, i_end) in &spec.inserted_ranges {
            slices.extend(build_insert_reveal_slices(
                spec.key,
                &spec.new_snapshot,
                (i_start, i_end),
            ));
        }
        for &(d_start, d_end) in &spec.deleted_ranges {
            slices.extend(build_delete_conceal_slices(
                spec.key,
                &spec.old_snapshot,
                (d_start, d_end),
                spec.old_cursor_rect.as_ref(),
            ));
        }
    }

    // 1b. Composition commit 特殊 crossfade（preedit→candidate 形变）
    if let Some(crossfade) = &spec.composition_commit_crossfade {
        slices.extend(build_composition_commit_crossfade_slices(
            spec.key,
            &spec.old_snapshot,
            &spec.new_snapshot,
            &spec.offset_map,
            crossfade.preedit_byte_start,
            crossfade.preedit_byte_end,
            crossfade.candidate_byte_start,
            crossfade.candidate_byte_end,
            spec.old_cursor_rect.as_ref(),
            spec.new_cursor_rect.as_ref(),
        ));
    }

    // 1c. Reflow（unchanged material）
    //
    // Issue #756: Reflow 是文字动画的一部分，由 text_animation_enabled 决定
    //（coordinated=true 或 typing_animation_enabled=true）。typing 关闭且非协同时
    // 只有光标动画，不生成文字 unit。
    let mut excluded_old: Vec<(usize, usize)> = spec.deleted_ranges.clone();
    let mut excluded_new: Vec<(usize, usize)> = spec.inserted_ranges.clone();
    if let Some(crossfade) = &spec.composition_commit_crossfade {
        excluded_old.push((crossfade.preedit_byte_start, crossfade.preedit_byte_end));
        excluded_new.push((crossfade.candidate_byte_start, crossfade.candidate_byte_end));
    }
    if spec.text_animation_enabled {
        slices.extend(build_cluster_reflow_slices(
            spec.key,
            &spec.old_snapshot,
            &spec.new_snapshot,
            &spec.offset_map,
            &excluded_old,
            &excluded_new,
            spec.old_cursor_rect.as_ref(),
            spec.new_cursor_rect.as_ref(),
        ));
    }

    // 2. Wrap units
    //
    // Issue #756: InsertReveal/DeleteConceal 的 timing 由 coordinated_animation_enabled
    // 决定（coordinated=true → CaretDriven，coordinated=false → Timed）。
    // ReflowMove/ReflowCrossFade 永远 Timed，与 coordinated 无关。
    let mut units: Vec<PreparedVisualUnit> = slices
        .into_iter()
        .map(|s| match s.kind {
            AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
                PreparedVisualUnit::wrap_with_coordinated(
                    s,
                    spec.unit_duration_ms,
                    spec.coordinated_animation_enabled,
                )
            }
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                PreparedVisualUnit::wrap(s, spec.unit_duration_ms)
            }
        })
        .collect();

    // 3. Rebase frame 匹配
    match_rebase_frames(&spec.rebase_frames, &mut units, &spec.offset_map);

    // 4. Cursor visual track
    //
    // Issue #756: caret motion track 就是正文编辑期间的光标动画，由
    // caret_animation_enabled 决定（coordinated=true 或 smooth_cursor_enabled=true）。
    // 关闭时本事务不拥有 caret motion，光标位置由 canonical caret 接管（Snap），
    // 不会在用户关掉"平滑光标"后仍然沿 track 滑动。
    let cursor_visual_track = if spec.caret_animation_enabled {
        build_cursor_visual_track(
            spec.old_cursor_rect.as_ref(),
            spec.new_cursor_rect.as_ref(),
            spec.old_cursor_visual_line_id,
            spec.new_cursor_visual_line_id,
            spec.old_cursor_line_top,
            spec.old_cursor_line_bottom,
            spec.new_cursor_line_top,
            spec.new_cursor_line_bottom,
            spec.caret_handoff.clone(),
            spec.unit_duration_ms,
        )
    } else {
        None
    };

    // 5. 诊断日志
    editor_animation_debug_log(&format!(
        "anim_spec: op={:?} units={} inserted={} deleted={} rebased={} handoff={} epoch={} \
         text_anim={} caret_anim={} coordinated_anim={}",
        spec.operation_kind,
        units.len(),
        spec.inserted_ranges.len(),
        spec.deleted_ranges.len(),
        spec.rebase_frames.len(),
        spec.caret_handoff.is_some(),
        spec.cursor_owner_epoch,
        spec.text_animation_enabled,
        spec.caret_animation_enabled,
        spec.coordinated_animation_enabled,
    ));

    // 6. 唯一 PreparedTextVisualTransaction struct literal
    PreparedTextVisualTransaction {
        key: spec.key,
        state: TextVisualTransactionState::Pending,
        operation_kind: spec.operation_kind,
        timeline: TransactionTimeline::new(spec.unit_duration_ms),
        units,
        old_cursor_rect: spec.old_cursor_rect,
        new_cursor_rect: spec.new_cursor_rect,
        cursor_visual_track,
        cancel_reason: None,
        texture_prepared: false,
        old_snapshot: Some(spec.old_snapshot),
        new_snapshot: Some(spec.new_snapshot),
        cursor_owner_epoch: spec.cursor_owner_epoch,
        caret_motion_retired: false,
        coordinated: spec.coordinated_animation_enabled,
        visual_affected_byte_range_old: spec.visual_affected_byte_range_old,
        visual_affected_byte_range_new: spec.visual_affected_byte_range_new,
        layout_basis_revision: spec.layout_basis_revision,
    }
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

    for new_line in new_snapshot.line_snapshots.iter() {
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
                        conceal_to_left_edge,
                        // Issue #722 评论 5749791161 问题2: 传真实 visual_line_id
                        Some(old_line.visual_line_id),
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
                            next_crossfade_group_id += 1;
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
        // Issue #756: 文字动画开关（coordinated || typing）与光标动画开关
        // （coordinated || smooth）由调用方按同一份设置算出，两者互相独立。
        text_animation_enabled: bool,
        caret_animation_enabled: bool,
        // Issue #756: 协同动画显式模式。决定吞吐字是否由 caret 驱动。
        coordinated_animation_enabled: bool,
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
                let inserted_range_tuple = (range_start, range_end);
                // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
                // build_prepared_transaction 内部调 build_insert_reveal_slices / build_cluster_reflow_slices
                // / match_rebase_frames / build_cursor_visual_track 完成全部 slice/unit/track 构造。
                // Issue #687: Insert 事务 changed range 由 Core 显式拥有，reflow 排除 inserted_range。
                // Issue #756: InsertReveal 生成由 text_animation_enabled + caret_animation_enabled 决定，
                // 不再由 smooth_cursor_enabled 单独决定，也不再把 typing && smooth 当成协同。
                // Issue #710 评论 5732160521 问题 1/3: Insert 事务 old 侧是插入点
                // (range_start, range_start)，new 侧是 inserted_range。
                let carried_rebase = rebase_frames.len();
                let spec = VisualEditSpec {
                    key,
                    operation_kind: TextVisualOperationKind::Insert,
                    old_snapshot: old_snapshot.clone(),
                    new_snapshot: new_snapshot.clone(),
                    inserted_ranges: vec![inserted_range_tuple],
                    deleted_ranges: vec![],
                    offset_map: insert_offset_map,
                    old_cursor_rect,
                    new_cursor_rect,
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    cursor_owner_epoch,
                    layout_basis_revision,
                    rebase_frames,
                    caret_handoff,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    unit_duration_ms: vt.duration_ms,
                    text_animation_enabled,
                    caret_animation_enabled,
                    coordinated_animation_enabled,
                    composition_commit_crossfade: None,
                };
                let prepared_tx = build_prepared_transaction(spec);

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared_tx, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Insert inserted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    inserted_range_tuple,
                    unit_kind_labels(&prepared_tx.units),
                    carried_rebase,
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

                // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
                // build_prepared_transaction 内部调 build_cluster_reflow_slices(key, old, new,
                // offset_map, &deleted_ranges, &[], ...) 排除 deleted_range，
                // Issue #687: changed range 由 Core 显式拥有。
                // Issue #756: DeleteConceal 生成由 text_animation_enabled + caret_animation_enabled 决定，
                // 不再由 smooth_cursor_enabled 单独决定，也不再把 typing && smooth 当成协同。
                // Issue #710 评论 5732160521 问题 1/3: Delete 事务 old 侧是 deleted_range，
                // new 侧是删除后落点 (rebase_byte_start, rebase_byte_start)。
                let carried_rebase = rebase_frames.len();
                let deleted_ranges_log = deleted_ranges.clone();
                let spec = VisualEditSpec {
                    key,
                    operation_kind: TextVisualOperationKind::Delete,
                    old_snapshot: old_snapshot.clone(),
                    new_snapshot: new_snapshot.clone(),
                    inserted_ranges: vec![],
                    deleted_ranges,
                    offset_map: delete_offset_map,
                    old_cursor_rect,
                    new_cursor_rect,
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    cursor_owner_epoch,
                    layout_basis_revision,
                    rebase_frames,
                    caret_handoff,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    unit_duration_ms: vt.duration_ms,
                    text_animation_enabled,
                    caret_animation_enabled,
                    coordinated_animation_enabled,
                    composition_commit_crossfade: None,
                };
                let prepared_tx = build_prepared_transaction(spec);

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared_tx, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Delete deleted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    deleted_ranges_log,
                    unit_kind_labels(&prepared_tx.units),
                    carried_rebase,
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
        coordinated_animation_enabled: bool,
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
        // Issue #756: 删除把"两个独立开关同时开启"等价成"协同动画"的逻辑。
        // - coordinated=true 时：文字与光标绑死，要求有效 caret motion，否则不创建事务
        //   （文字动画也不启动）。
        // - coordinated=false 时：typing_animation_enabled 只决定文字动画
        //   （Reflow + InsertReveal/DeleteConceal），smooth_cursor_enabled 只决定光标动画
        //   （caret motion track）。两者互相独立，同时为 true 不等于协同：
        //   只有 coordinated_animation_enabled 才走协同路径。
        let text_animation_enabled = coordinated_animation_enabled || typing_animation_enabled;
        let caret_animation_enabled = coordinated_animation_enabled || smooth_cursor_enabled;
        if (!text_animation_enabled && !caret_animation_enabled)
            || is_scrolling
            || is_loading
            || is_applying_format
        {
            return None;
        }

        // Issue #756: valid_caret_motion_track 检查。
        // - coordinated=true 时：文字和光标绑死，必须有有效 caret motion，否则不创建事务。
        // - coordinated=false 时：不把缺少 caret motion 当成"整笔不播"——文字动画（Reflow）
        //   与 cursor track 各自按自己的开关决定（无 caret motion 时只是没有 CaretDriven
        //   units 与 cursor track，与 Issue #727 约束 5 一致）。
        let valid_caret_motion_track = old_cursor_rect.is_some() && new_cursor_rect.is_some();
        if coordinated_animation_enabled && !valid_caret_motion_track {
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
                    let inserted_range_tuple = (range_start, range_end);
                    // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
                    // build_prepared_transaction 内部调 build_cluster_reflow_slices(key, old, new,
                    // offset_map, &[], &[inserted_range_tuple], ...) 排除 inserted_range，
                    // Issue #687: changed range 由 Core 显式拥有。
                    // Issue #756: InsertReveal 生成由 text_animation_enabled + caret_animation_enabled 决定，
                    // 不再把 typing && smooth 当成协同。
                    // Issue #710 评论 5732160521 问题 1/3: Insert 事务 old 侧是插入点
                    // (range_start, range_start)，new 侧是 inserted_range。
                    let carried_rebase = rebase_frames.len();
                    let spec = VisualEditSpec {
                        key,
                        operation_kind: TextVisualOperationKind::Insert,
                        old_snapshot: old_snapshot.clone(),
                        new_snapshot: new_snapshot.clone(),
                        inserted_ranges: vec![inserted_range_tuple],
                        deleted_ranges: vec![],
                        offset_map: insert_offset_map,
                        old_cursor_rect,
                        new_cursor_rect,
                        old_cursor_visual_line_id,
                        new_cursor_visual_line_id,
                        old_cursor_line_top,
                        old_cursor_line_bottom,
                        new_cursor_line_top,
                        new_cursor_line_bottom,
                        cursor_owner_epoch,
                        layout_basis_revision,
                        rebase_frames,
                        caret_handoff,
                        visual_affected_byte_range_old,
                        visual_affected_byte_range_new,
                        unit_duration_ms: vt.duration_ms,
                        text_animation_enabled,
                        caret_animation_enabled,
                        coordinated_animation_enabled,
                        composition_commit_crossfade: None,
                    };
                    let prepared = build_prepared_transaction(spec);

                    // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                    emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                    editor_animation_debug_log(&format!(
                        "anim_event: key={:?} op=Insert inserted={:?} unit_kinds={:?} carried_rebase={}",
                        key,
                        inserted_range_tuple,
                        unit_kind_labels(&prepared.units),
                        carried_rebase,
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

                // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
                // build_prepared_transaction 内部调 build_cluster_reflow_slices(key, old, new,
                // offset_map, &deleted_ranges, &[], ...) 排除 deleted_range，
                // Issue #687: changed range 由 Core 显式拥有。
                // Issue #756: DeleteConceal 生成由 text_animation_enabled + caret_animation_enabled 决定，
                // 不再由 smooth_cursor_enabled 单独决定，也不再把 typing && smooth 当成协同。
                // Issue #710 评论 5732160521 问题 1/3: Delete 事务 old 侧是 deleted_range，
                // new 侧是删除后落点 (rebase_byte_start, rebase_byte_start)。
                let carried_rebase = rebase_frames.len();
                let spec = VisualEditSpec {
                    key,
                    operation_kind: TextVisualOperationKind::Delete,
                    old_snapshot: old_snapshot.clone(),
                    new_snapshot: new_snapshot.clone(),
                    inserted_ranges: vec![],
                    deleted_ranges: deleted_ranges.clone(),
                    offset_map: delete_offset_map,
                    old_cursor_rect,
                    new_cursor_rect,
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    cursor_owner_epoch,
                    layout_basis_revision,
                    rebase_frames,
                    caret_handoff,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    unit_duration_ms: vt.duration_ms,
                    text_animation_enabled,
                    caret_animation_enabled,
                    coordinated_animation_enabled,
                    composition_commit_crossfade: None,
                };
                let prepared = build_prepared_transaction(spec);

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Delete deleted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    deleted_ranges,
                    unit_kind_labels(&prepared.units),
                    carried_rebase,
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
mod tests;
