//! Linux Qt 文字动画协调器 — 组合编辑（IME composition）方法。
//!
//! `handle_composition_update`、`prepare_composition_commit_handoff`、
//! `handle_composition_commit_or_cancel`、`active_composition_new_snapshot`、
//! `cancel_active_composition`。

use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::editor::layout::compute_affected_paragraph_ranges;
use crate::sujian_editor_item::animation::rebase::PreparedCompositionCommitHandoff;
use crate::sujian_editor_item::animation::transaction_builder::{
    build_prepared_transaction, emit_transaction_diagnostic, unit_kind_labels,
    CompositionCommitCrossfadeSpec, VisualEditSpec,
};
use crate::sujian_editor_item::animation::{TextVisualOperationKind, TextVisualTransactionState};
use crate::sujian_editor_item::edit_motion::{diff_plain_text, CursorRect};
use crate::sujian_editor_item::editor_animation_debug_log;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot;
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

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
        // Issue #756: 动画开关由调用方按同一份设置算出传入。
        text_animation_enabled: bool,
        caret_animation_enabled: bool,
        coordinated_animation_enabled: bool,
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

        // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
        let carried_rebase = rebase_frames.len();
        let spec = VisualEditSpec {
            key,
            operation_kind: TextVisualOperationKind::CompositionUpdate,
            old_snapshot: old_snapshot.clone(),
            new_snapshot: new_snapshot.clone(),
            inserted_ranges: comp_inserted_ranges,
            deleted_ranges: comp_deleted_ranges,
            offset_map,
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
            unit_duration_ms: u64::from(self.typing_animation_duration_ms),
            // Issue #756: composition 路径由调用方传入动画开关，不再硬编码 true。
            text_animation_enabled,
            caret_animation_enabled,
            coordinated_animation_enabled,
            composition_commit_crossfade: None,
        };
        let prepared = build_prepared_transaction(spec);

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
        // Issue #756: 动画开关由调用方按同一份设置算出传入。
        text_animation_enabled: bool,
        caret_animation_enabled: bool,
        coordinated_animation_enabled: bool,
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

        // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
        // composition commit/cancel 的 slice 构造（DeleteConceal/InsertReveal/ReflowCrossFade/
        // ReflowMove/reflow）全部由 build_prepared_transaction 内部统一完成。
        //
        // 三种情况：
        // - cancel（!is_commit）：deleted_ranges = preedit range，DeleteConceal 由 1a 生成，
        //   reflow 排除 preedit（old 侧）。
        // - commit 且 visual_text_unchanged：无 changed range，无 crossfade，reflow 处理全部。
        // - commit 且 !visual_text_unchanged：crossfade slice builder 统一构造 preedit→candidate
        //   形变（DeleteConceal/InsertReveal/ReflowCrossFade/ReflowMove），reflow 排除
        //   preedit（old）和 candidate（new）。inserted/deleted ranges 留空以避免 1a 与
        //   crossfade builder 重复生成 Reveal/Conceal。
        let (inserted_ranges, deleted_ranges, composition_commit_crossfade) = if !is_commit {
            // Issue #687: cancel 时显式生成 DeleteConceal for preedit 范围的 old cluster，
            // reflow 只处理 unchanged material。changed range 由显式函数拥有。
            (vec![], vec![(preedit_byte_start, preedit_byte_end)], None)
        } else if visual_text_unchanged {
            (vec![], vec![], None)
        } else {
            (
                vec![],
                vec![],
                Some(CompositionCommitCrossfadeSpec {
                    preedit_byte_start,
                    preedit_byte_end,
                    candidate_byte_start,
                    candidate_byte_end,
                }),
            )
        };

        // Issue #710 评论 5734282079: composition commit/cancel 的 visual affected range。
        // 不再用保守大区间 min/max，而是分别从 old preedit range（old virtualText 坐标）
        // 和 new-side range（commit: candidate_byte_range / cancel: committed_replace_range）
        // 扩段落得到。
        let carried_rebase = rebase_frames.len();
        let spec = VisualEditSpec {
            key,
            operation_kind: TextVisualOperationKind::CompositionCommitOrCancel,
            old_snapshot: old_snapshot.clone(),
            new_snapshot: new_snapshot.clone(),
            inserted_ranges,
            deleted_ranges,
            offset_map,
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
            unit_duration_ms: u64::from(self.typing_animation_duration_ms),
            // Issue #756: composition 路径由调用方传入动画开关，不再硬编码 true。
            text_animation_enabled,
            caret_animation_enabled,
            coordinated_animation_enabled,
            composition_commit_crossfade,
        };
        let prepared = build_prepared_transaction(spec);

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
