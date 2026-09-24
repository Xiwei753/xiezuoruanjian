//! Linux Qt 文字动画协调器 — 组合编辑（IME composition）方法。
//!
//! `handle_composition_update`、`prepare_composition_commit_handoff`、
//! `handle_composition_commit_or_cancel`、`active_composition_new_snapshot`、
//! `cancel_active_composition`。

use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::animated_slice::AnimatedSlice;
use crate::sujian_editor_item::edit_motion::{diff_plain_text, CursorRect};
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{EditorLayoutSnapshot, SourceRect};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
use crate::sujian_editor_item::animation::PreparedVisualUnit;
use crate::sujian_editor_item::animation::{
    TextVisualOperationKind, TextVisualTransactionState,
};
use crate::sujian_editor_item::animation::rebase::{
    match_rebase_frames, PreparedCompositionCommitHandoff,
};
use crate::sujian_editor_item::animation::cursor_motion::build_cursor_visual_track;
use crate::sujian_editor_item::animation::transaction_builder::{
    assemble_prepared_transaction, build_cluster_reflow_slices, build_delete_conceal_slices,
    build_insert_reveal_slices, emit_transaction_diagnostic, unit_kind_labels, VisualEditSpec,
};
use crate::editor::layout::compute_affected_paragraph_ranges;
use crate::sujian_editor_item::editor_animation_debug_log;

use super::coordinator::LinuxEditorAnimationCoordinator;

impl LinuxEditorAnimationCoordinator {
    pub fn handle_composition_update(
        &mut self,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        old_preedit_byte_start: usize,
        old_preedit_byte_end: usize,
        new_preedit_byte_start: usize,
        new_preedit_byte_end: usize,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_cursor_visual_line_id: Option<usize>,
        new_cursor_visual_line_id: Option<usize>,
        old_cursor_line_top: f64,
        old_cursor_line_bottom: f64,
        new_cursor_line_top: f64,
        new_cursor_line_bottom: f64,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> Option<VisualTransactionKey> {
        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
        // Issue #710 评论 5734282079: 冲突检测用 current-old 坐标系。
        // old_preedit_byte_start/end 是 update_preedit 之前的 old virtualText 坐标，
        // 传 &old_snapshot.virtual_text 作为 current_old_text。offset_map 仍保留用于 rebase。
        let conflicting = self.prepared_queue.find_conflicting_transaction(
            &old_snapshot.virtual_text,
            old_preedit_byte_start,
            old_preedit_byte_end,
        );
        // 预输入文本整体被替换，旧单元必然失效：不做保留判断。
        let now = Instant::now();
        let (rebase_frames, caret_handoff) = self.take_rebase_frames(
            &conflicting,
            "rebased_by_composition_update",
            now,
            None,
            &old_snapshot.virtual_text,
            cursor_owner_epoch,
        );

        let key = self.alloc_key();

        let mut slices = Vec::new();

        // Issue #687: IME 组合更新也显式拥有 changed range。
        // 用 diff_plain_text 找到 inserted/deleted range，显式生成 InsertReveal/DeleteConceal，
        // reflow 只处理 unchanged material。
        let comp_changes = diff_plain_text(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
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
            let reveal_slices = build_insert_reveal_slices(key, new_snapshot, (i_start, i_end));
            slices.extend(reveal_slices);
        }
        for &(d_start, d_end) in &comp_deleted_ranges {
            let conceal_slices = build_delete_conceal_slices(
                key,
                old_snapshot,
                (d_start, d_end),
                old_cursor_rect.as_ref(),
            );
            slices.extend(conceal_slices);
        }

        let reflow_slices = build_cluster_reflow_slices(
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

        let unit_duration_ms = u64::from(self.typing_animation_duration_ms);
        let mut units: Vec<PreparedVisualUnit> = slices
            .into_iter()
            .map(|s| PreparedVisualUnit::wrap(s, unit_duration_ms))
            .collect();
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

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
            caret_handoff.clone(),
            unit_duration_ms,
        );
        // Issue #710 评论 5734282079: composition update 的 visual affected range。
        // old_preedit_byte_start/end 是 old virtualText 坐标，new_preedit_byte_start/end
        // 是 new virtualText 坐标。分别从对应 snapshot 扩段落得到 affected range。
        let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
            let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                &old_snapshot.virtual_text,
                &new_snapshot.virtual_text,
                (old_preedit_byte_start, old_preedit_byte_end),
                (new_preedit_byte_start, new_preedit_byte_end),
            );
            (Some((old_s, old_e)), Some((new_s, new_e)))
        };
        // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
        let carried_rebase = rebase_frames.len();
        let prepared = assemble_prepared_transaction(VisualEditSpec {
            key,
            operation_kind: TextVisualOperationKind::CompositionUpdate,
            old_snapshot: old_snapshot.clone(),
            new_snapshot: new_snapshot.clone(),
            inserted_ranges: comp_inserted_ranges,
            deleted_ranges: comp_deleted_ranges,
            old_cursor_rect,
            new_cursor_rect,
            cursor_owner_epoch,
            layout_basis_revision,
            rebase_frames,
            caret_handoff,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
            units,
            cursor_visual_track,
            unit_duration_ms,
        });

        // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
        emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
        editor_animation_debug_log(&format!(
            "anim_event: key={:?} op=CompositionUpdate unit_kinds={:?} carried_rebase={}",
            key,
            unit_kind_labels(&prepared.units),
            carried_rebase,
        ));

        self.prepared_queue.enqueue(prepared);
        Some(key)
    }

    /// Issue #738 评论 5798704669 问题1: composition commit prepare 阶段——
    /// 在旧 CompositionUpdate 仍活着时采样 rebase frames + caret handoff。
    ///
    /// 用外层传入的统一 `now` 采样，旧事务还活着，采到的是真实当前帧
    ///（CaretDriven 还没被推到终态）。`take_rebase_frames` 自己 cancel
    /// 被覆盖的旧 composition transaction。
    ///
    /// 返回 `PreparedCompositionCommitHandoff` 供后续
    /// `handle_composition_commit_or_cancel`
    /// 创建新事务使用。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_composition_commit_handoff(
        &mut self,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        preedit_byte_start: usize,
        preedit_byte_end: usize,
        is_commit: bool,
        candidate_byte_start: usize,
        candidate_byte_end: usize,
        committed_replace_start: usize,
        committed_replace_end: usize,
        cursor_owner_epoch: u64,
        now: Instant,
    ) -> PreparedCompositionCommitHandoff {
        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
        // Issue #710 评论 5734282079: 不再把 committed_replace 坐标和 preedit virtualText 坐标 min/max。
        let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
            let new_edit_range = if is_commit {
                (candidate_byte_start, candidate_byte_end)
            } else {
                (committed_replace_start, committed_replace_end)
            };
            let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                &old_snapshot.virtual_text,
                &new_snapshot.virtual_text,
                (preedit_byte_start, preedit_byte_end),
                new_edit_range,
            );
            (Some((old_s, old_e)), Some((new_s, new_e)))
        };
        let (conflict_old_start, conflict_old_end) =
            visual_affected_byte_range_old.unwrap_or((preedit_byte_start, preedit_byte_end));
        let conflicting = self.prepared_queue.find_conflicting_transaction(
            &old_snapshot.virtual_text,
            conflict_old_start,
            conflict_old_end,
        );
        let (rebase_frames, caret_handoff) = self.take_rebase_frames(
            &conflicting,
            "rebased_by_composition_commit",
            now,
            None,
            &old_snapshot.virtual_text,
            cursor_owner_epoch,
        );
        PreparedCompositionCommitHandoff {
            rebase_frames,
            caret_handoff,
            offset_map,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
        }
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
        old_cursor_visual_line_id: Option<usize>,
        new_cursor_visual_line_id: Option<usize>,
        old_cursor_line_top: f64,
        old_cursor_line_bottom: f64,
        new_cursor_line_top: f64,
        new_cursor_line_bottom: f64,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
        now: Instant,
        prepared_handoff: Option<PreparedCompositionCommitHandoff>,
    ) -> Option<VisualTransactionKey> {
        // Issue #738 评论 5798704669 问题1: 若外层已调 prepare_composition_commit_handoff
        // 采好 handoff（commit 路径），直接用；否则内部 prepare（cancel 路径 / 旧调用方）。
        let handoff = match prepared_handoff {
            Some(h) => h,
            None => self.prepare_composition_commit_handoff(
                old_snapshot,
                new_snapshot,
                preedit_byte_start,
                preedit_byte_end,
                is_commit,
                candidate_byte_start,
                candidate_byte_end,
                committed_replace_start,
                committed_replace_end,
                cursor_owner_epoch,
                now,
            ),
        };
        let PreparedCompositionCommitHandoff {
            rebase_frames,
            caret_handoff,
            offset_map,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
        } = handoff;

        let key = self.alloc_key();

        let mut slices = Vec::new();
        // Issue #738 评论 5789470425 问题3: CrossFade group id 分配器（事务内唯一）。
        let mut next_crossfade_group_id: u64 = 1;

        if !is_commit {
            // Issue #687: cancel 时显式生成 DeleteConceal for preedit 范围的 old cluster，
            // reflow 只处理 unchanged material。changed range 由显式函数拥有。
            let cancel_deleted_range = (preedit_byte_start, preedit_byte_end);
            let conceal_slices = build_delete_conceal_slices(
                key,
                old_snapshot,
                cancel_deleted_range,
                old_cursor_rect.as_ref(),
            );
            slices.extend(conceal_slices);

            let cancel_excluded_old: [(usize, usize); 1] = [cancel_deleted_range];
            let reflow_slices = build_cluster_reflow_slices(
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
                                // 走 delete_conceal，按 old rect 两侧与旧光标距离
                                // 决定收进方向：靠近右端 → Backspace → conceal_to_left_edge=true，
                                // 靠近左端 → Delete 键 → conceal_to_left_edge=false。
                                let left = from_doc.x;
                                let right = from_doc.x + from_doc.w;
                                let conceal_to_left_edge =
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
                                        let old_doc =
                                            old_line.source_rect_to_document_rect(&old_sr);
                                        if let Some(new_sr) = new_line.source_rect_for_byte_range(
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                        ) {
                                            let new_doc =
                                                new_line.source_rect_to_document_rect(&new_sr);
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
                                        let new_doc =
                                            new_line.source_rect_to_document_rect(&new_sr);
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
                                        new_slice.static_hidden_document_rects =
                                            vec![new_doc_for_hide];
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
                                        let old_doc =
                                            old_line.source_rect_to_document_rect(&old_sr);
                                        let new_doc =
                                            new_line.source_rect_to_document_rect(&new_sr);
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
                                            move_slice.static_hidden_document_rects =
                                                vec![new_doc_for_hide];
                                            slices.push(move_slice);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let reflow_slices = build_cluster_reflow_slices(
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
            }
        }

        let unit_duration_ms = u64::from(self.typing_animation_duration_ms);
        let mut units: Vec<PreparedVisualUnit> = slices
            .into_iter()
            .map(|s| PreparedVisualUnit::wrap(s, unit_duration_ms))
            .collect();
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

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
            caret_handoff.clone(),
            unit_duration_ms,
        );
        // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
        let carried_rebase = rebase_frames.len();
        let prepared = assemble_prepared_transaction(VisualEditSpec {
            key,
            operation_kind: TextVisualOperationKind::CompositionCommitOrCancel,
            old_snapshot: old_snapshot.clone(),
            new_snapshot: new_snapshot.clone(),
            inserted_ranges: vec![(candidate_byte_start, candidate_byte_end)],
            deleted_ranges: vec![(preedit_byte_start, preedit_byte_end)],
            old_cursor_rect,
            new_cursor_rect,
            cursor_owner_epoch,
            layout_basis_revision,
            rebase_frames,
            caret_handoff,
            // Issue #710 评论 5734282079: composition commit/cancel 的 visual affected range。
            // 不再用保守大区间 min/max，而是分别从 old preedit range（old virtualText 坐标）
            // 和 new-side range（commit: candidate_byte_range / cancel: committed_replace_range）
            // 扩段落得到。
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
            units,
            cursor_visual_track,
            unit_duration_ms,
        });

        // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
        emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
        editor_animation_debug_log(&format!(
            "anim_event: key={:?} op=CompositionCommitOrCancel unit_kinds={:?} carried_rebase={}",
            key,
            unit_kind_labels(&prepared.units),
            carried_rebase,
        ));

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
}

