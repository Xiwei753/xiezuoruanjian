use super::*;

impl SujianEditorItem {
    pub(crate) fn current_cursor_rect_for_transaction(&self) -> Option<CursorRect> {
        let x = self.cursor_ctrl.visual_x;
        let y = self.cursor_ctrl.visual_y;
        let h = self.cursor_ctrl.visual_h;
        if h < 0.01 {
            return None;
        }
        Some(CursorRect {
            x,
            top: y,
            bottom: y + h,
            baseline_y: y + h * 0.8,
        })
    }

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

        let old = self.buffer.snapshot();

        if commit.was_composing && commit.session_replace_start != commit.session_replace_end {
            let _ = self.pipeline.replace_range(
                commit.session_replace_start,
                commit.session_replace_end,
                &inserted,
                EditorTransactionCause::TypingCommit,
            );
            self.sync_buffer_from_pipeline();
        } else {
            let (sel_start, sel_end) = self.buffer.selection_range();
            if sel_start != sel_end {
                let _ = self.pipeline.replace_range(
                    sel_start,
                    sel_end,
                    &inserted,
                    EditorTransactionCause::Typing,
                );
                self.sync_buffer_from_pipeline();
            } else {
                let _ = self.pipeline.insert_text(
                    self.buffer.cursor,
                    &inserted,
                    EditorTransactionCause::Typing,
                );
                self.sync_buffer_from_pipeline();
            }
        }
        // Issue #658 评论 5623746506 问题 1: 不在 record_transaction 之前调
        // adjust_affinity_at_wrap_boundary（会触发 ensure_layout_cached 排版 A，
        // 与 record_visual_transaction 排版 B 重复）。affinity 调整移到
        // emit_content_changed 内部 promote 之后（cache hit 不排版）。
        let cause = explicit_cause.unwrap_or_else(|| {
            if inserted.chars().count() == 1 {
                EditorTransactionCause::Typing
            } else {
                EditorTransactionCause::TypingCommit
            }
        });
        let new = self.buffer.snapshot();

        if commit.was_composing && self.current_typing_animation_enabled {
            self.record_composition_commit_transaction(
                &old,
                &new,
                cause,
                commit.pending_preedit_cursor_rect,
                commit.preedit_byte_start,
                commit.preedit_byte_end,
                &commit.saved_virtual_text,
                commit.candidate_byte_start,
                commit.candidate_byte_end,
                commit.committed_replace_start,
                commit.committed_replace_end,
                "commit_insert",
                "composition_commit",
            );
        } else {
            let _vt = self.record_transaction(old, new, cause, true);
        }

        self.pipeline.finish_composition_commit();

        // Issue #701 评论 5699573227 第三阶段 (F2/F7): 不再把
        // pending_preedit_cursor_rect 反写到 cursor_ctrl.visual_x/visual_y。
        // pending_preedit_cursor_rect 只作为 IME commit 动画的 old caret 起点
        // （已传给 record_composition_commit_transaction 的 old_cursor_rect）。
        // 提交后的 target caret 来自 new selection/head 在 new layout 中的 caret，
        // 由 emit_content_changed → update_cursor_visual_position 统一计算。
        // visual_x/visual_y 只是屏幕动画位置，不与 target 互相反写。

