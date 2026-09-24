use super::animation::find_line_geometry_in_snapshot;
use super::layout_revision::LayoutRevision;
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
        // Issue #722 评论 5749791161 问题2: 不再使用 pending_preedit_cursor_rect，
        // old caret 从 old_snapshot.caret_rect_doc 获取（文档坐标）。
        _pending_preedit_cursor_rect: Option<CursorRect>,
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
        // Issue #710 评论 5734666497: old/new snapshot 的 composition range 分属不同坐标系。
        // old_snapshot 只接 old virtualText range（preedit 在 old virtualText 中的范围）；
        // new_snapshot 只接 new committed text range（candidate 在 new committed text 中的范围）。
        // Issue #710 评论 5735006606: snapshot 的视觉提取范围不能直接等于 raw edit range。
        // raw new range 可以是零长度（空 commit + replacement 纯删除时 candidate_byte_start ==
        // candidate_byte_end），零长度时 build_editor_layout_snapshot 的
        // `if affected_start < affected_end` 为 false，不生成任何动画视觉资源。
        // 用 compute_affected_paragraph_ranges 把 raw edit range 扩展到所在段落边界，
        // 即使 candidate 是 (cursor, cursor)，也会扩成所在段落的非空视觉范围。
        let (old_affected_start, old_affected_end, new_affected_start, new_affected_end) =
            crate::editor::layout::compute_affected_paragraph_ranges(
                saved_virtual_text,
                &new.text,
                (preedit_byte_start, preedit_byte_end),
                (candidate_byte_start, candidate_byte_end),
            );
        let old_composition_range = Some((old_affected_start, old_affected_end));
        let new_composition_range = Some((new_affected_start, new_affected_end));
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
                        self.build_editor_layout_snapshot(width, false, old_composition_range)
                    })
            });

        // Issue #735: EditorEngine 已删除，不再调用 create_transaction。
        // composition commit 的动画由 handle_composition_commit_or_cancel 直接处理，
        // 不需要 EditorTransaction 中间结构。
        // Issue #738 评论 5798704669 问题1: 不再在 commit 路径前置 cancel_active_composition。
        // 旧 CompositionUpdate transaction 留在队列，prepare_composition_commit_handoff
        // 在旧事务仍活着时采样 rebase frame + caret handoff（采到真实当前帧），
        // take_rebase_frames 自己 cancel 被覆盖的旧 composition transaction。
        // 顺序：prepare → reconcile → handle(create) → commit。
        let change_count = super::edit_motion::diff_plain_text(&old.text, &new.text).len();

        // Issue #658 评论 5623746506 问题 2b: composition commit 的 new text
        // 走 Promote=true，generation 直接成为 current，不再用完即删。
        // Issue #738 评论 5797637204: 用共用 helper 同时拿 new_snapshot 和 new_canonical，
        // 让 composition commit 路径能把新 canonical 提交到 Pipeline.current_canonical_snapshot
        // 并作为 reconcile_active_transactions_with_canonical 的新 canonical 几何。
        // 一次排版同时产出两份视图，不再单独排一次 canonical。
        let (new_snapshot, new_canonical) =
            self.build_editor_layout_snapshot_with_canonical(width, true, new_composition_range);
        // Issue #722 评论 5749791161 问题2+3: IME commit 路径使用文档坐标的 caret_rect_doc，
        // 不再用 viewport 坐标的 caret_rect（避免重复减 scroll_y）。
        let new_cursor_rect = new_snapshot.caret_rect_doc.as_ref().map(|c| CursorRect {
            x: c.x,
            top: c.y,
            bottom: c.y + c.h,
            baseline_y: c.baseline_y,
        });
        let new_cursor_visual_line_id = new_snapshot
            .caret_rect_doc
            .as_ref()
            .map(|c| c.visual_line_id);

        // Issue #722 评论 5749791161: old_cursor_rect 从 pending_preedit_cursor_rect
        // 获取（它保存的是 preedit 状态下的 caret 位置）。但 pending_preedit_cursor_rect
        // 存的是 viewport 坐标，需要改为文档坐标。最简单的方案：从 old_snapshot 的
        // caret_rect_doc 获取（commit 时 old caret 就是 preedit 状态下的 caret 位置）。
        // 但 old_snapshot 可能是 active_composition_new_snapshot（也是 viewport 坐标）。
        // 为了保持一致性，从 old_snapshot.caret_rect_doc 获取文档坐标的 old caret。
        let old_cursor_rect = old_snapshot.caret_rect_doc.as_ref().map(|c| CursorRect {
            x: c.x,
            top: c.y,
            bottom: c.y + c.h,
            baseline_y: c.baseline_y,
        });
        let old_cursor_visual_line_id = old_snapshot
            .caret_rect_doc
            .as_ref()
            .map(|c| c.visual_line_id);

        // Issue #722 评论 5749791161: 从 snapshot 的 line_snapshots 中查找行几何。
        let (old_line_top, old_line_bottom) =
            find_line_geometry_in_snapshot(&old_snapshot, old_cursor_visual_line_id);
        let (new_line_top, new_line_bottom) =
            find_line_geometry_in_snapshot(&new_snapshot, new_cursor_visual_line_id);

        let visual_text_unchanged =
            !saved_virtual_text.is_empty() && saved_virtual_text == new.text;

        // Issue #738 评论 5797637204: composition commit 路径走 canonical basis 闭环，
        // 与普通正文路径 pipeline.rs::prepare_edit_motion 保持同一结构：
        //   1. 生成新 LayoutRevision（不再用旧 self.pipeline.layout_revision() 当 basis）；
        //   2. 用统一 edit_now 采样（与普通路径 prepare_edit_motion 行 1398 一致）；
        //   3. cancel_active_composition 已在前面结束旧 composition transaction，
        //      这里对队列里其余旧活动事务 reconcile 到新 canonical（retire CaretDriven
        //      + rebind Timed Reflow），让 passive ReflowMove/ReflowCrossFade 全部绑定
        //      committed new canonical；
        //   4. handle_composition_commit_or_cancel 用 new_revision 当新事务 basis；
        //   5. 无条件提交 Pipeline.layout_revision + current_canonical_snapshot
        //      （与普通路径 pipeline.rs:1452/1478 一致），basis 守卫（==/!=）才能正确
        //      识别旧事务过期，canonical 正文立即接管。
        // 顺序：prepare handoff → reconcile passive reflow → 提升 canonical →
        //       创建新 revision 的事务。
        let new_revision = LayoutRevision::next();
        let edit_now = std::time::Instant::now();
        // Issue #738 评论 5798704669 问题1: prepare 阶段——在旧 CompositionUpdate
        // 仍活着时采 rebase frames + caret handoff，用外层统一 edit_now 采样。
        // take_rebase_frames 自己 cancel 被覆盖的旧 composition transaction。
        let prepared_handoff = self
            .pipeline
            .animation_coordinator_mut()
            .prepare_composition_commit_handoff(
                &old_snapshot,
                &new_snapshot,
                preedit_byte_start,
                preedit_byte_end,
                true,
                candidate_byte_start,
                candidate_byte_end,
                committed_replace_start,
                committed_replace_end,
                self.cursor_ctrl.cursor_owner_epoch,
                edit_now,
            );
        self.pipeline
            .animation_coordinator_mut()
            .reconcile_active_transactions_with_canonical(
                &new.text,
                &new_canonical,
                new_revision,
                edit_now,
            );
        // Issue #738 评论 5788513592: reconcile 删除 unit / 完成事务后同步按剩余
        // active snapshot ids 收一次 texture cache，不让失去 owner 的纹理一直挂着。
        let active_ids = self
            .pipeline
            .animation_coordinator()
            .collect_active_snapshot_ids();
        self.pipeline
            .texture_cache_mut()
            .retain_active_snapshot_ids(&active_ids);

        // Issue #756: 算出 text/caret/coordinated 三个开关传入 composition 路径。
        let coordinated_anim = self.current_coordinated_animation_enabled;
        let text_anim = coordinated_anim || self.current_typing_animation_enabled;
        let caret_anim = coordinated_anim || self.current_smooth_cursor_enabled;
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
                old_cursor_rect,
                new_cursor_rect,
                old_cursor_visual_line_id,
                new_cursor_visual_line_id,
                old_line_top,
                old_line_bottom,
                new_line_top,
                new_line_bottom,
                self.cursor_ctrl.cursor_owner_epoch,
                new_revision,
                edit_now,
                Some(prepared_handoff),
                text_anim,
                caret_anim,
                coordinated_anim,
            );

        // Issue #738 评论 5797637204: 无条件提交 Pipeline.layout_revision +
        // current_canonical_snapshot，与普通正文路径 pipeline.rs:1452/1478 一致。
        // new_canonical 一旦成为当前 canonical，layout_revision 就必须无条件一起提交，
        // 否则 basis 守卫会把"事务 revision 比 Pipeline 当前 revision 更新"误当合法事务。
        self.pipeline.set_layout_revision(new_revision);
        self.pipeline
            .set_current_canonical_snapshot(Some(new_canonical));

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
            cause, change_count, summary_tag,
        )
        .into();
        editor_animation_debug_log(&format!(
            "record_composition_commit_transaction: cancel_reason={}, cause={:?}, changes={}",
            cancel_reason, cause, change_count,
        ));

        self.transaction_created();
    }

    /// Issue #701 评论 5699573227 第三阶段: 统一编辑事务入口。
    ///
    /// `insert_text_with_cause` / `delete_backward` / `delete_forward` /
    /// `ime_replace_and_insert` / `delete_selection` 全部收口到这一个 helper。
    /// 固定做：
    /// 1. 保存 old text/selection/caret（`self.pipeline.snapshot()`）；
    /// 2. 调一次 pipeline edit command（由 `op` 描述，不再由调用者各自直调）；
    /// 3. 读取 new text/selection/caret（通过 pipeline 只读投影 API）；
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
        let old = self.pipeline.snapshot();

        let edit_result: Option<writer_core::editor::EditorEditResult> = match op {
            EditOp::Insert {
                cursor,
                text,
                pipeline_cause,
            } => self.pipeline.insert_text(cursor, &text, pipeline_cause),
            EditOp::Replace {
                start,
                end,
                text,
                pipeline_cause,
            } => self
                .pipeline
                .replace_range(start, end, &text, pipeline_cause),
            EditOp::Delete {
                start,
                end,
                pipeline_cause,
            } => self.pipeline.delete_range(start, end, pipeline_cause),
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
                self.pipeline.ime_commit(
                    sel_start,
                    sel_end,
                    rep_start,
                    rep_end,
                    &inserted_text,
                    pipeline_cause,
                )
            }
        };
        let applied = edit_result.is_some();
        if !applied {
            return false;
        }
        // Issue #658 评论 5623746506 问题 1: 不在 record_transaction 之前调
        // adjust_affinity_at_wrap_boundary（会触发 ensure_layout_cached 排版 A，
        // 与 record_visual_transaction 排版 B 重复）。affinity 调整移到
        // emit_content_changed 内部 promote 之后（cache hit 不排版）。
        let new = self.pipeline.snapshot();

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
            let result = edit_result
                .as_ref()
                .expect("edit_result is Some when applied is true");
            let _vt = self.record_transaction(old, new, result, true);
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
        // Issue #756: composition commit 动画进入条件 = coordinated || typing。
        let composition = if commit.was_composing
            && (self.current_coordinated_animation_enabled || self.current_typing_animation_enabled)
        {
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

        // Issue #756: composition commit 动画进入条件 = coordinated || typing。
        let composition = if commit.was_composing
            && (self.current_coordinated_animation_enabled || self.current_typing_animation_enabled)
        {
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
        if self.record_edit_transaction(op, EditorTransactionCause::Delete, None) {
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
        if self.record_edit_transaction(op, EditorTransactionCause::Delete, None) {
            self.emit_content_changed();
        }
    }

    pub(crate) fn select_all(&mut self) {
        // Issue #705 评论 5717380886: 全选是非正文事务导致的逻辑 cursor 移动。
        self.begin_manual_cursor_move();
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
        if let Some(result) = self.pipeline.perform_undo() {
            // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
            let new = self.pipeline.snapshot();
            self.record_transaction(old, new, &result, true);
            self.emit_content_changed();
        }
    }

    pub(crate) fn redo(&mut self) {
        let old = self.pipeline.snapshot();
        if let Some(result) = self.pipeline.perform_redo() {
            // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
            let new = self.pipeline.snapshot();
            self.record_transaction(old, new, &result, true);
            self.emit_content_changed();
        }
    }

    pub(crate) fn handle_key(&mut self, key: i32, modifiers: i32) -> bool {
        input::handle_key(self, key, modifiers)
    }

    pub(crate) fn click_at(&mut self, x: f32, y: f32, extend: bool) {
        // Issue #705 评论 5718299909: 先 hit_test 算出最终 caret/selection，确认
        // 真的改变当前 caret/selection 后再 bump epoch。点击当前逻辑 caret 的同一
        // 位置不应把正在播放的正文协同 caret 所有权白白失效。
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        let current_anchor = self.pipeline.selection_anchor();
        let current_cursor = self.pipeline.cursor();
        let new_anchor = if extend { current_anchor } else { index };
        let new_head = index;
        // Issue #705 评论 5717380886: bump cursor_owner_epoch 使活动正文事务失去 caret 所有权。
        // begin_manual_cursor_move 内部 bump cursor_owner_epoch（不清文字事务）。
        // Issue #705 评论 5718299909: 仅在 anchor/head/affinity 真的改变时 bump。
        if new_anchor != current_anchor
            || new_head != current_cursor
            || self.cursor_ctrl.affinity != affinity
        {
            self.begin_manual_cursor_move();
        }
        self.cursor_ctrl.affinity = affinity;
        // Issue #712: 鼠标点击设置 CursorMoveSource::PointerClick，
        // 允许 smooth cursor 开启时跨行 Tween。
        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::PointerClick;
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
                self.pipeline.selection_anchor()
            } else {
                index
            },
            index,
        );
        self.bump_visual_revision();
        self.pipeline.composition_mut().clear();
        self.cursor_position_changed();
        self.selection_changed();
        self.cursor_ctrl.dirty = true;
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn drag_select_at(&mut self, x: f32, y: f32) {
        // Issue #705 评论 5718299909: 先 hit_test，确认 head/affinity 真的改变
        // 再 bump epoch。拖到当前 cursor 同一位置不应 bump。
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        // Issue #705 评论 5717380886: 拖选是非正文事务导致的逻辑 cursor 移动。
        // Issue #705 评论 5718299909: 仅在 head 或 affinity 真的改变时 bump。
        if index != self.pipeline.cursor() || self.cursor_ctrl.affinity != affinity {
            self.begin_manual_cursor_move();
        }
        self.cursor_ctrl.affinity = affinity;
        // Issue #712: 拖选设置 CursorMoveSource::DragSelection，跨行走 Snap。
        // Issue #705: 鼠标点击路径里不要自己单独决定光标动画模式。
        // 是否 Tween 由统一的光标移动规则决定。drag_select 走统一 snap 辅助方法。
        self.snap_cursor_for_pointer_action();
        let _ = self
            .pipeline
            .set_selection(self.pipeline.selection_anchor(), index);
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn long_press_at(&mut self, x: f32, y: f32) {
        // Issue #705 评论 5718299909: 先 hit_test + 预判选词结果，确认
        // selection/affinity 真的改变再 bump epoch。
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        // Issue #705 评论 5717380886: 长按是非正文事务导致的逻辑 cursor 移动。
        // Issue #705 评论 5718299909: 预判最终 selection 是否改变：
        //  - 若已有 selection：不选词，selection 不变，只有 affinity 变才算改变。
        //  - 若无 selection：将选词，算 word bounds 与当前 (anchor, cursor) 比较。
        let current_anchor = self.pipeline.selection_anchor();
        let current_cursor = self.pipeline.cursor();
        let committed_text = self.pipeline.committed_text().to_string();
        let caret_will_change = if self.pipeline.has_selection() {
            self.cursor_ctrl.affinity != affinity
        } else {
            match compute_word_bounds(&committed_text, index) {
                Some((byte_start, byte_end)) => {
                    byte_start != current_anchor
                        || byte_end != current_cursor
                        || self.cursor_ctrl.affinity != affinity
                }
                None => self.cursor_ctrl.affinity != affinity,
            }
        };
        if caret_will_change {
            self.begin_manual_cursor_move();
        }
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
        self.context_menu_requested(x, y);
    }

    pub(crate) fn select_word_at(&mut self, x: f32, y: f32) {
        // Issue #705 评论 5718299909: 先 hit_test + 算 word bounds，确认
        // selection/affinity 真的改变再 bump epoch。
        let (index, affinity) = self.hit_test(f64::from(x), f64::from(y));
        // Issue #705 评论 5717380886: 选词是非正文事务导致的逻辑 cursor 移动。
        // Issue #705 评论 5718299909: 预判 word bounds 是否改变 selection 或 affinity。
        let current_anchor = self.pipeline.selection_anchor();
        let current_cursor = self.pipeline.cursor();
        let committed_text = self.pipeline.committed_text().to_string();
        let caret_will_change = match compute_word_bounds(&committed_text, index) {
            Some((byte_start, byte_end)) => {
                byte_start != current_anchor
                    || byte_end != current_cursor
                    || self.cursor_ctrl.affinity != affinity
            }
            None => self.cursor_ctrl.affinity != affinity,
        };
        if caret_will_change {
            self.begin_manual_cursor_move();
        }
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

    /// Issue #705: 鼠标点击路径统一的光标 snap 辅助方法。
    ///
    /// drag_select_at/long_press_at/select_word_at 都走此方法设置
    /// force_snap_next,不在每个点击方法里自己单独决定光标动画模式。
    /// 是否 Tween 由统一的光标移动规则(update_cursor_visual_position)决定。
    fn snap_cursor_for_pointer_action(&mut self) {
        self.cursor_ctrl.force_snap_next = true;
    }

    /// Issue #705 评论 5717380886: 标记一次"非正文事务导致的逻辑 cursor 移动"。
    ///
    /// 鼠标点击、方向键、Home/End、拖选等路径在方法开头调用本方法，bump
    /// `cursor_owner_epoch`，使当前所有活动正文事务的 `cursor_owner_epoch`
    /// 不再等于当前 epoch。之后 `animation_coordinator` 在驱动 coordinated
    /// caret 前检查到 epoch 不一致，跳过 caret 驱动（文字事务继续播自己的
    /// glyph/reflow，但不再驱动 caret）。
    ///
    /// 普通输入/删除（`insert_text`、`delete_*` 等）**不要**调用本方法，
    /// 它们创建的正文事务应该继续拥有 coordinated caret。
    ///
    /// **不要**在本方法里 `clear_active_text_animations()`，那会把还在正常
    /// 播放的文字动画一起掐掉。epoch 不一致时文字事务继续播自己的 glyph/reflow，
    /// 只是不再驱动 caret。
    fn begin_manual_cursor_move(&mut self) {
        self.cursor_ctrl.bump_cursor_owner_epoch();
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
        if next != current_cursor {
            self.begin_manual_cursor_move();
        }
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
        self.begin_manual_cursor_move();
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
        if index != self.pipeline.cursor() || self.cursor_ctrl.affinity != affinity {
            self.begin_manual_cursor_move();
        }
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
