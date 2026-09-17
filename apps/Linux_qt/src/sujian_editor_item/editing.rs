use super::*;
use crate::editor::input::events::ImeReplaceEvent;

/// Issue #701 评论 5699573227 第三阶段: 统一编辑操作描述。
///
/// `record_edit_transaction` 内部根据此枚举执行一次 pipeline edit command。
/// 所有普通输入、删除、IME commit/replace 都收口到这同一个入口，
/// 不再各自直接调 `pipeline.insert_text` / `pipeline.replace_range` /
/// `pipeline.delete_range`。
///
/// `pipeline_cause` 是传给 Core pipeline 的事务分类（用于 undo/redo 栈语义），
/// 与 `record_edit_transaction` 的 `visual_cause`（用于视觉事务分类）分离。
/// 多数场景两者相同，但 `clipboard_paste` 走 `insert_text_with_cause` 时
/// `pipeline_cause` 仍是 `Typing`/`TypingCommit`，`visual_cause` 是 `Paste`。
enum EditOp {
    Insert {
        cursor: usize,
        text: String,
        pipeline_cause: EditorTransactionCause,
    },
    Replace {
        start: usize,
        end: usize,
        text: String,
        pipeline_cause: EditorTransactionCause,
    },
    Delete {
        start: usize,
        end: usize,
        pipeline_cause: EditorTransactionCause,
    },
    /// Issue #701 评论 5702675971: IME commit 的 Qt 两步语义。
    ///
    /// 调用一次 Core `ImeCommit` 原子命令（三段语义），在 Core 内部顺序执行两步
    /// 正文修改，只产生一个 Core revision 推进和一个 UndoEntry：
    /// 1. 第一步：删 selection（在 committed text 上），
    ///    `selection_byte_range` 为 `None` 或零长度时跳过（传 (0, 0)）；
    /// 2. 第二步：在删 selection 后的文本（base_text）上做 replacement/insert，
    ///    `replacement_byte_range` 是 base_text 坐标。
    ImeCommit {
        selection_byte_range: Option<(usize, usize)>,
        replacement_byte_range: (usize, usize),
        inserted_text: String,
        pipeline_cause: EditorTransactionCause,
    },
}

