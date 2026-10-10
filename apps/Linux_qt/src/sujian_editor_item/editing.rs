use super::edit_flow::{CompositionCommitParams, EditOp};
use super::layout_revision::LayoutRevision;
use super::*;
use crate::editor::input::events::ImeReplaceEvent;

impl SujianEditorItem {
    pub(crate) fn flush_content_height(&mut self) {
        if self.content_height_dirty.get() {
            self.content_height_dirty.set(false);
            self.content_height_changed();
        }
    }

    /// Issue #690 评论 5675007226 步骤 4: 补完整绘制生命周期。
    ///
    /// `tick_blink()` 返回 `blink_changed=true` 时，和 `still_animating=true` 一样
    /// 必须请求 item 更新，否则空正文光标闪烁切到不可见后，下一次切回可见未必触发
    /// Scene Graph 重绘，表现成"莫名其妙消失"。
    ///
    /// Issue #690 评论 5679744253 问题 2: CursorOnly 位置动画已由 `update_paint_node`
    /// 的 `frame_now` 驱动，本函数只负责 blink。QML 265ms Timer 不再推进位置，
    /// 避免 CursorOnly 被低频 blink Timer 降成 ~4Hz。
    pub(crate) fn tick_cursor_animation(&mut self) {
        // Issue #710 评论 5732160521 问题 2: 统一 blink 决策入口。
        // 之前 tick_cursor_animation / build_cursor_render_state_for_frame /
        // cursor_blink_opacity / 边沿 reset 各自判断，且条件不一致：
        // tick 用 has_active_text_transaction() || has_cursor_only_tween，
        // render/opacity 用 has_active_insert()。快速点击时只有 CursorOnly
        // Tween（无 Insert），tick 认为 Suppressed（常亮），render 认为 Normal
        //（正常 blink），opacity 可能为 0 → 光标消失。
        // 现在统一消费 current_cursor_blink_mode()，保证四处一致。
        let blink_mode = self.current_cursor_blink_mode();
        // Issue #690 评论 5679744253 问题 2: CursorOnly 位置动画已由 update_paint_node 的
        // frame_now 驱动，本函数只负责 blink。QML 265ms Timer 不再推进位置。
        let blink_changed = self.cursor_ctrl.tick_blink(blink_mode);
        if blink_changed {
            self.cursor_rect_changed();
            self.request_frame_update();
        }
    }

    pub(crate) fn clear_undo_stack(&mut self) {
        self.pipeline.clear_undo_redo();
    }

