//! Layer 3: EditorInputController — 归一化输入事件分发
//!
//! 接收归一化 EditorInputEvent，调用 EditorInputHost 修改正文和生成视觉事务。
//! 不包含任何平台分支逻辑。动画状态不影响文本正确性。

use super::events::*;
use crate::sujian_editor_item::PreeditAttribute;

#[allow(dead_code)]
pub(crate) struct EditorInputController;

impl EditorInputController {
    #[allow(dead_code)]
    pub(crate) fn dispatch<H: EditorInputHost + ?Sized>(host: &mut H, event: EditorInputEvent) {
        match event {
            EditorInputEvent::PlainText { text } => {
                if !host.input_enabled() || text.is_empty() {
                    return;
                }
                host.input_insert_text(text);
            }
            EditorInputEvent::Shortcut { key, modifiers } => {
                handle_key(host, key, modifiers);
            }
            EditorInputEvent::PreeditChanged { text, cursor, attributes } => {
                if !host.input_enabled() {
                    return;
                }
                if !text.is_empty() {
                    host.input_set_suppress_next_ime_commit(false);
                }
                host.input_set_preedit_with_attrs(text, cursor, attributes);
            }
            EditorInputEvent::ImeCommit { text } => {
                ime_commit(host, text);
            }
            EditorInputEvent::ImeReplacementCommit { text, replace_start, replace_length } => {
                ime_replace_and_commit(host, text, replace_start, replace_length);
            }
            EditorInputEvent::ImeCancel => {
                ime_cancel(host);
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) trait EditorInputHost {
    fn input_enabled(&self) -> bool;
    fn input_emit_explicit_clear_requested(&mut self);
    fn input_clipboard_copy(&mut self) -> bool;
    fn input_clipboard_paste(&mut self);
    fn input_undo(&mut self);
    fn input_redo(&mut self);
    fn input_select_all(&mut self);
    fn input_delete_selection(&mut self);
    fn input_delete_backward(&mut self);
    fn input_delete_forward(&mut self);
    fn input_insert_text(&mut self, text: String);
    fn input_replace_and_insert(&mut self, replace_start: i32, replace_length: i32, text: String);
    fn input_move_cursor_horizontal(&mut self, forward: bool, extend: bool);
    fn input_move_cursor_vertical(&mut self, down: bool, extend: bool);
    fn input_move_to_line_edge(&mut self, end: bool, extend: bool);
    fn input_clear_preedit(&mut self);
    fn input_set_preedit(&mut self, text: String, cursor: usize);
    fn input_set_preedit_with_attrs(&mut self, text: String, cursor: usize, attributes: Vec<PreeditAttribute>);
    fn input_set_suppress_next_ime_commit(&mut self, value: bool);
    fn input_take_suppress_next_ime_commit(&mut self) -> bool;
    fn input_request_repaint(&mut self);

    fn input_preedit_text(&self) -> String { String::new() }
    fn input_preedit_cursor(&self) -> usize { 0 }
    fn input_preedit_attributes(&self) -> Vec<PreeditAttribute> { Vec::new() }
    fn input_preedit_transaction_created(&self, _old_text: &str, _new_text: &str) {}
}

pub(crate) fn handle_key<H: EditorInputHost + ?Sized>(
    host: &mut H,
    key: i32,
    modifiers: i32,
) -> bool {
    if !host.input_enabled() {
        return false;
    }
    let ctrl = has_ctrl(modifiers);
    let shift = has_shift(modifiers);
    if is_copy_shortcut(key, modifiers) {
        host.input_clipboard_copy();
        return true;
    }
    if is_paste_shortcut(key, modifiers) {
        host.input_clipboard_paste();
        return true;
    }
    if is_redo_shortcut(key, modifiers) {
        host.input_redo();
        return true;
    }
    if ctrl {
        match key {
            KEY_A => {
                host.input_select_all();
                return true;
            }
            KEY_X => {
                host.input_clipboard_copy();
                host.input_delete_selection();
                return true;
            }
            KEY_Z => {
                host.input_undo();
                return true;
            }
            _ => return false,
        }
    }

    match key {
        KEY_ESCAPE => {
            host.input_clear_preedit();
            host.input_set_suppress_next_ime_commit(true);
        }
        KEY_BACKSPACE => host.input_delete_backward(),
        KEY_DELETE => host.input_delete_forward(),
        KEY_RETURN | KEY_ENTER => host.input_insert_text("\n".to_string()),
        KEY_TAB => host.input_insert_text("\t".to_string()),
        KEY_LEFT => host.input_move_cursor_horizontal(false, shift),
        KEY_RIGHT => host.input_move_cursor_horizontal(true, shift),
        KEY_UP => host.input_move_cursor_vertical(false, shift),
        KEY_DOWN => host.input_move_cursor_vertical(true, shift),
        KEY_HOME => host.input_move_to_line_edge(false, shift),
        KEY_END => host.input_move_to_line_edge(true, shift),
        _ => return false,
    }
    true
}

pub(crate) fn handle_key_and_text<H: EditorInputHost + ?Sized>(
    host: &mut H,
    key: i32,
    modifiers: i32,
    text: String,
) -> bool {
    if !host.input_enabled() {
        return false;
    }

    let ctrl = has_ctrl(modifiers);
    if is_destructive_key(key, modifiers) {
        host.input_emit_explicit_clear_requested();
    }

    if handle_key(host, key, modifiers) {
        return true;
    }

    if !ctrl && !has_alt(modifiers) && !has_meta(modifiers) && !text.is_empty() {
        host.input_insert_text(text);
        return true;
    }

    false
}

pub(crate) fn insert_preedit_text<H: EditorInputHost + ?Sized>(host: &mut H, text: String) {
    if !host.input_enabled() {
        return;
    }
    let cursor = text.len();
    host.input_set_preedit(text, cursor);
    host.input_request_repaint();
}

pub(crate) fn commit_preedit_text<H: EditorInputHost + ?Sized>(host: &mut H, text: String) {
    if !host.input_enabled() {
        return;
    }
    if !text.is_empty() {
        host.input_insert_text(text);
    } else {
        host.input_clear_preedit();
    }
}

pub(crate) fn cancel_preedit<H: EditorInputHost + ?Sized>(host: &mut H) {
    host.input_clear_preedit();
    host.input_request_repaint();
}

pub(crate) fn ime_commit<H: EditorInputHost + ?Sized>(host: &mut H, text: String) {
    if !host.input_enabled() || text.is_empty() {
        return;
    }
    if host.input_take_suppress_next_ime_commit() {
        host.input_clear_preedit();
        return;
    }
    host.input_insert_text(text);
}

pub(crate) fn ime_replace_and_commit<H: EditorInputHost + ?Sized>(
    host: &mut H,
    text: String,
    replace_start: i32,
    replace_length: i32,
) {
    if !host.input_enabled() || text.is_empty() {
        return;
    }
    if host.input_take_suppress_next_ime_commit() {
        host.input_clear_preedit();
        return;
    }
    host.input_replace_and_insert(replace_start, replace_length, text);
}

pub(crate) fn ime_preedit<H: EditorInputHost + ?Sized>(host: &mut H, text: String, cursor: i32) {
    if !host.input_enabled() {
        return;
    }
    if !text.is_empty() {
        host.input_set_suppress_next_ime_commit(false);
    }
    let cursor = (cursor.max(0) as usize).min(text.len());
    host.input_set_preedit(text, cursor);
}

pub(crate) fn ime_preedit_with_attrs<H: EditorInputHost + ?Sized>(
    host: &mut H,
    text: String,
    cursor: i32,
    attributes: Vec<PreeditAttribute>,
) {
    if !host.input_enabled() {
        return;
    }
    if !text.is_empty() {
        host.input_set_suppress_next_ime_commit(false);
    }
    let cursor = (cursor.max(0) as usize).min(text.len());
    host.input_set_preedit_with_attrs(text, cursor, attributes);
}

pub(crate) fn ime_cancel<H: EditorInputHost + ?Sized>(host: &mut H) {
    host.input_clear_preedit();
}
