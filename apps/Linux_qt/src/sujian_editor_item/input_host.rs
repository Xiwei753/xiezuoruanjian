use super::*;

pub(crate) fn is_left_button_pressed(event: &QMouseEvent) -> bool {
    cpp!(unsafe [event as "const QMouseEvent*"] -> bool as "bool" {
        return event ? (event->buttons() & Qt::LeftButton) : false;
    })
}

impl SujianEditorItem {
    fn ensure_composition_session(&mut self) {
        if self.composition_session.is_none() {
            let cursor = self.buffer.cursor;
            self.composition_session = Some(CompositionSession::new(
                self.text_revision,
                self.visual_revision,
                self.buffer.text.clone(),
                cursor,
            ));
        }
    }

    pub(crate) fn preedit_byte_range_in_virtual_text(&self) -> (usize, usize) {
        if let Some(ref session) = self.composition_session {
            session.preedit_byte_range_in_virtual_text()
        } else {
            (self.buffer.cursor, self.buffer.cursor)
        }
    }

    fn prepare_composition_update(&mut self, text: String, cursor: usize) -> CompositionUpdateData {
        self.ensure_composition_session();

        let (old_preedit, generation, composition_byte_start, composition_byte_end, virtual_text) = {
            let session = self.composition_session.as_mut().unwrap();
            let old_preedit = session.preedit_text.clone();
            session.update_preedit(text, cursor);
            let generation = session.last_submitted_generation;
            let (start, end) = session.preedit_byte_range_in_virtual_text();
            let vt = session.virtual_text();
            (old_preedit, generation, start, end, vt)
        };

        CompositionUpdateData {
            old_preedit,
            generation,
            composition_byte_start,
            composition_byte_end,
            virtual_text,
        }
    }
}

struct CompositionUpdateData {
    old_preedit: String,
    generation: u64,
    composition_byte_start: usize,
    composition_byte_end: usize,
    virtual_text: String,
}

impl EditorInputHost for SujianEditorItem {
    fn input_enabled(&self) -> bool {
        self.current_editor_enabled
    }

    fn input_emit_explicit_clear_requested(&mut self) {
        self.explicit_clear_requested();
    }

    fn input_clipboard_copy(&mut self) -> bool {
        self.clipboard_copy()
    }

    fn input_clipboard_paste(&mut self) {
        self.clipboard_paste();
    }

    fn input_undo(&mut self) {
        self.undo();
    }

    fn input_redo(&mut self) {
        self.redo();
    }

    fn input_select_all(&mut self) {
        self.select_all();
    }

    fn input_delete_selection(&mut self) {
        self.delete_selection();
    }

    fn input_delete_backward(&mut self) {
        self.delete_backward();
    }

    fn input_delete_forward(&mut self) {
        self.delete_forward();
    }

    fn input_insert_text(&mut self, text: String) {
        self.insert_text(text.into());
    }

    fn input_replace_and_insert(&mut self, replace_start: i32, replace_length: i32, text: String) {
        self.ime_replace_and_insert(replace_start, replace_length, text);
    }

    fn input_move_cursor_horizontal(&mut self, forward: bool, extend: bool) {
        self.move_cursor_horizontal(forward, extend);
    }

    fn input_move_cursor_vertical(&mut self, down: bool, extend: bool) {
        self.move_cursor_vertical(down, extend);
    }

    fn input_move_to_line_edge(&mut self, end: bool, extend: bool) {
        self.move_to_line_edge(end, extend);
    }

    fn input_clear_preedit(&mut self) {
        if !self.preedit_text.is_empty() || self.composition_session.is_some() {
            self.pending_preedit_cursor_rect = self.preedit_cursor_rect.clone();

            if self.typing_animation_enabled {
                let (composition_byte_start, composition_byte_end) = self.preedit_byte_range_in_virtual_text();
                let width = self.bounding_width();
                let old_cursor_rect = self.preedit_cursor_rect.as_ref().map(|c| CursorRect { x: c.x, top: c.top, bottom: c.bottom, baseline_y: c.baseline_y });
                let new_cursor_rect = self.current_layout_snapshot.as_ref().and_then(|s| s.caret_rect.as_ref()).map(|c| CursorRect { x: c.x, top: c.y, bottom: c.y + c.h, baseline_y: c.y + c.h * 0.8 });

                let old_snapshot = self.animation_coordinator
                    .active_composition_new_snapshot()
                    .cloned()
                    .unwrap_or_else(|| {
                        self.current_layout_snapshot.clone().unwrap_or_else(|| {
                            self.build_editor_layout_snapshot(width)
                        })
                    });
                let new_snapshot = self.build_editor_layout_snapshot(width);

                self.animation_coordinator.cancel_active_composition("clear_preedit");
                self.animation_coordinator.handle_composition_commit_or_cancel(
                    self.current_typing_animation_duration_ms as u64,
                    &old_snapshot,
                    &new_snapshot,
                    composition_byte_start,
                    composition_byte_end,
                    false,
                    false,
                    composition_byte_start,
                    composition_byte_start,
                    composition_byte_start,
                    composition_byte_start,
                    old_cursor_rect,
                    new_cursor_rect,
                );
            }
        }
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
        self.last_preedit_visual_transaction_json = "".into();
        self.composition_session = None;
        self.update_ime_cursor_for_preedit();
    }

