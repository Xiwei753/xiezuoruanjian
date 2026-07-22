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

    pub(crate) fn tick_cursor_animation(&mut self) {
        use animation_coordinator::CursorBlinkMode;
        let blink_mode = if self.current_coordinated_text_cursor_animation_enabled
            && self.pipeline.animation_coordinator_mut().has_active_insert()
        {
            CursorBlinkMode::Suppressed
        } else {
            CursorBlinkMode::Normal
        };

        let active_progress = self.pipeline.animation_coordinator_mut().active_cursor_progress();
        let still_animating = if self.cursor_ctrl.animation.is_some() {
            if let Some(progress) = active_progress {
                self.cursor_ctrl.update_animation_progress(progress)
            } else {
                self.cursor_ctrl.tick_animation()
            }
        } else {
            false
        };
        let blink_changed = self.cursor_ctrl.tick_blink(blink_mode);
        if still_animating || blink_changed {
            self.cursor_rect_changed();
        }
        if still_animating {
            self.request_frame_update();
        }
    }

    pub(crate) fn clear_undo_stack(&mut self) {
        self.pipeline.clear_undo_redo();
        self.sync_buffer_from_pipeline();
    }

    pub(crate) fn insert_text(&mut self, text: QString) {
        self.insert_text_with_cause(text, None);
    }

    pub(crate) fn insert_text_with_cause(&mut self, text: QString, explicit_cause: Option<EditorTransactionCause>) {
        if !self.current_editor_enabled {
            return;
        }
        let inserted = normalize_plain_text(&text.to_string());
        if inserted.is_empty() {
            return;
        }

        let (preedit_byte_start, preedit_byte_end) = self.preedit_byte_range_in_virtual_text();
        let commit = self.pipeline.prepare_composition_commit(&inserted, self.buffer.cursor, preedit_byte_start, preedit_byte_end);

        let old = self.buffer.snapshot();

        if commit.was_composing && commit.session_replace_start != commit.session_replace_end {
            let _ = self.pipeline.replace_range(commit.session_replace_start, commit.session_replace_end, &inserted, EditorTransactionCause::TypingCommit);
            self.sync_buffer_from_pipeline();
        } else {
            let (sel_start, sel_end) = self.buffer.selection_range();
            if sel_start != sel_end {
                let _ = self.pipeline.replace_range(sel_start, sel_end, &inserted, EditorTransactionCause::Typing);
                self.sync_buffer_from_pipeline();
            } else {
                let _ = self.pipeline.insert_text(self.buffer.cursor, &inserted, EditorTransactionCause::Typing);
                self.sync_buffer_from_pipeline();
            }
        }
        self.adjust_affinity_at_wrap_boundary();
        let cause = explicit_cause.unwrap_or_else(|| {
            if inserted.chars().count() == 1 {
                EditorTransactionCause::Typing
            } else {
                EditorTransactionCause::TypingCommit
            }
        });
        let new = self.buffer.snapshot();

        if commit.was_composing && self.current_typing_animation_enabled {
            let width = self.bounding_width();
            let old_cursor_rect = commit.pending_preedit_cursor_rect.as_ref().map(|c| CursorRect { x: c.x, top: c.top, bottom: c.bottom, baseline_y: c.baseline_y });

            let old_snapshot = self.pipeline.animation_coordinator()
                .active_composition_new_snapshot()
                .cloned()
                .unwrap_or_else(|| {
                    self.pipeline.current_layout_snapshot().clone().unwrap_or_else(|| {
                        self.build_editor_layout_snapshot(width)
                    })
                });

            let transaction = self.pipeline.engine().create_transaction(
                &old.text, &new.text,
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
            self.pipeline.animation_coordinator_mut().cancel_active_composition("commit_insert");

            let new_snapshot = self.build_editor_layout_snapshot(width);
            let new_cursor_rect = new_snapshot.caret_rect.as_ref().map(|c| CursorRect { x: c.x, top: c.y, bottom: c.y + c.h, baseline_y: c.y + c.h * 0.8 });

            let visual_text_unchanged = !commit.saved_virtual_text.is_empty() && commit.saved_virtual_text == new.text;

            let key = self.pipeline.animation_coordinator_mut().handle_composition_commit_or_cancel(
                u64::from(self.current_typing_animation_duration_ms),
                &old_snapshot,
                &new_snapshot,
                commit.preedit_byte_start,
                commit.preedit_byte_end,
                true,
                visual_text_unchanged,
                commit.candidate_byte_start,
                commit.candidate_byte_end,
                commit.committed_replace_start,
                commit.committed_replace_end,
                old_cursor_rect,
                new_cursor_rect,
            );

            if let Some(key) = key {
                self.prepare_transaction_textures(key);
            }
            self.pipeline.set_previous_layout_snapshot(Some(old_snapshot));
            self.pipeline.set_current_layout_snapshot(Some(new_snapshot));

            self.last_event_count = 1;
            self.last_summary = format!(
                "cause={:?};changes={};vt=composition_commit;animate=true",
                transaction.cause,
                transaction.changes.len(),
            ).into();
            editor_animation_debug_log(&format!(
                "insert_text_with_cause: composition commit, cause={:?}, changes={}",
                transaction.cause,
                transaction.changes.len(),
            ));

            self.transaction_created();
        } else {
            let _vt = self.record_transaction(old, new, cause, true);
        }

        self.pipeline.finish_composition_commit();

        if let Some(pcr) = commit.pending_preedit_cursor_rect {
            self.cursor_ctrl.visual_x = pcr.x;
            self.cursor_ctrl.visual_y = pcr.top;
            self.cursor_ctrl.force_snap_next = false;
            editor_animation_debug_log(&format!("[commit] pending_preedit_cursor_rect present cursor_start_source=preedit pcr_x={:.1} pcr_y={:.1}", pcr.x, pcr.top));
        } else {
            editor_animation_debug_log("[commit] pending_preedit_cursor_rect absent cursor_start_source=normal");
        }

        self.emit_content_changed();
    }

    pub(crate) fn ime_replace_and_insert(&mut self, replace_start: i32, replace_length: i32, text: String) {
        if !self.current_editor_enabled {
            return;
        }
        let inserted = normalize_plain_text(&text);
        if inserted.is_empty() {
            return;
        }

        let (preedit_byte_start, preedit_byte_end) = self.preedit_byte_range_in_virtual_text();
        let commit = self.pipeline.prepare_composition_commit(&inserted, self.buffer.cursor, preedit_byte_start, preedit_byte_end);

        let committed_text = self.buffer.text.clone();

        let base_text = format!(
            "{}{}",
            &committed_text[..commit.session_replace_start],
            &committed_text[commit.session_replace_end..]
        );

        fn utf16_forward(text: &str, byte_start: usize, utf16_count: i32) -> usize {
            if utf16_count <= 0 { return byte_start; }
            let mut remaining = utf16_count;
            let mut pos = byte_start;
            for ch in text[byte_start..].chars() {
                if remaining <= 0 { break; }
                remaining -= ch.len_utf16() as i32;
                pos += ch.len_utf8();
            }
            pos.min(text.len())
        }

        fn utf16_backward(text: &str, byte_start: usize, utf16_count: i32) -> usize {
            if utf16_count <= 0 { return byte_start; }
            let mut remaining = utf16_count;
            let mut pos = byte_start;
            for ch in text[..byte_start].chars().rev() {
                if remaining <= 0 { break; }
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
            let _ = self.pipeline.replace_range(del_start, del_end, &inserted, cause);
        } else {
            let _ = self.pipeline.insert_text(del_start, &inserted, cause);
        }
        self.sync_buffer_from_pipeline();

        self.adjust_affinity_at_wrap_boundary();
        let new = self.buffer.snapshot();

        if commit.was_composing && self.current_typing_animation_enabled {
            let width = self.bounding_width();
            let old_cursor_rect = commit.pending_preedit_cursor_rect.as_ref().map(|c| CursorRect { x: c.x, top: c.top, bottom: c.bottom, baseline_y: c.baseline_y });

            let old_snapshot = self.pipeline.animation_coordinator()
                .active_composition_new_snapshot()
                .cloned()
                .unwrap_or_else(|| {
                    self.pipeline.current_layout_snapshot().clone().unwrap_or_else(|| {
                        self.build_editor_layout_snapshot(width)
                    })
                });

            let transaction = self.pipeline.engine().create_transaction(
                &old.text, &new.text,
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
            self.pipeline.animation_coordinator_mut().cancel_active_composition("commit_replace");

            let new_snapshot = self.build_editor_layout_snapshot(width);
            let new_cursor_rect = new_snapshot.caret_rect.as_ref().map(|c| CursorRect { x: c.x, top: c.y, bottom: c.y + c.h, baseline_y: c.y + c.h * 0.8 });

            let key = self.pipeline.animation_coordinator_mut().handle_composition_commit_or_cancel(
                u64::from(self.current_typing_animation_duration_ms),
                &old_snapshot,
                &new_snapshot,
                commit.preedit_byte_start,
                commit.preedit_byte_end,
                true,
                !commit.saved_virtual_text.is_empty() && commit.saved_virtual_text == new.text,
                candidate_byte_start,
                candidate_byte_end,
                committed_replace_start,
                committed_replace_end,
                old_cursor_rect,
                new_cursor_rect,
            );

            if let Some(key) = key {
                self.prepare_transaction_textures(key);
            }
            self.pipeline.set_previous_layout_snapshot(Some(old_snapshot));
            self.pipeline.set_current_layout_snapshot(Some(new_snapshot));

            self.last_event_count = 1;
            self.last_summary = format!(
                "cause={:?};changes={};vt=composition_commit_replace;animate=true",
                transaction.cause,
                transaction.changes.len(),
            ).into();

            self.transaction_created();
        } else {
            let _vt = self.record_transaction(old, new, cause, true);
        }

        self.pipeline.finish_composition_commit();

        if let Some(pcr) = commit.pending_preedit_cursor_rect {
            self.cursor_ctrl.visual_x = pcr.x;
            self.cursor_ctrl.visual_y = pcr.top;
            self.cursor_ctrl.force_snap_next = false;
        }

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
            if self.pipeline.delete_range(start, end, EditorTransactionCause::Delete).is_some() {
                self.sync_buffer_from_pipeline();
                self.adjust_affinity_at_wrap_boundary();
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
        if self.pipeline.delete_range(prev, cursor, EditorTransactionCause::Delete).is_some() {
            self.sync_buffer_from_pipeline();
            self.adjust_affinity_at_wrap_boundary();
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
            if self.pipeline.delete_range(start, end, EditorTransactionCause::Delete).is_some() {
                self.sync_buffer_from_pipeline();
                self.adjust_affinity_at_wrap_boundary();
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
        if self.pipeline.delete_range(cursor, next, EditorTransactionCause::Delete).is_some() {
            self.sync_buffer_from_pipeline();
            self.adjust_affinity_at_wrap_boundary();
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
        if self.pipeline.delete_range(start, end, EditorTransactionCause::Delete).is_some() {
            self.sync_buffer_from_pipeline();
            self.adjust_affinity_at_wrap_boundary();
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
            self.adjust_affinity_at_wrap_boundary();
            let new = self.buffer.snapshot();
            self.record_transaction(old, new, EditorTransactionCause::Undo, true);
            self.emit_content_changed();
        }
    }

    pub(crate) fn redo(&mut self) {
        let old = self.buffer.snapshot();
        if self.pipeline.perform_redo().is_some() {
            self.sync_buffer_from_pipeline();
            self.adjust_affinity_at_wrap_boundary();
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
        let _ = self.pipeline.set_selection(if extend { self.buffer.selection_anchor } else { index }, index);
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
        let _ = self.pipeline.set_selection(self.buffer.selection_anchor, index);
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
        use writer_core::platform_interaction::clipboard_focus::{ClipboardAndFocusAdapter, ClipboardRequest, ClipboardResult};
        let result = ClipboardAndFocusAdapter::execute_clipboard(self.pipeline.clipboard_adapter_mut(), ClipboardRequest::Copy { text });
        matches!(result, ClipboardResult::Copied)
    }

    pub(crate) fn clipboard_paste(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        use writer_core::platform_interaction::clipboard_focus::{ClipboardAndFocusAdapter, ClipboardRequest, ClipboardResult};
        let result = ClipboardAndFocusAdapter::execute_clipboard(self.pipeline.clipboard_adapter_mut(), ClipboardRequest::Paste);
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
        let old_cursor_rect = self.current_cursor_rect_for_transaction();
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
        let _ = self.update_cursor_visual_position();
        let new_cursor_rect = self.current_cursor_rect_for_transaction();
        if self.current_smooth_cursor_enabled && !extend {
            self.pipeline.animation_coordinator_mut().handle_cursor_only(
                u64::from(self.current_cursor_animation_duration_ms),
                old_cursor_rect,
                new_cursor_rect,
            );
        }
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
        let old_cursor_rect = self.current_cursor_rect_for_transaction();
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
        let _ = self.update_cursor_visual_position();
        let new_cursor_rect = self.current_cursor_rect_for_transaction();
        if self.current_smooth_cursor_enabled && !extend {
            self.pipeline.animation_coordinator_mut().handle_cursor_only(
                u64::from(self.current_cursor_animation_duration_ms),
                old_cursor_rect,
                new_cursor_rect,
            );
        }
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
        let old_cursor_rect = self.current_cursor_rect_for_transaction();
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
        let _ = self.update_cursor_visual_position();
        let new_cursor_rect = self.current_cursor_rect_for_transaction();
        if self.current_smooth_cursor_enabled && !extend {
            self.pipeline.animation_coordinator_mut().handle_cursor_only(
                u64::from(self.current_cursor_animation_duration_ms),
                old_cursor_rect,
                new_cursor_rect,
            );
        }
        self.request_static_repaint();
    }
}