        self.emit_content_changed();
    }

    pub(crate) fn ime_replace_and_insert(
        &mut self,
        replace_start: i32,
        replace_length: i32,
        text: String,
    ) {
        if !self.current_editor_enabled {
            return;
        }
        let inserted = normalize_plain_text(&text);
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

        let committed_text = self.buffer.text.clone();

        let base_text = format!(
            "{}{}",
            &committed_text[..commit.session_replace_start],
            &committed_text[commit.session_replace_end..]
        );

        fn utf16_forward(text: &str, byte_start: usize, utf16_count: i32) -> usize {
            if utf16_count <= 0 {
                return byte_start;
            }
            let mut remaining = utf16_count;
            let mut pos = byte_start;
            for ch in text[byte_start..].chars() {
                if remaining <= 0 {
                    break;
                }
                remaining -= ch.len_utf16() as i32;
                pos += ch.len_utf8();
            }
            pos.min(text.len())
        }

        fn utf16_backward(text: &str, byte_start: usize, utf16_count: i32) -> usize {
            if utf16_count <= 0 {
                return byte_start;
            }
            let mut remaining = utf16_count;
            let mut pos = byte_start;
            for ch in text[..byte_start].chars().rev() {
                if remaining <= 0 {
                    break;
                }
                remaining -= ch.len_utf16() as i32;
                pos -= ch.len_utf8();
            }
            pos
        }

        let anchor_in_base = commit.session_replace_start;
        let rs_byte = if replace_start < 0 {
            utf16_backward(&base_text, anchor_in_base, -replace_start)
        } else if replace_start == 0 {
            anchor_in_base
        } else {
            utf16_forward(&base_text, anchor_in_base, replace_start)
        };
        let re_byte = if replace_length > 0 {
            utf16_forward(&base_text, rs_byte, replace_length)
        } else {
            rs_byte
        };
        let (del_start, del_end) = if rs_byte <= re_byte {
            (rs_byte, re_byte)
        } else {
            (re_byte, rs_byte)
        };

        let new_base = format!(
            "{}{}{}",
            &base_text[..del_start],
            inserted,
            &base_text[del_end..]
        );

        let cursor_in_new_base = del_start + inserted.len();

        let _new_text = new_base;
        let _new_cursor = cursor_in_new_base;

        let preedit_len = commit.preedit_byte_end - commit.preedit_byte_start;
        let qt_replace_start_in_vt = if del_start <= commit.session_replace_start {
            del_start
        } else {
            del_start + preedit_len
        };
        let qt_replace_end_in_vt = if del_end <= commit.session_replace_start {
            del_end
        } else {
            del_end + preedit_len
        };

        let committed_replace_start = qt_replace_start_in_vt;
        let committed_replace_end = qt_replace_end_in_vt;

        let candidate_byte_start = del_start;
        let candidate_byte_end = del_start + inserted.len();

        let old = self.buffer.snapshot();

        let cause = if inserted.chars().count() == 1 {
            EditorTransactionCause::Typing
        } else {
            EditorTransactionCause::TypingCommit
        };
        if del_start != del_end {
            let _ = self
                .pipeline
                .replace_range(del_start, del_end, &inserted, cause);
        } else {
            let _ = self.pipeline.insert_text(del_start, &inserted, cause);
        }
        self.sync_buffer_from_pipeline();

        // Issue #658 评论 5623746506 问题 1: 不在 record_transaction 之前调
        // adjust_affinity_at_wrap_boundary。affinity 调整移到 emit_content_changed。
        let new = self.buffer.snapshot();

        if commit.was_composing && self.current_typing_animation_enabled {
            self.record_composition_commit_transaction(
                &old,
                &new,
                cause,
                commit.pending_preedit_cursor_rect,
                commit.preedit_byte_start,
                commit.preedit_byte_end,
                &commit.saved_virtual_text,
                candidate_byte_start,
                candidate_byte_end,
                committed_replace_start,
                committed_replace_end,
                "commit_replace",
                "composition_commit_replace",
            );
        } else {
            let _vt = self.record_transaction(old, new, cause, true);
        }

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
        if self.buffer.has_selection() {
            let (start, end) = self.buffer.selection_range();
            let old = self.buffer.snapshot();
            if self
                .pipeline
                .delete_range(start, end, EditorTransactionCause::Delete)
                .is_some()
            {
                self.sync_buffer_from_pipeline();
                // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
                let new = self.buffer.snapshot();
                let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
                self.emit_content_changed();
            }
            return;
        }
        let Some(prev) = prev_char_boundary(&self.buffer.text, cursor) else {
            return;
        };
        let old = self.buffer.snapshot();
        if self
            .pipeline
            .delete_range(prev, cursor, EditorTransactionCause::Delete)
            .is_some()
        {
            self.sync_buffer_from_pipeline();
            // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
            let new = self.buffer.snapshot();
            let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
            self.emit_content_changed();
        }
    }

    pub(crate) fn delete_forward(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        let cursor = self.buffer.cursor;
        if self.buffer.has_selection() {
            let (start, end) = self.buffer.selection_range();
            let old = self.buffer.snapshot();
            if self
                .pipeline
                .delete_range(start, end, EditorTransactionCause::Delete)
                .is_some()
            {
                self.sync_buffer_from_pipeline();
                // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
                let new = self.buffer.snapshot();
                let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
                self.emit_content_changed();
            }
            return;
        }
        let Some(next) = next_char_boundary(&self.buffer.text, cursor) else {
            return;
        };
        let old = self.buffer.snapshot();
        if self
            .pipeline
            .delete_range(cursor, next, EditorTransactionCause::Delete)
            .is_some()
        {
            self.sync_buffer_from_pipeline();
            // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
            let new = self.buffer.snapshot();
            let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
            self.emit_content_changed();
        }
    }

    pub(crate) fn delete_selection(&mut self) {
        if !self.current_editor_enabled || !self.buffer.has_selection() {
            return;
        }
        let (start, end) = self.buffer.selection_range();
        let old = self.buffer.snapshot();
        if self
            .pipeline
            .delete_range(start, end, EditorTransactionCause::Delete)
            .is_some()
        {
            self.sync_buffer_from_pipeline();
            // Issue #658 评论 5623746506 问题 1: affinity 调整移到 emit_content_changed。
            let new = self.buffer.snapshot();
            let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
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
        self.cursor_ctrl.force_snap_next = true;
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
        self.cursor_ctrl.force_snap_next = true;
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
        self.cursor_ctrl.force_snap_next = true;
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
        self.cursor_ctrl.force_snap_next = true;
        self.select_word_at_impl(index);
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
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
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let Some((line_idx, x)) = self.cursor_line_and_x(&lines) else {
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
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let Some((line_idx, _)) = self.cursor_line_and_x(&lines) else {
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