    fn input_set_preedit(&mut self, text: String, cursor: usize) {
        self.preedit_old_text = self.preedit_text.clone();
        self.preedit_text = text.clone();
        self.preedit_cursor = cursor;
        self.preedit_attributes.clear();

        if self.typing_animation_enabled && !text.is_empty() {
            let data = self.prepare_composition_update(text, cursor);
            let width = self.bounding_width();

            let old_snapshot = if data.generation <= 1 || data.old_preedit.is_empty() {
                self.current_layout_snapshot.clone().unwrap_or_else(|| {
                    self.build_editor_layout_snapshot(width)
                })
            } else {
                self.animation_coordinator
                    .active_composition_new_snapshot()
                    .cloned()
                    .unwrap_or_else(|| {
                        self.current_layout_snapshot.clone().unwrap_or_else(|| {
                            self.build_editor_layout_snapshot(width)
                        })
                    })
            };

            let new_snapshot = self.build_virtual_layout_snapshot(&data.virtual_text, width);

            let old_cursor_rect = self.current_layout_snapshot.as_ref().and_then(|s| s.caret_rect.as_ref()).map(|c| CursorRect { x: c.x, top: c.y, bottom: c.y + c.h, baseline_y: c.y + c.h * 0.8 });
            let new_cursor_rect = new_snapshot.caret_rect.as_ref().map(|c| CursorRect { x: c.x, top: c.y, bottom: c.y + c.h, baseline_y: c.y + c.h * 0.8 });

            self.animation_coordinator.handle_composition_update(
                self.current_typing_animation_duration_ms as u64,
                &old_snapshot,
                &new_snapshot,
                data.composition_byte_start,
                data.composition_byte_end,
                old_cursor_rect,
                new_cursor_rect,
            );
        } else {
            self.update_preedit_visual_state();
        }

        self.update_ime_cursor_for_preedit();
        self.request_static_repaint();
    }

    fn input_set_preedit_with_attrs(
        &mut self,
        text: String,
        cursor: usize,
        attributes: Vec<PreeditAttribute>,
    ) {
        self.preedit_old_text = self.preedit_text.clone();
        self.preedit_text = text.clone();
        self.preedit_cursor = cursor;
        self.preedit_attributes = attributes;

        if self.typing_animation_enabled && !text.is_empty() {
            let data = self.prepare_composition_update(text, cursor);
            let width = self.bounding_width();

            let old_snapshot = if data.generation <= 1 || data.old_preedit.is_empty() {
                self.current_layout_snapshot.clone().unwrap_or_else(|| {
                    self.build_editor_layout_snapshot(width)
                })
            } else {
                self.animation_coordinator
                    .active_composition_new_snapshot()
                    .cloned()
                    .unwrap_or_else(|| {
                        self.current_layout_snapshot.clone().unwrap_or_else(|| {
                            self.build_editor_layout_snapshot(width)
                        })
                    })
            };

            let new_snapshot = self.build_virtual_layout_snapshot(&data.virtual_text, width);

            let old_cursor_rect = self.current_layout_snapshot.as_ref().and_then(|s| s.caret_rect.as_ref()).map(|c| CursorRect { x: c.x, top: c.y, bottom: c.y + c.h, baseline_y: c.y + c.h * 0.8 });
            let new_cursor_rect = new_snapshot.caret_rect.as_ref().map(|c| CursorRect { x: c.x, top: c.y, bottom: c.y + c.h, baseline_y: c.y + c.h * 0.8 });

            self.animation_coordinator.handle_composition_update(
                self.current_typing_animation_duration_ms as u64,
                &old_snapshot,
                &new_snapshot,
                data.composition_byte_start,
                data.composition_byte_end,
                old_cursor_rect,
                new_cursor_rect,
            );
        } else {
            self.update_preedit_visual_state();
        }

        self.update_ime_cursor_for_preedit();
        self.request_static_repaint();
    }

    fn input_set_suppress_next_ime_commit(&mut self, value: bool) {
        self.suppress_next_ime_commit = value;
    }

    fn input_take_suppress_next_ime_commit(&mut self) -> bool {
        let v = self.suppress_next_ime_commit;
        self.suppress_next_ime_commit = false;
        v
    }

    fn input_request_repaint(&mut self) {
        self.request_static_repaint();
    }
}
