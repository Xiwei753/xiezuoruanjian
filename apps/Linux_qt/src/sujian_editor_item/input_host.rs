use super::*;

pub(crate) fn is_left_button_pressed(event: &QMouseEvent) -> bool {
    cpp!(unsafe [event as "const QMouseEvent*"] -> bool as "bool" {
        return event ? (event->buttons() & Qt::LeftButton) : false;
    })
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
        if !self.preedit_text.is_empty() {
            self.pending_preedit_cursor_rect = self.preedit_cursor_rect.clone();
        }
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
        self.last_preedit_visual_transaction_json = "".into();
        self.update_ime_cursor_for_preedit();
    }

    fn input_set_preedit(&mut self, text: String, cursor: usize) {
        self.preedit_old_text = self.preedit_text.clone();
        self.preedit_text = text;
        self.preedit_cursor = cursor;
        self.preedit_attributes.clear();
        self.update_preedit_visual_state();
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
        self.preedit_text = text;
        self.preedit_cursor = cursor;
        self.preedit_attributes = attributes;
        self.update_preedit_visual_state();
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
