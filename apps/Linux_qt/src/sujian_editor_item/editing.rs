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
            && self.animation_coordinator.has_active_insert()
        {
            CursorBlinkMode::Suppressed
        } else {
            CursorBlinkMode::Normal
        };

        let active_progress = self.animation_coordinator.active_cursor_progress();
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
        self.buffer.undo_stack.clear();
        self.buffer.redo_stack.clear();
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

        if !self.preedit_text.is_empty() && self.pending_preedit_cursor_rect.is_none() {
            self.pending_preedit_cursor_rect = self.preedit_cursor_rect.clone();
        }
        let pending_pcr = self.pending_preedit_cursor_rect.take();
        let was_composing = !self.preedit_text.is_empty() || self.composition_session.is_some();

        let (preedit_byte_start, preedit_byte_end) = self.preedit_byte_range_in_virtual_text();
        let saved_virtual_text = self.composition_session.as_ref().map(|s| s.virtual_text()).unwrap_or_default();
        let session_replace_start = self.composition_session.as_ref().map(|s| s.replace_start).unwrap_or(self.buffer.cursor);
        let session_replace_end = self.composition_session.as_ref().map(|s| s.replace_end_exclusive).unwrap_or(self.buffer.cursor);
        let candidate_byte_start = session_replace_start;
        let candidate_byte_end = session_replace_start + inserted.len();
        let committed_replace_start = session_replace_start;
        let committed_replace_end = session_replace_end;

        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
        self.last_preedit_visual_transaction_json = "".into();

        let old = self.buffer.snapshot();
        self.buffer.push_undo(old.clone());

        if was_composing && session_replace_start != session_replace_end {
            self.buffer.text.replace_range(session_replace_start..session_replace_end, &inserted);
            self.buffer.cursor = session_replace_start + inserted.len();
            self.buffer.cursor = crate::sujian_editor_item::buffer::clamp_to_char_boundary(&self.buffer.text, self.buffer.cursor);
            self.buffer.selection_anchor = self.buffer.cursor;
        } else {
            self.buffer.replace_selection_or_insert(&inserted);
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

        if was_composing && self.current_typing_animation_enabled {
            let width = self.bounding_width();
            let old_cursor_rect = pending_pcr.as_ref().map(|c| CursorRect { x: c.x, top: c.top, bottom: c.bottom, baseline_y: c.baseline_y });

            let old_snapshot = self.animation_coordinator
                .active_composition_new_snapshot()
                .cloned()
                .unwrap_or_else(|| {
                    self.current_layout_snapshot.clone().unwrap_or_else(|| {
                        self.build_editor_layout_snapshot(width)
                    })
                });

            let transaction = self.engine.create_transaction(
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
            self.animation_coordinator.cancel_active_composition("commit_insert");

            let new_snapshot = self.build_editor_layout_snapshot(width);
            let new_cursor_rect = new_snapshot.caret_rect.as_ref().map(|c| CursorRect { x: c.x, top: c.y, bottom: c.y + c.h, baseline_y: c.y + c.h * 0.8 });

            let visual_text_unchanged = !saved_virtual_text.is_empty() && saved_virtual_text == new.text;

            let key = self.animation_coordinator.handle_composition_commit_or_cancel(
                self.current_typing_animation_duration_ms as u64,
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
            );

            if let Some(key) = key {
                self.prepare_transaction_textures(key);
            }
            self.previous_layout_snapshot = Some(old_snapshot);
            self.current_layout_snapshot = Some(new_snapshot);

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
            self.visual_transaction_changed();
        } else {
            let _vt = self.record_transaction(old, new, cause, true);
        }

        self.composition_session = None;

        if let Some(pcr) = pending_pcr {
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

        if !self.preedit_text.is_empty() && self.pending_preedit_cursor_rect.is_none() {
            self.pending_preedit_cursor_rect = self.preedit_cursor_rect.clone();
        }
        let pending_pcr = self.pending_preedit_cursor_rect.take();
        let was_composing = !self.preedit_text.is_empty() || self.composition_session.is_some();
        let saved_virtual_text = self.composition_session.as_ref().map(|s| s.virtual_text()).unwrap_or_default();

        let (preedit_byte_start, preedit_byte_end) = self.preedit_byte_range_in_virtual_text();

        let session_replace_start = self.composition_session
            .as_ref()
            .map(|s| s.replace_start)
            .unwrap_or(self.buffer.cursor);
        let session_replace_end = self.composition_session
            .as_ref()
            .map(|s| s.replace_end_exclusive)
            .unwrap_or(self.buffer.cursor);

        let text_str = &self.buffer.text;

        fn utf16_offset_to_utf8_byte(text: &str, base: usize, utf16_offset: i32) -> usize {
            if utf16_offset == 0 {
                return base;
            }
            let start = base;
            if utf16_offset > 0 {
                let mut utf16_count = 0i32;
                let mut byte_pos = start;
                for ch in text[start..].chars() {
                    if utf16_count >= utf16_offset {
                        break;
                    }
                    utf16_count += ch.len_utf16() as i32;
                    byte_pos += ch.len_utf8();
                }
                byte_pos
            } else {
                let abs_offset = (-utf16_offset) as usize;
                let mut utf16_count = 0usize;
                let mut byte_pos = start;
                let before_cursor: Vec<char> = text[..start].chars().collect();
                for &ch in before_cursor.iter().rev() {
                    if utf16_count >= abs_offset {
                        break;
                    }
                    utf16_count += ch.len_utf16();
                    byte_pos -= ch.len_utf8();
                }
                byte_pos
            }
        }

        let anchor_byte = session_replace_start;

        let qt_replace_start_byte = utf16_offset_to_utf8_byte(text_str, anchor_byte, replace_start);
        let qt_replace_start_byte = qt_replace_start_byte.min(text_str.len());

        let qt_replace_end_byte = if replace_length > 0 {
            let mut utf16_count = 0i32;
            let mut byte_pos = qt_replace_start_byte;
            for ch in text_str[qt_replace_start_byte..].chars() {
                if utf16_count >= replace_length {
                    break;
                }
                utf16_count += ch.len_utf16() as i32;
                byte_pos += ch.len_utf8();
            }
            byte_pos
        } else {
            qt_replace_start_byte
        };
        let qt_replace_end_byte = qt_replace_end_byte.min(text_str.len());

        let (qt_rs, qt_re) = if qt_replace_start_byte <= qt_replace_end_byte {
            (qt_replace_start_byte, qt_replace_end_byte)
        } else {
            (qt_replace_end_byte, qt_replace_start_byte)
        };

        let del_start = session_replace_start.min(qt_rs);
        let del_end = session_replace_end.max(qt_re);

        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
        self.last_preedit_visual_transaction_json = "".into();

        let old = self.buffer.snapshot();
        self.buffer.push_undo(old.clone());

        self.buffer.text.replace_range(del_start..del_end, &inserted);
        self.buffer.cursor = del_start + inserted.len();
        self.buffer.cursor = crate::sujian_editor_item::buffer::clamp_to_char_boundary(&self.buffer.text, self.buffer.cursor);
        self.buffer.selection_anchor = self.buffer.cursor;

        self.adjust_affinity_at_wrap_boundary();
        let cause = if inserted.chars().count() == 1 {
            EditorTransactionCause::Typing
        } else {
            EditorTransactionCause::TypingCommit
        };
        let new = self.buffer.snapshot();

        if was_composing && self.current_typing_animation_enabled {
            let width = self.bounding_width();
            let old_cursor_rect = pending_pcr.as_ref().map(|c| CursorRect { x: c.x, top: c.top, bottom: c.bottom, baseline_y: c.baseline_y });

            let old_snapshot = self.animation_coordinator
                .active_composition_new_snapshot()
                .cloned()
                .unwrap_or_else(|| {
                    self.current_layout_snapshot.clone().unwrap_or_else(|| {
                        self.build_editor_layout_snapshot(width)
                    })
                });

            let transaction = self.engine.create_transaction(
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
            self.animation_coordinator.cancel_active_composition("commit_replace");

            let new_snapshot = self.build_editor_layout_snapshot(width);
            let new_cursor_rect = new_snapshot.caret_rect.as_ref().map(|c| CursorRect { x: c.x, top: c.y, bottom: c.y + c.h, baseline_y: c.y + c.h * 0.8 });

            let candidate_byte_start = del_start;
            let candidate_byte_end = del_start + inserted.len();

            let committed_replace_start = del_start;
            let committed_replace_end = del_end;

            let key = self.animation_coordinator.handle_composition_commit_or_cancel(
                self.current_typing_animation_duration_ms as u64,
                &old_snapshot,
                &new_snapshot,
                preedit_byte_start,
                preedit_byte_end,
                true,
                !saved_virtual_text.is_empty() && saved_virtual_text == new.text,
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
            self.previous_layout_snapshot = Some(old_snapshot);
            self.current_layout_snapshot = Some(new_snapshot);

            self.last_event_count = 1;
            self.last_summary = format!(
                "cause={:?};changes={};vt=composition_commit_replace;animate=true",
                transaction.cause,
                transaction.changes.len(),
            ).into();

            self.transaction_created();
            self.visual_transaction_changed();
        } else {
            let _vt = self.record_transaction(old, new, cause, true);
        }

        self.composition_session = None;

        if let Some(pcr) = pending_pcr {
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
        let old = self.buffer.snapshot();

        if !self.buffer.delete_backward() {
            return;
        }
        self.buffer.push_undo(old.clone());
        self.adjust_affinity_at_wrap_boundary();
        let new = self.buffer.snapshot();

        let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);

        self.emit_content_changed();
    }

    pub(crate) fn delete_forward(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        let old = self.buffer.snapshot();

        if !self.buffer.delete_forward() {
            return;
        }
        self.buffer.push_undo(old.clone());
        self.adjust_affinity_at_wrap_boundary();
        let new = self.buffer.snapshot();

        let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);

        self.emit_content_changed();
    }

    pub(crate) fn delete_selection(&mut self) {
        if !self.current_editor_enabled || !self.buffer.has_selection() {
            return;
        }
        let old = self.buffer.snapshot();

        if !self.buffer.delete_selection() {
            return;
        }
        self.buffer.push_undo(old.clone());
        self.adjust_affinity_at_wrap_boundary();
        let new = self.buffer.snapshot();

        let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);

        self.emit_content_changed();
    }

    pub(crate) fn select_all(&mut self) {
        self.buffer.select_all();
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
        let Some((old, new)) = self.buffer.undo() else {
            return;
        };
        self.adjust_affinity_at_wrap_boundary();
        self.record_transaction(old, new, EditorTransactionCause::Undo, true);
        self.emit_content_changed();
    }

    pub(crate) fn redo(&mut self) {
        let Some((old, new)) = self.buffer.redo() else {
            return;
        };
        self.adjust_affinity_at_wrap_boundary();
        self.record_transaction(old, new, EditorTransactionCause::Redo, true);
        self.emit_content_changed();
    }

    pub(crate) fn handle_key(&mut self, key: i32, modifiers: i32) -> bool {
        input::handle_key(self, key, modifiers)
    }

    pub(crate) fn click_at(&mut self, x: f32, y: f32, extend: bool) {
        let (index, affinity) = self.hit_test(x as f64, y as f64);
        self.cursor_ctrl.affinity = affinity;
        self.cursor_ctrl.force_snap_next = true;
        editor_debug_log(&format!(
            "click_at: mouse_x={:.1}, mouse_y={:.1}, current_scroll_y={:.1}, hit_index={}, affinity={:?}, extend={}",
            x, y, self.current_scroll_y, index, affinity, extend
        ));
        self.buffer.move_cursor(index, extend);
        self.bump_visual_revision();
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
        self.pending_preedit_cursor_rect = None;
        self.last_preedit_visual_transaction_json = "".into();
        self.cursor_position_changed();
        self.selection_changed();
        self.cursor_ctrl.dirty = true;
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn drag_select_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(x as f64, y as f64);
        self.cursor_ctrl.affinity = affinity;
        self.cursor_ctrl.force_snap_next = true;
        self.buffer.move_cursor(index, true);
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn long_press_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(x as f64, y as f64);
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
        let (index, affinity) = self.hit_test(x as f64, y as f64);
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

        self.buffer.selection_anchor = byte_start;
        self.buffer.cursor = byte_end;
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
        let result = ClipboardAndFocusAdapter::execute_clipboard(&mut self.clipboard_adapter, ClipboardRequest::Copy { text });
        matches!(result, ClipboardResult::Copied)
    }

    pub(crate) fn clipboard_paste(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        use writer_core::platform_interaction::clipboard_focus::{ClipboardAndFocusAdapter, ClipboardRequest, ClipboardResult};
        let result = ClipboardAndFocusAdapter::execute_clipboard(&mut self.clipboard_adapter, ClipboardRequest::Paste);
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
        self.buffer.move_cursor(next, extend);
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        let new_cursor_rect = self.current_cursor_rect_for_transaction();
        if self.current_smooth_cursor_enabled && !extend {
            self.animation_coordinator.handle_cursor_only(
                self.current_cursor_animation_duration_ms as u64,
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
        self.buffer.move_cursor(index, extend);
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        let new_cursor_rect = self.current_cursor_rect_for_transaction();
        if self.current_smooth_cursor_enabled && !extend {
            self.animation_coordinator.handle_cursor_only(
                self.current_cursor_animation_duration_ms as u64,
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
        self.buffer.move_cursor(index, extend);
        self.bump_visual_revision();
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        let new_cursor_rect = self.current_cursor_rect_for_transaction();
        if self.current_smooth_cursor_enabled && !extend {
            self.animation_coordinator.handle_cursor_only(
                self.current_cursor_animation_duration_ms as u64,
                old_cursor_rect,
                new_cursor_rect,
            );
        }
        self.request_static_repaint();
    }
}