/// Issue #701 评论 5699573227 第三阶段: IME composition commit 参数。
///
/// 仅在 `record_edit_transaction` 处理 composition commit/replace 时提供。
/// 普通输入/删除传 `None`，走 `record_transaction` 路径。
/// 带 `Some` 时走 `record_composition_commit_transaction` 路径，处理 preedit
/// 区间收进、candidate 揭示、committed replace range、pending preedit cursor rect
/// 作为 old caret 起点等 composition 专属语义。
///
/// 两条路径最终都创建同一种 `TextVisualTransaction`（放入 `prepared_queue`），
/// 文字显隐/位移和光标位移消费同一个 timeline、同一个 frame progress。
struct CompositionCommitParams {
    pending_preedit_cursor_rect: Option<CursorRect>,
    preedit_byte_start: usize,
    preedit_byte_end: usize,
    saved_virtual_text: String,
    candidate_byte_start: usize,
    candidate_byte_end: usize,
    committed_replace_start: usize,
    committed_replace_end: usize,
    cancel_reason: &'static str,
    summary_tag: &'static str,
}

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
        use animation_coordinator::CursorBlinkMode;
        let blink_mode = if self.current_coordinated_text_cursor_animation_enabled
            && self
                .pipeline
                .animation_coordinator_mut()
                .has_active_insert()
        {
            CursorBlinkMode::Suppressed
        } else {
            CursorBlinkMode::Normal
        };
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
        self.sync_buffer_from_pipeline();
    }

    /// Issue #701 评论 5699573227 第三阶段: 统一 composition commit 事务创建入口。
    ///
    /// 把 `insert_text_with_cause` 和 `ime_replace_and_insert` 的 composition commit
    /// 分支收口到这一个 helper，固定做：生成 old/new layout snapshot → 创建
    /// transaction → cancel_active_composition → handle_composition_commit_or_cancel
    /// → prepare_transaction_textures → set_previous/current_layout_snapshot。
    ///
    /// `pending_preedit_cursor_rect` 只作为 IME commit 动画的 old caret 起点
    /// （传给 `handle_composition_commit_or_cancel` 的 `old_cursor_rect`），
    /// **不再**覆盖提交后的 target caret 或 `cursor_ctrl.visual_x/visual_y`。
    /// 提交后的 target caret 来自 new selection/head 在 new layout 中的 caret，
    /// 由 `emit_content_changed` → `update_cursor_visual_position` 统一计算。
    #[allow(clippy::too_many_arguments)]
    fn record_composition_commit_transaction(
        &mut self,
        old: &EditorSnapshot,
        new: &EditorSnapshot,
        cause: EditorTransactionCause,
        pending_preedit_cursor_rect: Option<CursorRect>,
        preedit_byte_start: usize,
        preedit_byte_end: usize,
        saved_virtual_text: &str,
        candidate_byte_start: usize,
        candidate_byte_end: usize,
        committed_replace_start: usize,
        committed_replace_end: usize,
        cancel_reason: &str,
        summary_tag: &str,
    ) {
        let width = self.bounding_width();
        let composition_range = Some((preedit_byte_start, preedit_byte_end));
        let old_snapshot = self
            .pipeline
            .animation_coordinator()
            .active_composition_new_snapshot()
            .cloned()
            .unwrap_or_else(|| {
                self.pipeline
                    .current_layout_snapshot()
                    .clone()
                    .unwrap_or_else(|| {
                        self.build_editor_layout_snapshot(width, false, composition_range)
                    })
            });

        let transaction = self.pipeline.engine().create_transaction(
            &old.text,
            &new.text,
            EditorSelection {
                anchor: EditorCursor::new(&old.text, old.selection_anchor),
                head: EditorCursor::new(&old.text, old.cursor),
            },
            EditorSelection {
                anchor: EditorCursor::new(&new.text, new.selection_anchor),
                head: EditorCursor::new(&new.text, new.cursor),
            },
            cause,
        );
        self.pipeline
            .animation_coordinator_mut()
            .cancel_active_composition(cancel_reason);

        // Issue #658 评论 5623746506 问题 2b: composition commit 的 new text
        // 走 promote=true，generation 直接成为 current，不再用完即删。
        let new_snapshot = self.build_editor_layout_snapshot(width, true, composition_range);
        let new_cursor_rect = new_snapshot.caret_rect.as_ref().map(|c| CursorRect {
            x: c.x,
            top: c.y,
            bottom: c.y + c.h,
            baseline_y: c.y + c.h * 0.8,
        });

        let visual_text_unchanged =
            !saved_virtual_text.is_empty() && saved_virtual_text == new.text;

        let key = self
            .pipeline
            .animation_coordinator_mut()
            .handle_composition_commit_or_cancel(
                &old_snapshot,
                &new_snapshot,
                preedit_byte_start,
                preedit_byte_end,
                true,
                visual_text_unchanged,
                candidate_byte_start,
                candidate_byte_end,
                committed_replace_start,
                committed_replace_end,
                pending_preedit_cursor_rect,
                new_cursor_rect,
            );

        if let Some(key) = key {
            self.prepare_transaction_textures(key);
        }
        self.pipeline
            .set_previous_layout_snapshot(Some(old_snapshot));
        self.pipeline
            .set_current_layout_snapshot(Some(new_snapshot));

        self.last_event_count = 1;
        self.last_summary = format!(
            "cause={:?};changes={};vt={};animate=true",
            transaction.cause,
            transaction.changes.len(),
            summary_tag,
        )
        .into();
        editor_animation_debug_log(&format!(
            "record_composition_commit_transaction: cancel_reason={}, cause={:?}, changes={}",
            cancel_reason,
            transaction.cause,
            transaction.changes.len(),
        ));

        self.transaction_created();
    }

    /// Issue #701 评论 5699573227 第三阶段: 统一编辑事务入口。
    ///
    /// `insert_text_with_cause` / `delete_backward` / `delete_forward` /
    /// `ime_replace_and_insert` / `delete_selection` 全部收口到这一个 helper。
    /// 固定做：
    /// 1. 保存 old text/selection/caret（`self.buffer.snapshot()`）；
    /// 2. 调一次 pipeline edit command（由 `op` 描述，不再由调用者各自直调）；
    /// 3. `sync_buffer_from_pipeline` 后读取 new text/selection/caret；
    /// 4. 生成一对 old/new layout snapshot 并创建一次视觉事务。
    ///
    /// - `op$` 不带 composition commit 参数（`composition == None`）时走
    ///   `record_transaction`，由 `pipeline.record_visual_transaction` 内部
    ///   排版 old/new 并 `process_transaction`。
    /// - 带 `CompositionCommitParams` 时走 `record_composition_commit_transaction`，
    ///   处理 preedit 区间收进、candidate 揭示、committed replace range、
    ///   `pending_preedit_cursor_rect` 作为 old caret 起点等 composition 专属语义。
    ///
    /// 两条路径最终都创建同一种 `TextVisualTransaction`（放入 `prepared_queue`），
    /// Typing / Delete / TypingCommit / IME commit/replace 都进入同一种
    /// `VisualTransaction`，文字显隐/位移和光标位移消费同一个 timeline、
    /// 同一个 frame progress。
    ///
    /// 返回 `true` 表示编辑已应用并记录事务；`false` 表示 pipeline edit 未应用
    /// （如空删除范围），调用者据此决定是否 `emit_content_changed`。
    /// `insert_text_with_cause` 总是 `emit_content_changed`（保持原行为），
    /// `delete_backward` / `delete_forward` / `delete_selection` 仅在 `true` 时
    /// `emit_content_changed`。
    fn record_edit_transaction(
        &mut self,
        op: EditOp,
        visual_cause: EditorTransactionCause,
        composition: Option<CompositionCommitParams>,
    ) -> bool {
        let old = self.buffer.snapshot();

        let applied = match op {
            EditOp::Insert {
                cursor,
                text,
                pipeline_cause,
            } => self
                .pipeline
                .insert_text(cursor, &text, pipeline_cause)
                .is_some(),
            EditOp::Replace {
                start,
                end,
                text,
                pipeline_cause,
            } => self
                .pipeline
                .replace_range(start, end, &text, pipeline_cause)
                .is_some(),
            EditOp::Delete {
                start,
                end,
                pipeline_cause,
            } => self
                .pipeline
                .delete_range(start, end, pipeline_cause)
                .is_some(),
            EditOp::ImeCommit {
                selection_byte_range,
                replacement_byte_range,
                inserted_text,
                pipeline_cause,
            } => {
                // Issue #701 评论 5704110106: 调用一次 Core ImeCommit 原子命令
                // （三段语义），Core 内部顺序执行两步正文修改，只产生一个
                // revision 推进和一个 UndoEntry。
                // selection_byte_range 为 None 或零长度时传 (0, 0)（零长度 range，
                // Core 不会删除）。
                let (sel_start, sel_end) = selection_byte_range.unwrap_or((0, 0));
                let (rep_start, rep_end) = replacement_byte_range;
                self.pipeline
                    .ime_commit(
                        sel_start,
                        sel_end,
                        rep_start,
                        rep_end,
                        &inserted_text,
                        pipeline_cause,
                    )
                    .is_some()
            }
        };
        if !applied {
            return false;
        }
        self.sync_buffer_from_pipeline();
        // Issue #658 评论 5623746506 问题 1: 不在 record_transaction 之前调
        // adjust_affinity_at_wrap_boundary（会触发 ensure_layout_cached 排版 A，
        // 与 record_visual_transaction 排版 B 重复）。affinity 调整移到
        // emit_content_changed 内部 promote 之后（cache hit 不排版）。
        let new = self.buffer.snapshot();

        if let Some(params) = composition {
            self.record_composition_commit_transaction(
                &old,
                &new,
                visual_cause,
                params.pending_preedit_cursor_rect,
                params.preedit_byte_start,
                params.preedit_byte_end,
                &params.saved_virtual_text,
                params.candidate_byte_start,
                params.candidate_byte_end,
                params.committed_replace_start,
                params.committed_replace_end,
                params.cancel_reason,
                params.summary_tag,
            );
        } else {
            let _vt = self.record_transaction(old, new, visual_cause, true);
        }
        true
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
            self.buffer.cursor,
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
        let composition = if commit.was_composing && self.current_typing_animation_enabled {
            Some(CompositionCommitParams {
                pending_preedit_cursor_rect: commit.pending_preedit_cursor_rect.clone(),
                preedit_byte_start: commit.preedit_byte_start,
                preedit_byte_end: commit.preedit_byte_end,
                saved_virtual_text: commit.saved_virtual_text.clone(),
                candidate_byte_start: commit.candidate_byte_start,
                candidate_byte_end: commit.candidate_byte_end,
                committed_replace_start: commit.committed_replace_start,
                committed_replace_end: commit.committed_replace_end,
                cancel_reason: "commit_insert",
                summary_tag: "composition_commit",
            })
        } else {
            None
        };

        // 构造 EditOp。先读出 buffer 状态和 commit 的 session replace range，
        // 避免 commit 被 composition 消耗后无法访问。
        let was_composing_replace =
            commit.was_composing && commit.session_replace_start != commit.session_replace_end;
        let session_replace_start = commit.session_replace_start;
        let session_replace_end = commit.session_replace_end;
        let cursor = self.buffer.cursor;
        let (sel_start, sel_end) = self.buffer.selection_range();

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

        // Issue #701 评论 5699573227 第三阶段: 普通输入与 IME commit 共用
        // record_edit_transaction 统一入口。insert_text_with_cause 总是
        // emit_content_changed（保持原行为，即使 pipeline edit 未应用）。
        let _applied = self.record_edit_transaction(op, visual_cause, composition);

        self.pipeline.finish_composition_commit();

        // Issue #701 评论 5699573227 第三阶段 (F2/F7): 不再把
        // pending_preedit_cursor_rect 反写到 cursor_ctrl.visual_x/visual_y。
        // pending_preedit_cursor_rect 只作为 IME commit 动画的 old caret 起点
        // （已传给 record_composition_commit_transaction 的 old_cursor_rect）。
        // 提交后的 target caret 来自 new selection/head 在 new layout 中的 caret,
        // 由 emit_content_changed → update_cursor_visual_position 统一计算。
        // visual_x/visual_y 只是屏幕动画位置，不与 target 互相反写。

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
    /// 只在 `record_edit_transaction` 末尾做一次 `sync_buffer_from_pipeline`
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
            self.buffer.cursor,
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
        let committed_replace_start = if let Some((sel_start, _)) = selection_byte_range {
            sel_start
        } else {
            rep_start
        };
        let committed_replace_end = if let Some((_, sel_end)) = selection_byte_range {
            sel_end
        } else {
            rep_end
        };

        let visual_cause = if inserted.chars().count() == 1 {
            EditorTransactionCause::Typing
        } else {
            EditorTransactionCause::TypingCommit
        };

        let composition = if commit.was_composing && self.current_typing_animation_enabled {
            Some(CompositionCommitParams {
                pending_preedit_cursor_rect: commit.pending_preedit_cursor_rect.clone(),
                preedit_byte_start: commit.preedit_byte_start,
                preedit_byte_end: commit.preedit_byte_end,
                saved_virtual_text: commit.saved_virtual_text.clone(),
                candidate_byte_start,
                candidate_byte_end,
                committed_replace_start,
                committed_replace_end,
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

        // Issue #701 评论 5699573227 第三阶段: IME replace+commit 与普通输入/删除
        // 共用 record_edit_transaction 统一入口。
        let _applied = self.record_edit_transaction(op, visual_cause, composition);

        self.pipeline.finish_composition_commit();

        // Issue #701 评论 5699573227 第三阶段 (F2/F7): 不再把
        // pending_preedit_cursor_rect 反写到 cursor_ctrl.visual_x/visual_y。
        // pending_preedit_cursor_rect 只作为 IME commit 动画的 old caret 起点
        // （已传给 record_composition_commit_transaction 的 old_cursor_rect）。
        // 提交后的 target caret 来自 new selection/head 在 new layout 中的 caret，
        // 由 emit_content_changed → update_cursor_visual_position 统一计算。

        self.emit_content_changed();
    }

    pub(crate) fn delete_backward(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        let cursor = self.buffer.cursor;
        let (start, end) = if self.buffer.has_selection() {
            self.buffer.selection_range()
        } else {
            // 无选区时删除前一个字符；行首时 prev_char_boundary 返回 None，直接返回。
            match prev_char_boundary(&self.buffer.text, cursor) {
                Some(prev) => (prev, cursor),
                None => return,
            }
        };

        // Issue #701 评论 5699573227 第三阶段: 普通删除与普通输入/IME commit
        // 共用 record_edit_transaction 统一入口。跨行删除的 old/new caret 都从
        // 同一对 snapshot 读取（record_transaction 内部排版 old/new 后取 caret），
        // 解决删除到上一行时回抽/跳一下。
        let op = EditOp::Delete {
            start,
            end,
            pipeline_cause: EditorTransactionCause::Delete,
        };
        if self.record_edit_transaction(op, EditorTransactionCause::Delete, None) {
            self.emit_content_changed();
        }
    }

    pub(crate) fn delete_forward(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        let cursor = self.buffer.cursor;
        let (start, end) = if self.buffer.has_selection() {
            self.buffer.selection_range()
        } else {
            // 无选区时删除后一个字符；行末时 next_char_boundary 返回 None，直接返回。
            match next_char_boundary(&self.buffer.text, cursor) {
                Some(next) => (cursor, next),
                None => return,
            }
        };

        let op = EditOp::Delete {
            start,
            end,
            pipeline_cause: EditorTransactionCause::Delete,
        };
        if self.record_edit_transaction(op, EditorTransactionCause::Delete, None) {
            self.emit_content_changed();
        }
    }

    pub(crate) fn delete_selection(&mut self) {
        if !self.current_editor_enabled || !self.buffer.has_selection() {
            return;
        }
        let (start, end) = self.buffer.selection_range();

        let op = EditOp::Delete {
            start,
            end,
            pipeline_cause: EditorTransactionCause::Delete,
        };
        if self.record_edit_transaction(op, EditorTransactionCause::Delete, None) {
            self.emit_content_changed();
        }
    }

    pub(crate) fn select_all(&mut self) {
        let text_len = self.buffer.text.len();
        let _ = self.pipeline.set_selection(0, text_len);
        self.sync_buffer_from_pipeline();
        self.bump_visual_revision();
        self.adjust_affinity_at_wrap_boundary();
        self.cursor_position_changed();
        self.selection_changed();
        self.request_static_repaint();
    }

    pub(crate) fn selected_text(&self) -> QString {
        self.buffer.selected_text().into()
    }

    pub(crate) fn undo(&mut self) {
        let old = self.buffer.snapshot();
        if self.pipeline.perform_undo().is_some() {
            self.sync_buffer_from_pipeline();
            // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
            let new = self.buffer.snapshot();
            self.record_transaction(old, new, EditorTransactionCause::Undo, true);
            self.emit_content_changed();
        }
    }

    pub(crate) fn redo(&mut self) {
        let old = self.buffer.snapshot();
        if self.pipeline.perform_redo().is_some() {
            self.sync_buffer_from_pipeline();
            // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
            let new = self.buffer.snapshot();
            self.record_transaction(old, new, EditorTransactionCause::Redo, true);
            self.emit_content_changed();
        }
    }

    pub(crate) fn handle_key(&mut self, key: i32, modifiers: i32) -> bool {
        input::handle_key(self, key, modifiers)
    }

    pub(crate) fn click_at(&mut self, x: f32, y: f32, extend: bool) {
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        self.cursor_ctrl.affinity = affinity;
        // Issue #702 评论 5707449688 问题 1: 普通鼠标单击不再无条件 force_snap_next。
        // drag_select_at/long_press_at/select_word_at 仍保留 force_snap_next=true，
        // 因为它们确实应该立即对齐。普通单击只更新逻辑 cursor/affinity，
        // 然后让 update_cursor_visual_position() 从当前 visual_x/visual_y rebase
        // 到新 target，走 Tween 路径。
        editor_debug_log(&format!(
            "click_at: mouse_x={:.1}, mouse_y={:.1}, current_scroll_y={:.1}, hit_index={}, affinity={:?}, extend={}",
            x, y, self.current_scroll_y, index, affinity, extend
        ));
        let _ = self.pipeline.set_selection(
            if extend {
                self.buffer.selection_anchor
            } else {
                index
            },
            index,
        );
        self.sync_buffer_from_pipeline();
        self.bump_visual_revision();
        self.pipeline.composition_mut().clear();
        self.cursor_position_changed();
        self.selection_changed();
        self.cursor_ctrl.dirty = true;
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn drag_select_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        self.cursor_ctrl.affinity = affinity;
        // Issue #705: 鼠标点击路径里不要自己单独决定光标动画模式。
        // 是否 Tween 由统一的光标移动规则决定。drag_select 走统一 snap 辅助方法。
        self.snap_cursor_for_pointer_action();
        let _ = self
            .pipeline
            .set_selection(self.buffer.selection_anchor, index);
        self.sync_buffer_from_pipeline();
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn long_press_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        self.cursor_ctrl.affinity = affinity;
        // Issue #705: 统一 snap 辅助方法,不在点击代码里自己强制 Snap。
        self.snap_cursor_for_pointer_action();
        if !self.buffer.has_selection() {
            self.select_word_at_impl(index);
        }
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
        self.context_menu_requested(x, y);
    }

    pub(crate) fn select_word_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        self.cursor_ctrl.affinity = affinity;
        // Issue #705: 统一 snap 辅助方法,不在点击代码里自己强制 Snap。
        self.snap_cursor_for_pointer_action();
        self.select_word_at_impl(index);
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    /// Issue #705: 鼠标点击路径统一的光标 snap 辅助方法。
    ///
    /// drag_select_at/long_press_at/select_word_at 都走此方法设置
    /// force_snap_next,不在每个点击方法里自己单独决定光标动画模式。
    /// 是否 Tween 由统一的光标移动规则(update_cursor_visual_position)决定。
    fn snap_cursor_for_pointer_action(&mut self) {
        self.cursor_ctrl.force_snap_next = true;
    }

    pub(crate) fn select_word_at_impl(&mut self, index: usize) {
        let text = &self.buffer.text;
        if text.is_empty() || index > text.len() {
            return;
        }
        let char_index = byte_to_char_index(text, index);
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return;
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

        let _ = self.pipeline.set_selection(byte_start, byte_end);
        self.sync_buffer_from_pipeline();
    }

    pub(crate) fn clipboard_copy(&mut self) -> bool {
        if !self.buffer.has_selection() {
            return false;
        }
        let text = self.buffer.selected_text();
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
        let next = if forward {
            next_char_boundary(&self.buffer.text, self.buffer.cursor).unwrap_or(self.buffer.cursor)
        } else {
            prev_char_boundary(&self.buffer.text, self.buffer.cursor).unwrap_or(self.buffer.cursor)
        };
        if next == self.buffer.cursor && !extend {
            return;
        }
        self.cursor_ctrl.affinity = if forward {
            CaretAffinity::Downstream
        } else {
            CaretAffinity::Upstream
        };
        if extend {
            let anchor = self.buffer.selection_anchor;
            let _ = self.pipeline.set_selection(anchor, next);
        } else {
            let _ = self.pipeline.set_selection(next, next);
        }
        self.sync_buffer_from_pipeline();
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        // Issue #679 评论 5657313927 (7a): CursorOnly 的创建统一放到
        // update_cursor_visual_position() 里，这里不再手动调 handle_cursor_only。
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn move_cursor_vertical(&mut self, down: bool, extend: bool) {
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
        let index = self.index_at_line_x(&lines[target_idx], x);
        self.cursor_ctrl.affinity = self
            .editor_layout
            .affinity_for_index_on_line(&lines[target_idx], index);
        if extend {
            let anchor = self.buffer.selection_anchor;
            let _ = self.pipeline.set_selection(anchor, index);
        } else {
            let _ = self.pipeline.set_selection(index, index);
        }
        self.sync_buffer_from_pipeline();
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        // Issue #679 评论 5657313927 (7a): CursorOnly 的创建统一放到
        // update_cursor_visual_position() 里，这里不再手动调 handle_cursor_only。
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn move_to_line_edge(&mut self, end: bool, extend: bool) {
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
        self.cursor_ctrl.affinity = affinity;
        if extend {
            let anchor = self.buffer.selection_anchor;
            let _ = self.pipeline.set_selection(anchor, index);
        } else {
            let _ = self.pipeline.set_selection(index, index);
        }
        self.sync_buffer_from_pipeline();
        self.cursor_position_changed();
        self.selection_changed();
        // Issue #679 评论 5657313927 (7a): CursorOnly 的创建统一放到
        // update_cursor_visual_position() 里，这里不再手动调 handle_cursor_only。
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }
}