    /// composition commit/cancel 的视觉过渡入口。
    ///
    /// 固定顺序：
    /// 1. 拿 committed old layout snapshot（preedit 期间的正文几何）；
    /// 2. 用 Core 已提交的 new text 排一次版，拿到 committed new snapshot；
    /// 3. 交给 `handle_composition_commit_or_cancel` 从最近成功绘制帧追到新正文；
    /// 4. 无条件提交 `layout_revision` + `current_canonical_snapshot`。
    ///
    /// preedit 临时层独立显示，不 carry/rebase 到正文；commit 时它整体消失。
    /// 正文过渡只认 Core `display_patches`。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_composition_commit_transaction(
        &mut self,
        old: &EditorSnapshot,
        new: &EditorSnapshot,
        result: &writer_core::editor::EditorEditResult,
        cause: EditorTransactionCause,
        preedit_byte_start: usize,
        preedit_byte_end: usize,
        saved_virtual_text: &str,
        candidate_byte_start: usize,
        candidate_byte_end: usize,
        cancel_reason: &str,
        summary_tag: &str,
    ) -> super::pipeline::VisualPrepareOutcome {
        let width = self.bounding_width();
        // Issue #710 评论 5734666497: old snapshot 只接 old virtualText range
        // （preedit 在 old virtualText 中的范围）；new snapshot 只接 new committed
        // text range。视觉提取范围必须扩到段落边界，纯删除（candidate 是零长度）
        // 也要有非空视觉范围。
        let (old_affected_start, old_affected_end, new_affected_start, new_affected_end) =
            crate::editor::layout::compute_affected_paragraph_ranges(
                saved_virtual_text,
                &new.text,
                (preedit_byte_start, preedit_byte_end),
                (candidate_byte_start, candidate_byte_end),
            );
        let old_composition_range = Some((old_affected_start, old_affected_end));
        let new_composition_range = Some((new_affected_start, new_affected_end));
        let candidate_range = (candidate_byte_start, candidate_byte_end);

        // Issue #810 评论 5933167246: build_editor_layout_snapshot 返回 Result，
        // Err 时记诊断并结束本次 commit，不伪装成功。
        let old_snapshot = self.pipeline.current_layout_snapshot().clone().or_else(|| {
            match self.build_editor_layout_snapshot(width, false, old_composition_range) {
                Ok(snap) => Some(snap),
                Err(err) => {
                    crate::backend::app_backend::debug_error_static(
                        "editing",
                        "record_composition_commit_old_snapshot_invariant_failure",
                        &format!(
                            "{} — aborting composition commit (Issue #810 评论 5933167246)",
                            err
                        ),
                    );
                    None
                }
            }
        });
        let Some(old_snapshot) = old_snapshot else {
            record_composition_commit_skip(
                self,
                "composition_commit_old_snapshot_unavailable",
                Some(candidate_range),
            );
            return super::pipeline::VisualPrepareOutcome::Skipped(
                super::edit_flow::EditVisualSkipReason::CompositionCommitSnapshotUnavailable,
            );
        };

        let change_count = super::edit_motion::diff_plain_text(&old.text, &new.text).len();

        // Issue #810 评论 5934658350: composition commit 在 Core 已提交 new text 后调用，
        // 此时 pipeline.text_revision() 仍是 emit 前的 old revision。new canonical 必须带
        // next text revision，与即将 emit_content_changed bump 后的正文 revision 对齐。
        let canonical_text_revision = self.pipeline.text_revision().wrapping_add(1);
        let (new_snapshot, new_canonical) = match self.build_editor_layout_snapshot_with_canonical(
            width,
            true,
            new_composition_range,
            canonical_text_revision,
        ) {
            Ok(pair) => pair,
            Err(err) => {
                crate::backend::app_backend::debug_error_static(
                    "editing",
                    "record_composition_commit_new_snapshot_invariant_failure",
                    &format!(
                        "{} — aborting composition commit (Issue #810 评论 5933167246)",
                        err
                    ),
                );
                record_composition_commit_skip(
                    self,
                    "composition_commit_new_snapshot_invariant_failure",
                    Some(candidate_range),
                );
                return super::pipeline::VisualPrepareOutcome::Skipped(
                    super::edit_flow::EditVisualSkipReason::CompositionCommitSnapshotUnavailable,
                );
            }
        };

        // Issue #826: 正文动画只认 Core display_patches。`from_edit_result` 已经把它
        // 派生成 inserted/deleted ranges（正文动画分类的唯一事实源）。
        let motion =
            super::edit_motion::PreparedEditMotion::from_edit_result(result, &old.text, &new.text);

        let new_revision = LayoutRevision::next();
        let edit_now = std::time::Instant::now();

        let outcome = self
            .pipeline
            .animation_coordinator_mut()
            .handle_composition_commit_or_cancel(
                super::animation::composition::CompositionVisualEditInput {
                    motion,
                    old_snapshot: old_snapshot.clone(),
                    new_snapshot: new_snapshot.clone(),
                    now: edit_now,
                },
            );
        let visual_outcome = super::pipeline::VisualPrepareOutcome::Created;
        // 旧行纹理由最近成功绘制帧引用，保留到新的帧成功提交后再裁掉。
        let active_ids = self
            .pipeline
            .animation_coordinator()
            .collect_active_snapshot_ids();
        self.pipeline.retain_active_snapshot_ids(&active_ids);
        self.prepare_visual_edit_textures(&old_snapshot);

        // Issue #738 评论 5797637204: 无条件提交 Pipeline.layout_revision +
        // current_canonical_snapshot。new_canonical 一旦成为当前 canonical，
        // layout_revision 就必须无条件一起提交。
        self.pipeline.set_layout_revision(new_revision);
        self.pipeline
            .set_current_canonical_snapshot(Some(new_canonical));
        self.pipeline
            .set_previous_layout_snapshot(Some(old_snapshot));
        self.pipeline
            .set_current_layout_snapshot(Some(new_snapshot));

        self.last_event_count = 1;
        self.last_summary = format!(
            "cause={:?};changes={};visual_edit=1;animate=true",
            cause, change_count,
        )
        .into();
        editor_animation_debug_log(&format!(
            "record_composition_commit_transaction: cancel_reason={}, cause={:?}, changes={}",
            cancel_reason, cause, change_count,
        ));
        editor_animation_debug_log(&format!(
            "record_composition_commit_transaction: summary_tag={}, created={}",
            summary_tag,
            matches!(
                visual_outcome,
                super::pipeline::VisualPrepareOutcome::Created
            ),
        ));
        self.transaction_created();
        // Issue #819 评论 5968931455 问题 2.2: 返回 VisualPrepareOutcome，
        // 透传 skip reason，不再让 edit_flow.rs 猜。
        let _ = outcome;
        visual_outcome
    }

    /// Issue #810 评论 5934060933 问题1: 在真正调用 Core edit command 之前保证
    /// old/current canonical 已建立且属于当前 text revision。
    ///
    /// 调用顺序是 `old = snapshot -> Core edit -> new = snapshot -> record_transaction
    /// -> prepare_edit_motion`。进入 prepare_edit_motion 时 `self.mirror.text()` 已是
    /// new text，此时再补 canonical 拿到的是 new canonical 却被当 old canonical 使用。
    ///
    /// 本 helper 在 Core edit 之前调用，此时 `mirror.text()` 是 old text、
    /// `text_revision` 是 old revision，构造的 canonical 描述 old 正文、记录 old revision。
    ///
    /// 不变量：`current_canonical_snapshot == Some` 不代表有效；必须同时满足
    /// `canonical.text_revision == pipeline.text_revision()` 且描述当前 committed text。
    /// 不一致（stale，例如动画关闭路径上一笔没走 prepare_edit_motion 但正文已变）就重建。
    pub(crate) fn ensure_current_canonical_before_edit(&mut self) {
        let needs_rebuild = match self.pipeline.current_canonical_snapshot() {
            None => true,
            Some(canonical) => canonical.text_revision != self.pipeline.text_revision(),
        };
        if needs_rebuild {
            let ctx = self.build_visual_transaction_context();
            let snap = self
                .pipeline
                .build_canonical_snapshot_for_current_layout(&ctx, &self.editor_layout);
            self.pipeline.set_current_canonical_snapshot(Some(snap));
        }
    }

    pub(crate) fn insert_text(&mut self, text: QString) {
        self.insert_text_with_cause(text, None);
    }

    pub(crate) fn insert_text_with_cause(
        &mut self,
        text: QString,
        explicit_cause: Option<EditorTransactionCause>,
    ) {
        if !self.current_editor_enabled {
            return;
        }
        let inserted = normalize_plain_text(&text.to_string());
        if inserted.is_empty() {
            return;
        }

        let (preedit_byte_start, preedit_byte_end) = self.preedit_byte_range_in_virtual_text();
        let commit = self.pipeline.prepare_composition_commit(
            &inserted,
            self.pipeline.cursor(),
            preedit_byte_start,
            preedit_byte_end,
        );

        // visual_cause 用于视觉事务分类；pipeline_cause（在 EditOp 内）用于 Core
        // undo/redo 栈分类。clipboard_paste 走此入口时 visual_cause 是 Paste，
        // pipeline_cause 仍是 Typing/TypingCommit。
        let visual_cause = explicit_cause.unwrap_or_else(|| {
            if inserted.chars().count() == 1 {
                EditorTransactionCause::Typing
            } else {
                EditorTransactionCause::TypingCommit
            }
        });

        // composition commit 仅在 was_composing 且动画开启时走 composition 专属路径；
        // 否则走普通 record_transaction，与普通输入/删除同一种 VisualTransaction。
        // Issue #756: composition commit follows the shared effective animation policy.
        let composition = if commit.was_composing
            && super::animation::any_animation_enabled(
                self.current_typing_animation_enabled,
                self.current_smooth_cursor_enabled,
                self.current_coordinated_animation_enabled,
            ) {
            Some(CompositionCommitParams {
                preedit_byte_start: commit.preedit_byte_start,
                preedit_byte_end: commit.preedit_byte_end,
                saved_virtual_text: commit.saved_virtual_text.clone(),
                candidate_byte_start: commit.candidate_byte_start,
                candidate_byte_end: commit.candidate_byte_end,
                cancel_reason: "commit_insert",
                summary_tag: "composition_commit",
            })
        } else {
            None
        };

        // 构造 EditOp。先读出 committed 投影状态和 commit 的 session replace range，
        // 避免 commit 被 composition 消耗后无法访问。
        let was_composing_replace =
            commit.was_composing && commit.session_replace_start != commit.session_replace_end;
        let session_replace_start = commit.session_replace_start;
        let session_replace_end = commit.session_replace_end;
        let cursor = self.pipeline.cursor();
        let (sel_start, sel_end) = self.pipeline.selection_range();

        let op = if was_composing_replace {
            EditOp::Replace {
                start: session_replace_start,
                end: session_replace_end,
                text: inserted,
                pipeline_cause: EditorTransactionCause::TypingCommit,
            }
        } else if sel_start != sel_end {
            EditOp::Replace {
                start: sel_start,
                end: sel_end,
                text: inserted,
                pipeline_cause: EditorTransactionCause::Typing,
            }
        } else {
            EditOp::Insert {
                cursor,
                text: inserted,
                pipeline_cause: EditorTransactionCause::Typing,
            }
        };

        // Issue #819 评论 5956495850 第 1 节: 普通输入与 IME commit 共用
        // apply_edit_with_visuals 统一入口。insert_text_with_cause 总是
        // emit_content_changed（保持原行为，即使 pipeline edit 未应用）。
        let _outcome = self.apply_edit_with_visuals(op, visual_cause, composition);

        self.pipeline.finish_composition_commit();

        // Issue #701 评论 5699573227 第三阶段 (F2/F7): 不再把
        // pending_preedit_cursor_rect 反写到 cursor_ctrl.visual_x/visual_y。
        // pending_preedit_cursor_rect 只作为 IME commit 动画的 old caret 起点
        // （已传给 record_composition_commit_transaction 的 old_cursor_rect）。
        // 提交后的 target caret 来自 new selection/head 在 new layout 中的 caret,
        // 由 emit_content_changed → update_cursor_visual_position 统一计算。
        // visual_x/visual_y 只是屏幕动画位置，不与 target 互相反写。

        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::TextTransaction;
        self.emit_content_changed();
    }

    /// Issue #701 评论 5702675971: IME replace+commit — 接收 Qt 两步语义的
    /// `ImeReplaceEvent`，携带 `selection_byte_range` 和
    /// `replacement_byte_range_after_selection`。
    ///
    /// 事件由 `platform_ime` 结合当前 `CompositionSession` 把 Qt 的
    /// `replacementStart`/`replacementLength`（UTF-16 QChar 偏移）解析后构造。
    /// 此函数不再做任何 UTF-16→UTF-8 或 base_text↔virtual_text↔committed_text
    /// 坐标换算。
    ///
    /// `EditOp::ImeCommit` 调用一次 Core `ImeCommit` 原子命令（三段语义），
    /// Core 内部顺序执行两步正文修改，只产生一个 revision 推进和一个 UndoEntry，
    /// 只在 `record_edit_transaction` 末尾做一次 pipeline 只读投影读取
    /// + 一次 snapshot + 一次视觉事务。
    pub(crate) fn ime_replace_and_insert(&mut self, event: ImeReplaceEvent) {
        if !self.current_editor_enabled {
            return;
        }
        // Issue #701 评论 5702675971: 不再因 inserted_text.is_empty() 直接 return。
        // 改为：既无删除又无插入时 return。允许"空 commit + replacement"（纯删除）进入事务。
        let inserted = normalize_plain_text(&event.inserted_text);
        if !event.has_any_deletion() && inserted.is_empty() {
            return;
        }

        let (preedit_byte_start, preedit_byte_end) = self.preedit_byte_range_in_virtual_text();
        let commit = self.pipeline.prepare_composition_commit(
            &inserted,
            self.pipeline.cursor(),
            preedit_byte_start,
            preedit_byte_end,
        );

        let selection_byte_range = event.selection_byte_range;
        let (rep_start, rep_end) = event.replacement_byte_range_after_selection;

        // candidate = 插入文本在新 committed text 中的位置。
        // inserted 在 new text 中的起点 = rep_start（第二步 replacement 的起点）。
        let candidate_byte_start = rep_start;
        let candidate_byte_end = rep_start + inserted.len();

        // committed_replace 传给动画协调器：
        // - 有 selection 时用 selection range（第一步删除的范围）；
        // - 无 selection 时用 replacement range（第二步的范围）。
        let _committed_replace_start = if let Some((sel_start, _)) = selection_byte_range {
            sel_start
        } else {
            rep_start
        };
        let _committed_replace_end = if let Some((_, sel_end)) = selection_byte_range {
            sel_end
        } else {
            rep_end
        };

        let visual_cause = if inserted.chars().count() == 1 {
            EditorTransactionCause::Typing
        } else {
            EditorTransactionCause::TypingCommit
        };

        // Issue #756: composition commit follows the shared effective animation policy.
        let composition = if commit.was_composing
            && super::animation::any_animation_enabled(
                self.current_typing_animation_enabled,
                self.current_smooth_cursor_enabled,
                self.current_coordinated_animation_enabled,
            ) {
            Some(CompositionCommitParams {
                preedit_byte_start: commit.preedit_byte_start,
                preedit_byte_end: commit.preedit_byte_end,
                saved_virtual_text: commit.saved_virtual_text.clone(),
                candidate_byte_start,
                candidate_byte_end,
                cancel_reason: "commit_replace",
                summary_tag: "composition_commit_replace",
            })
        } else {
            None
        };

        // Issue #701 评论 5704110106: 用 Core ImeCommit 原子命令（三段语义），
        // Core 内部顺序执行两步正文修改，只产生一个 revision 推进和一个 UndoEntry。
        let op = EditOp::ImeCommit {
            selection_byte_range,
            replacement_byte_range: (rep_start, rep_end),
            inserted_text: inserted,
            pipeline_cause: visual_cause,
        };

        // Issue #819 评论 5956495850 第 1 节: IME replace+commit 与普通输入/删除
        // 共用 apply_edit_with_visuals 统一入口。
        let _outcome = self.apply_edit_with_visuals(op, visual_cause, composition);

        self.pipeline.finish_composition_commit();

        // Issue #701 评论 5699573227 第三阶段 (F2/F7): 不再把
        // pending_preedit_cursor_rect 反写到 cursor_ctrl.visual_x/visual_y。
        // pending_preedit_cursor_rect 只作为 IME commit 动画的 old caret 起点
        // （已传给 record_composition_commit_transaction 的 old_cursor_rect）。
        // 提交后的 target caret 来自 new selection/head 在 new layout 中的 caret，
        // 由 emit_content_changed → update_cursor_visual_position 统一计算。

        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::TextTransaction;
        self.emit_content_changed();
    }

    pub(crate) fn delete_backward(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        let cursor = self.pipeline.cursor();
        let (start, end) = if self.pipeline.has_selection() {
            self.pipeline.selection_range()
        } else {
            // 无选区时删除前一个字符；行首时 previous_grapheme_boundary 返回 cursor，直接返回。
            let prev = self.pipeline.previous_grapheme_boundary(cursor);
            if prev == cursor {
                return;
            }
            (prev, cursor)
        };

        // Issue #819 评论 5956495850 第 1 节: 普通删除与普通输入/IME commit
        // 共用 apply_edit_with_visuals 统一入口。跨行删除的 old/new caret 都从
        // 同一对 snapshot 读取（record_transaction 内部排版 old/new 后取 caret），
        // 解决删除到上一行时回抽/跳一下。
        let op = EditOp::Delete {
            start,
            end,
            pipeline_cause: EditorTransactionCause::Delete,
        };
        let outcome = self.apply_edit_with_visuals(op, EditorTransactionCause::Delete, None);
        if outcome.applied {
            self.cursor_ctrl.last_move_source =
                cursor_controller::CursorMoveSource::TextTransaction;
            self.emit_content_changed();
        }
    }

    pub(crate) fn delete_forward(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        let cursor = self.pipeline.cursor();
        let (start, end) = if self.pipeline.has_selection() {
            self.pipeline.selection_range()
        } else {
            // 无选区时删除后一个字符；行末时 next_grapheme_boundary 返回 cursor，直接返回。
            let next = self.pipeline.next_grapheme_boundary(cursor);
            if next == cursor {
                return;
            }
            (cursor, next)
        };

        let op = EditOp::Delete {
            start,
            end,
            pipeline_cause: EditorTransactionCause::Delete,
        };
        let outcome = self.apply_edit_with_visuals(op, EditorTransactionCause::Delete, None);
        if outcome.applied {
            self.cursor_ctrl.last_move_source =
                cursor_controller::CursorMoveSource::TextTransaction;
            self.emit_content_changed();
        }
    }

    pub(crate) fn delete_selection(&mut self) {
        if !self.current_editor_enabled || !self.pipeline.has_selection() {
            return;
        }
        let (start, end) = self.pipeline.selection_range();

        let op = EditOp::Delete {
            start,
            end,
            pipeline_cause: EditorTransactionCause::Delete,
        };
        let outcome = self.apply_edit_with_visuals(op, EditorTransactionCause::Delete, None);
        if outcome.applied {
            self.cursor_ctrl.last_move_source =
                cursor_controller::CursorMoveSource::TextTransaction;
            self.emit_content_changed();
        }
    }

    pub(crate) fn select_all(&mut self) {
        // Issue #705 评论 5717380886: 全选是非正文事务导致的逻辑 cursor 移动。
        let text_len = self.pipeline.committed_text().len();
        let _ = self.pipeline.set_selection(0, text_len);
        self.bump_visual_revision();
        self.adjust_affinity_at_wrap_boundary();
        self.cursor_position_changed();
        self.selection_changed();
        self.request_static_repaint();
    }

    pub(crate) fn selected_text(&self) -> QString {
        self.pipeline.selected_text().into()
    }

    pub(crate) fn undo(&mut self) {
        let old = self.pipeline.snapshot();
        // Issue #810 评论 5934658350: Undo 会修改 committed text 并可能生成文字动画，
        // 必须在 Core 修改前固定 old canonical（与 Insert/Delete/Replace 同一不变量）。
        self.ensure_current_canonical_before_edit();
        if let Some(result) = self.pipeline.perform_undo() {
            // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
            let new = self.pipeline.snapshot();
            let _ = self.record_transaction(old, new, &result, true);
            self.cursor_ctrl.last_move_source =
                cursor_controller::CursorMoveSource::TextTransaction;
            self.emit_content_changed();
            // Issue #843: Undo 也上报编辑事实，使统计能正确回退。
            // result.cause 是 Undo，content_delta 是真实逆向字符变化。
            self.emit_editor_change_fact(result.cause, &result);
        }
    }

    pub(crate) fn redo(&mut self) {
        let old = self.pipeline.snapshot();
        // Issue #810 评论 5934658350: Redo 同 Undo，在 Core 修改前固定 old canonical。
        self.ensure_current_canonical_before_edit();
        if let Some(result) = self.pipeline.perform_redo() {
            // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
            let new = self.pipeline.snapshot();
            let _ = self.record_transaction(old, new, &result, true);
            self.cursor_ctrl.last_move_source =
                cursor_controller::CursorMoveSource::TextTransaction;
            self.emit_content_changed();
            // Issue #843: Redo 也上报编辑事实，使统计能正确重放。
            // result.cause 是 Redo，content_delta 是真实正向字符变化。
            self.emit_editor_change_fact(result.cause, &result);
        }
    }

    pub(crate) fn handle_key(&mut self, key: i32, modifiers: i32) -> bool {
        input::handle_key(self, key, modifiers)
    }

    /// Issue #826: 指针点击的唯一实现。
    ///
    /// 固定顺序：
    /// 1. `hit_test` 得到目标 byte index 与 affinity；
    /// 2. `set_selection` 立即改 Core selection；
    /// 3. `update_cursor_visual_position()` 更新唯一 CursorController。
    /// 正文过渡独立运行，不因点击而提前结束。
    ///
    /// 旧路线的 caret handover / detach / epoch ownership 全部删除：光标与文字
    /// 动画完全解耦，点击不需要"抢"正文动画的 caret 所有权。
    pub(crate) fn click_at(&mut self, x: f32, y: f32, extend: bool) {
        let pointer_sequence = self.pointer_diagnostics.ensure_active_sequence();
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        let old_cursor = self.pipeline.cursor();
        let old_anchor = self.pipeline.selection_anchor();
        let anchor = if extend {
            self.pipeline.selection_anchor()
        } else {
            index
        };

        self.cursor_ctrl.affinity = affinity;
        // PointerClick 的 Tween/Snap 由统一 Coordinator 按 smooth cursor 设置决定。
        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::PointerClick;
        editor_debug_log(&format!(
            "click_at: mouse_x={:.1}, mouse_y={:.1}, current_scroll_y={:.1}, hit_index={}, affinity={:?}, extend={}",
            x, y, self.current_scroll_y, index, affinity, extend
        ));
        let _ = self.pipeline.set_selection(anchor, index);
        self.bump_visual_revision();
        self.pipeline.composition_mut().clear();
        self.cursor_position_changed();
        self.selection_changed();
        self.cursor_ctrl.dirty = true;
        self.update_cursor_visual_position();

        // A click commits the logical caret immediately. The pointer sequence joins
        // this logical result to its visual target and the caret submitted for drawing.
        record_pointer_click(
            x,
            y,
            index,
            old_cursor,
            self.pipeline.cursor(),
            old_anchor,
            anchor,
            pointer_sequence,
            affinity,
            self.current_scroll_y,
        );

        let mut target_fields = std::collections::BTreeMap::new();
        target_fields.insert(
            "logical_byte_index".to_string(),
            serde_json::json!(self.pipeline.cursor()),
        );
        target_fields.insert(
            "affinity".to_string(),
            serde_json::json!(format!("{:?}", affinity)),
        );
        target_fields.insert(
            "target_x".to_string(),
            serde_json::json!(self.cursor_ctrl.target_x),
        );
        target_fields.insert(
            "target_y".to_string(),
            serde_json::json!(self.cursor_ctrl.target_y),
        );
        target_fields.insert(
            "visual_x".to_string(),
            serde_json::json!(self.cursor_ctrl.visual_x),
        );
        target_fields.insert(
            "visual_y".to_string(),
            serde_json::json!(self.cursor_ctrl.visual_y),
        );
        target_fields.insert(
            "scroll_y".to_string(),
            serde_json::json!(self.current_scroll_y),
        );
        target_fields.insert(
            "visible".to_string(),
            serde_json::json!(self.cursor_ctrl.visible),
        );
        target_fields.insert(
            "animation_active".to_string(),
            serde_json::json!(self.cursor_ctrl.animation.is_some()),
        );
        target_fields.insert(
            "smooth_cursor_enabled".to_string(),
            serde_json::json!(self.current_smooth_cursor_enabled),
        );
        target_fields.insert(
            "coordinated_animation_enabled".to_string(),
            serde_json::json!(self.current_coordinated_animation_enabled),
        );
        super::pointer_diagnostics::record_event(
            "editor.pointer.visual_target",
            Some(pointer_sequence),
            target_fields,
        );

        if let Some(previous) = self.pointer_diagnostics.pending_render() {
            let mut fields = std::collections::BTreeMap::new();
            fields.insert(
                "phase".to_string(),
                serde_json::json!(if previous.first_frame_logged {
                    "retargeted_by_new_click"
                } else {
                    "superseded_before_frame"
                }),
            );
            fields.insert(
                "drawn_caret_x".to_string(),
                serde_json::json!(self.cursor_ctrl.visual_x),
            );
            fields.insert(
                "drawn_caret_y".to_string(),
                serde_json::json!(self.cursor_ctrl.visual_y),
            );
            fields.insert(
                "new_pointer_sequence".to_string(),
                serde_json::json!(pointer_sequence),
            );
            super::pointer_diagnostics::record_event(
                "editor.pointer.rendered",
                Some(previous.sequence),
                fields,
            );
        }
        self.pointer_diagnostics
            .set_pending_render(pointer_sequence);

        self.request_static_repaint();
    }

    pub(crate) fn drag_select_at(&mut self, x: f32, y: f32) {
        // Issue #705 评论 5718299909: 先 hit_test，确认 head/affinity 真的改变
        // 再 bump epoch。拖到当前 cursor 同一位置不应 bump。
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        self.cursor_ctrl.affinity = affinity;
        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::DragSelection;
        // Issue #712: 拖选设置 CursorMoveSource::DragSelection，跨行走 Snap。
        // Issue #705: 鼠标点击路径里不要自己单独决定光标动画模式。
        // 是否 Tween 由统一的光标移动规则决定。drag_select 走统一 snap 辅助方法。
        self.snap_cursor_for_pointer_action();
        let old_cursor = self.pipeline.cursor();
        let old_anchor = self.pipeline.selection_anchor();
        let anchor = self.pipeline.selection_anchor();
        let _ = self.pipeline.set_selection(anchor, index);
        if old_cursor != self.pipeline.cursor() || old_anchor != self.pipeline.selection_anchor() {
            let mut fields = std::collections::BTreeMap::new();
            fields.insert("x".to_string(), serde_json::json!(x));
            fields.insert("y".to_string(), serde_json::json!(y));
            fields.insert("old_cursor".to_string(), serde_json::json!(old_cursor));
            fields.insert(
                "new_cursor".to_string(),
                serde_json::json!(self.pipeline.cursor()),
            );
            fields.insert("old_anchor".to_string(), serde_json::json!(old_anchor));
            fields.insert(
                "new_anchor".to_string(),
                serde_json::json!(self.pipeline.selection_anchor()),
            );
            super::pointer_diagnostics::record_event(
                "editor.pointer.selection",
                self.pointer_diagnostics.active_sequence(),
                fields,
            );
        }
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn long_press_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        self.cursor_ctrl.affinity = affinity;
        // Issue #712: 长按设置 CursorMoveSource::DragSelection，跨行走 Snap。
        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::DragSelection;
        // Issue #705: 统一 snap 辅助方法,不在点击代码里自己强制 Snap。
        self.snap_cursor_for_pointer_action();
        if !self.pipeline.has_selection() {
            self.select_word_at_impl(index);
        }
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
        // Issue #819 评论 5956495850 第 6 节：左键长按只负责选择，不弹菜单。
        // 旧的 self.context_menu_requested(x, y) 已删除——右键菜单只由右键
        // TapHandler (acceptedButtons: Qt.RightButton) 触发，不再由左键长按触发。
    }

    pub(crate) fn select_word_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        self.cursor_ctrl.affinity = affinity;
        // Issue #712: 选词设置 CursorMoveSource::DragSelection，跨行走 Snap。
        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::DragSelection;
        // Issue #705: 统一 snap 辅助方法,不在点击代码里自己强制 Snap。
        self.snap_cursor_for_pointer_action();
        self.select_word_at_impl(index);
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    /// 拖选、长按和选词期间将光标固定在逻辑 selection head。
    ///
    /// 普通 click_at 不调用此方法；它由 Coordinator 按 smooth cursor 设置决定是否 Tween。
    fn snap_cursor_for_pointer_action(&mut self) {
        self.cursor_ctrl.force_snap_next = true;
    }

    pub(crate) fn select_word_at_impl(&mut self, index: usize) {
        let committed_text = self.pipeline.committed_text().to_string();
        let Some((byte_start, byte_end)) = compute_word_bounds(&committed_text, index) else {
            return;
        };
        let _ = self.pipeline.set_selection(byte_start, byte_end);
    }

    pub(crate) fn clipboard_copy(&mut self) -> bool {
        if !self.pipeline.has_selection() {
            return false;
        }
        let text = self.pipeline.selected_text();
        if text.is_empty() {
            return false;
        }
        use writer_core::platform_interaction::clipboard_focus::{
            ClipboardAndFocusAdapter, ClipboardRequest, ClipboardResult,
        };
        let result = ClipboardAndFocusAdapter::execute_clipboard(
            self.pipeline.clipboard_adapter_mut(),
            ClipboardRequest::Copy { text },
        );
        matches!(result, ClipboardResult::Copied)
    }

    pub(crate) fn clipboard_paste(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        use writer_core::platform_interaction::clipboard_focus::{
            ClipboardAndFocusAdapter, ClipboardRequest, ClipboardResult,
        };
        let result = ClipboardAndFocusAdapter::execute_clipboard(
            self.pipeline.clipboard_adapter_mut(),
            ClipboardRequest::Paste,
        );
        if let ClipboardResult::Pasted { text } = result {
            let normalized = normalize_plain_text(&text);
            self.insert_text_with_cause(normalized.into(), Some(EditorTransactionCause::Paste));
        }
    }

    pub(crate) fn insert_preedit(&mut self, text: QString) {
        self.clear_active_text_animations();
        input::insert_preedit_text(self, text.to_string());
    }

    pub(crate) fn commit_preedit(&mut self, text: QString) {
        input::commit_preedit_text(self, text.to_string());
    }

    pub(crate) fn cancel_preedit(&mut self) {
        self.clear_active_text_animations();
        input::cancel_preedit(self);
    }

    pub(crate) fn move_cursor_horizontal(&mut self, forward: bool, extend: bool) {
        // Issue #705 评论 5718299909: 先算 next，确认逻辑 caret/selection 真的会变
        // 再 bump epoch。no-op（已在行首/文末且 !extend）不 bump，避免切断活动
        // 正文事务 caret 所有权。epoch 的定义是"用户手动改变了当前 caret 所有权/
        // 逻辑位置"，不是"用户按过一次键"。
        let current_cursor = self.pipeline.cursor();
        let committed_text = self.pipeline.committed_text();
        let next = if forward {
            next_char_boundary(committed_text, current_cursor).unwrap_or(current_cursor)
        } else {
            prev_char_boundary(committed_text, current_cursor).unwrap_or(current_cursor)
        };
        if next == current_cursor && !extend {
            return;
        }
        // Issue #705 评论 5717380886: 方向键是非正文事务导致的逻辑 cursor 移动。
        // Issue #705 评论 5718299909: 仅在确认 next != cursor 后 bump。
        // extend 且 next == cursor 时 head/anchor 不变（no-op），不 bump。
        // Issue #712: 方向键水平移动设置 CursorMoveSource::KeyboardNavigation，
        // 允许 smooth cursor 开启时跨行 Tween。
        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::KeyboardNavigation;
        self.cursor_ctrl.affinity = if forward {
            CaretAffinity::Downstream
        } else {
            CaretAffinity::Upstream
        };
        if extend {
            let anchor = self.pipeline.selection_anchor();
            let _ = self.pipeline.set_selection(anchor, next);
        } else {
            let _ = self.pipeline.set_selection(next, next);
        }
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        // Issue #679 评论 5657313927 (7a): CursorOnly 的创建统一放到
        // update_cursor_visual_position() 里，这里不再手动调 handle_cursor_only。
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn move_cursor_vertical(&mut self, down: bool, extend: bool) {
        // Issue #705 评论 5718299909: 先算 line_idx/target_idx，确认目标行不同
        // 再 bump epoch。no-op（已在第一/最后一行或 cursor_line_and_x() 返回
        // None）不 bump，避免切断活动正文事务 caret 所有权。
        // Issue #705 评论 5716410988: lines 也来自 current_render_layout_snapshot,
        // 与 cursor_line_and_x / index_at_line_x 同源。
        let snapshot = self.current_render_layout_snapshot();
        let lines = snapshot.lines.clone();
        let Some((line_idx, x)) = self.cursor_line_and_x() else {
            return;
        };
        let target_idx = if down {
            (line_idx + 1).min(lines.len().saturating_sub(1))
        } else {
            line_idx.saturating_sub(1)
        };
        if target_idx == line_idx {
            return;
        }
        // Issue #705 评论 5717380886: 方向键是非正文事务导致的逻辑 cursor 移动。
        // Issue #705 评论 5718299909: 仅在确认 target_idx != line_idx 后 bump。
        // Issue #712: 方向键垂直移动设置 CursorMoveSource::KeyboardNavigation，
        // 允许 smooth cursor 开启时跨行 Tween。
        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::KeyboardNavigation;
        let index = self.index_at_line_x(&lines[target_idx], x);
        self.cursor_ctrl.affinity = self
            .editor_layout
            .affinity_for_index_on_line(&lines[target_idx], index);
        if extend {
            let anchor = self.pipeline.selection_anchor();
            let _ = self.pipeline.set_selection(anchor, index);
        } else {
            let _ = self.pipeline.set_selection(index, index);
        }
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        // Issue #679 评论 5657313927 (7a): CursorOnly 的创建统一放到
        // update_cursor_visual_position() 里，这里不再手动调 handle_cursor_only。
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn move_to_line_edge(&mut self, end: bool, extend: bool) {
        // Issue #705 评论 5718299909: 先算目标 index + affinity，确认与当前 caret
        // 不同再 bump epoch。no-op（cursor_line_and_x() 返回 None，或目标与当前
        // caret 完全相同）不 bump，避免切断活动正文事务 caret 所有权。
        // Issue #705 评论 5716410988: lines 也来自 current_render_layout_snapshot,
        // 与 cursor_line_and_x 同源。
        let snapshot = self.current_render_layout_snapshot();
        let lines = snapshot.lines.clone();
        let Some((line_idx, _)) = self.cursor_line_and_x() else {
            return;
        };
        let line = &lines[line_idx];
        let (index, affinity) = if end {
            (line.byte_end, CaretAffinity::Upstream)
        } else {
            (line.byte_start, CaretAffinity::Downstream)
        };
        // Issue #705 评论 5717380886: Home/End 是非正文事务导致的逻辑 cursor 移动。
        // Issue #705 评论 5718299909: 仅在目标 index 或 affinity 与当前不同时 bump。
        // Issue #712: Home/End 设置 CursorMoveSource::KeyboardNavigation，
        // 允许 smooth cursor 开启时跨行 Tween。
        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::KeyboardNavigation;
        self.cursor_ctrl.affinity = affinity;
        if extend {
            let anchor = self.pipeline.selection_anchor();
            let _ = self.pipeline.set_selection(anchor, index);
        } else {
            let _ = self.pipeline.set_selection(index, index);
        }
        self.cursor_position_changed();
        self.selection_changed();
        // Issue #679 评论 5657313927 (7a): CursorOnly 的创建统一放到
        // update_cursor_visual_position() 里，这里不再手动调 handle_cursor_only。
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }
}

/// Issue #705 评论 5718299909: 纯计算 word bounds，供 `select_word_at_impl` 和
/// `long_press_at` / `select_word_at` 的 no-op 预判使用。
///
/// 返回 `(byte_start, byte_end)`（UTF-8 byte offset，半开区间）。`text` 为空或
/// `index > text.len()` 时返回 `None`。这是纯函数，不触碰任何 editor 状态，
/// 因此可在 bump epoch 之前安全调用以预判选词结果是否会改变当前 selection。
fn compute_word_bounds(text: &str, index: usize) -> Option<(usize, usize)> {
    if text.is_empty() || index > text.len() {
        return None;
    }
    let char_index = byte_to_char_index(text, index);
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let ci = char_index.min(chars.len().saturating_sub(1));

    fn is_word_boundary(c: char) -> bool {
        c.is_whitespace()
            || c == '\n'
            || c == ','
            || c == '?'
            || c == '!'
            || c == '！'
            || c == ';'
            || c == ':'
            || c == '"'
            || c == '"'
            || c == '\u{2018}'
            || c == '\u{2019}'
            || c == '？'
            || c == '-'
            || c == '.'
            || c == '('
            || c == ')'
            || c == '（'
            || c == '）'
    }

    let mut start = ci;
    while start > 0 && !is_word_boundary(chars[start - 1]) {
        start -= 1;
    }
    let mut end = ci + 1;
    while end < chars.len() && !is_word_boundary(chars[end]) {
        end += 1;
    }

    let byte_start = chars[..start].iter().map(|c| c.len_utf8()).sum::<usize>();
    let byte_end = chars[..end].iter().map(|c| c.len_utf8()).sum::<usize>();
    Some((byte_start, byte_end))
}

/// Issue #824 评论 5971089641 第 9 节 / 评论 5972388049 第 4 节：
/// 鼠标点击的正式诊断事件。
///
/// Issue #826: 指针点击的正式诊断事件 `editor.pointer.click`。
///
/// 点击导致逻辑 cursor 变化时记录：pointer press 坐标、hit_test byte index、
/// old/new cursor、old/new anchor。写 `writer_diagnostics` 正式事件
/// （诊断包可见），不是 env-gated debug log。
///
/// 字段：press 坐标 x/y、hit_test 得到的 byte index、old/new cursor、
/// old/new anchor。用于排查"点击落点与逻辑 caret 不一致"。
fn record_pointer_click(
    pointer_x: f32,
    pointer_y: f32,
    hit_test_byte_index: usize,
    old_cursor: usize,
    new_cursor: usize,
    old_anchor: usize,
    new_anchor: usize,
    pointer_sequence: u64,
    affinity: CaretAffinity,
    scroll_y: f32,
) {
    let mut fields: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    fields.insert("x".to_string(), serde_json::json!(pointer_x));
    fields.insert("y".to_string(), serde_json::json!(pointer_y));
    fields.insert(
        "hit_index".to_string(),
        serde_json::json!(hit_test_byte_index),
    );
    fields.insert("old_cursor".to_string(), serde_json::json!(old_cursor));
    fields.insert("new_cursor".to_string(), serde_json::json!(new_cursor));
    fields.insert("old_anchor".to_string(), serde_json::json!(old_anchor));
    fields.insert("new_anchor".to_string(), serde_json::json!(new_anchor));
    fields.insert(
        "affinity".to_string(),
        serde_json::json!(format!("{:?}", affinity)),
    );
    fields.insert("scroll_y".to_string(), serde_json::json!(scroll_y));
    super::pointer_diagnostics::record_event(
        "editor.pointer.click",
        Some(pointer_sequence),
        fields,
    );
}

/// Issue #815 评论 6042062633 修改 8: IME composition commit 的正式跳过事件。
///
/// 日志只暴露 `cause`；三个动画开关字段用来确认"是不是被哪个开关关掉了"。
fn record_composition_commit_skip(
    item: &SujianEditorItem,
    cause: &str,
    candidate_range: Option<(usize, usize)>,
) {
    editor_animation_transaction_skipped_event(&AnimationSkipFields {
        cause,
        operation_kind: "CompositionCommitOrCancel",
        typing_animation_enabled: item.current_typing_animation_enabled,
        smooth_cursor_enabled: item.current_smooth_cursor_enabled,
        coordinated_animation_enabled: item.current_coordinated_animation_enabled,
        old_caret_present: false,
        new_caret_present: false,
        inserted_range: candidate_range,
        unit_kinds: "",
        cursor_track_present: false,
        is_scrolling: item.current_is_scrolling,
        is_loading: item.current_is_loading,
        is_applying_format: item.current_is_applying_format,
        transaction_id: None,
        generation: 0,
    });
}
