use crate::sujian_editor_item::SujianEditorItem;
use cpp::cpp;
use std::ffi::c_void;

cpp! {{
    #include <QtGui/QInputMethodEvent>
    #include <QtGui/QKeyEvent>
    #include <QtQuick/QQuickItem>
    #include <QEvent>
    #include <QObject>
    #include <QRectF>
    #include <QString>

    extern "C" bool sujian_handle_key_and_text(void* rust_item, int key, int modifiers, const ushort* text, int text_len);
    extern "C" void sujian_ime_commit(void* rust_item, const ushort* text, int text_len);
    extern "C" void sujian_ime_preedit(void* rust_item, const ushort* text, int text_len, int cursor);
    extern "C" void sujian_ime_cancel(void* rust_item);
    extern "C" void sujian_request_repaint(void* rust_item);

    class SujianEventFilter : public QObject {
    public:
        void* rust_item;
        SujianEventFilter(QObject* parent, void* item)
            : QObject(parent), rust_item(item) {}

        bool eventFilter(QObject* obj, QEvent* event) override {
            if (!rust_item) return false;

            switch (event->type()) {
            case QEvent::KeyPress: {
                auto* ke = static_cast<QKeyEvent*>(event);
                QString text = ke->text();
                bool accepted = sujian_handle_key_and_text(
                    rust_item,
                    ke->key(),
                    static_cast<int>(ke->modifiers()),
                    reinterpret_cast<const ushort*>(text.utf16()),
                    static_cast<int>(text.size())
                );
                if (accepted) {
                    event->accept();
                    return true;
                }
                return false;
            }
            case QEvent::InputMethod: {
                auto* ime = static_cast<QInputMethodEvent*>(event);
                QString commit = ime->commitString();
                QString preedit = ime->preeditString();
                if (!commit.isEmpty()) {
                    sujian_ime_commit(
                        rust_item,
                        reinterpret_cast<const ushort*>(commit.utf16()),
                        static_cast<int>(commit.size())
                    );
                }
                if (!preedit.isEmpty()) {
                    int cursor = preedit.length();
                    if (ime->replacementStart() >= 0) {
                        cursor = ime->replacementStart() + ime->replacementLength();
                        if (cursor < 0) cursor = preedit.length();
                    }
                    sujian_ime_preedit(
                        rust_item,
                        reinterpret_cast<const ushort*>(preedit.utf16()),
                        static_cast<int>(preedit.size()),
                        cursor
                    );
                } else if (commit.isEmpty()) {
                    sujian_ime_cancel(rust_item);
                }
                sujian_request_repaint(rust_item);
                event->accept();
                return true;
            }
            case QEvent::InputMethodQuery: {
                auto* qe = static_cast<QInputMethodQueryEvent*>(event);
                if (qe->queries() & Qt::ImCursorRectangle) {
                    double cx = obj->property("cursor_rect_x").toDouble();
                    double cy = obj->property("cursor_rect_y").toDouble();
                    double cw = obj->property("cursor_rect_width").toDouble();
                    double ch = obj->property("cursor_rect_height").toDouble();
                    qe->setValue(Qt::ImCursorRectangle, QRectF(cx, cy, cw, ch));
                }
                if (qe->queries() & Qt::ImEnabled) {
                    qe->setValue(Qt::ImEnabled, true);
                }
                if (qe->queries() & Qt::ImHints) {
                    qe->setValue(Qt::ImHints, static_cast<int>(Qt::ImhNoPredictiveText));
                }
                if (qe->queries() & Qt::ImAnchorRectangle) {
                    double cx = obj->property("cursor_rect_x").toDouble();
                    double cy = obj->property("cursor_rect_y").toDouble();
                    double cw = obj->property("cursor_rect_width").toDouble();
                    double ch = obj->property("cursor_rect_height").toDouble();
                    qe->setValue(Qt::ImAnchorRectangle, QRectF(cx, cy, cw, ch));
                }
                event->accept();
                return true;
            }
            default:
                return false;
            }
        }
    };

    void sujian_install_event_filter(QQuickItem* item, void* rust_item) {
        if (!item) return;
        auto* filter = new SujianEventFilter(item, rust_item);
        item->installEventFilter(filter);
        item->setFlag(QQuickItem::ItemHasContents, true);
        item->setFlag(QQuickItem::ItemAcceptsInputMethod, true);
        item->setFlag(QQuickItem::ItemIsFocusScope, true);
        item->setAcceptedMouseButtons(Qt::AllButtons);
    }

    void sujian_focus_item(QQuickItem* item) {
        if (item) item->setFocus(true);
    }
}}

pub(crate) const KEY_BACKSPACE: i32 = 0x0100_0003;
pub(crate) const KEY_TAB: i32 = 0x0100_0001;
pub(crate) const KEY_ENTER: i32 = 0x0100_0005;
pub(crate) const KEY_INSERT: i32 = 0x0100_0006;
pub(crate) const KEY_RETURN: i32 = 0x0100_0004;
pub(crate) const KEY_DELETE: i32 = 0x0100_0007;
pub(crate) const KEY_LEFT: i32 = 0x0100_0012;
pub(crate) const KEY_UP: i32 = 0x0100_0013;
pub(crate) const KEY_RIGHT: i32 = 0x0100_0014;
pub(crate) const KEY_DOWN: i32 = 0x0100_0015;
pub(crate) const KEY_HOME: i32 = 0x0100_0010;
pub(crate) const KEY_END: i32 = 0x0100_0011;
pub(crate) const KEY_ESCAPE: i32 = 0x0100_0000;
pub(crate) const KEY_A: i32 = 0x41;
pub(crate) const KEY_C: i32 = 0x43;
pub(crate) const KEY_V: i32 = 0x56;
pub(crate) const KEY_X: i32 = 0x58;
pub(crate) const KEY_Y: i32 = 0x59;
pub(crate) const KEY_Z: i32 = 0x5a;
pub(crate) const CTRL_MODIFIER: i32 = 0x0400_0000;
pub(crate) const SHIFT_MODIFIER: i32 = 0x0200_0000;
pub(crate) const ALT_MODIFIER: i32 = 0x0800_0000;
pub(crate) const META_MODIFIER: i32 = 0x1000_0000;

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
    fn input_move_cursor_horizontal(&mut self, forward: bool, extend: bool);
    fn input_move_cursor_vertical(&mut self, down: bool, extend: bool);
    fn input_move_to_line_edge(&mut self, end: bool, extend: bool);
    fn input_clear_preedit(&mut self);
    fn input_set_preedit(&mut self, text: String, cursor: usize);
    fn input_set_suppress_next_ime_commit(&mut self, value: bool);
    fn input_take_suppress_next_ime_commit(&mut self) -> bool;
    fn input_request_repaint(&mut self);
}

pub(crate) fn install_event_filter(item: *mut c_void, rust_item: *mut c_void) {
    cpp!(unsafe [item as "QQuickItem*", rust_item as "void*"] {
        sujian_install_event_filter(item, rust_item);
    });
}

pub(crate) fn focus_item(item: *mut c_void) {
    cpp!(unsafe [item as "QQuickItem*"] {
        sujian_focus_item(item);
    });
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
    host.input_clear_preedit();
    if !text.is_empty() {
        host.input_insert_text(text);
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
    host.input_clear_preedit();
    host.input_insert_text(text);
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

pub(crate) fn ime_cancel<H: EditorInputHost + ?Sized>(host: &mut H) {
    host.input_clear_preedit();
}

fn has_ctrl(modifiers: i32) -> bool {
    modifiers & CTRL_MODIFIER != 0
}

fn has_shift(modifiers: i32) -> bool {
    modifiers & SHIFT_MODIFIER != 0
}

fn has_alt(modifiers: i32) -> bool {
    modifiers & ALT_MODIFIER != 0
}

fn has_meta(modifiers: i32) -> bool {
    modifiers & META_MODIFIER != 0
}

fn is_copy_shortcut(key: i32, modifiers: i32) -> bool {
    has_ctrl(modifiers) && (key == KEY_C || key == KEY_INSERT)
}

fn is_paste_shortcut(key: i32, modifiers: i32) -> bool {
    (has_ctrl(modifiers) && key == KEY_V) || (has_shift(modifiers) && key == KEY_INSERT)
}

fn is_redo_shortcut(key: i32, modifiers: i32) -> bool {
    has_ctrl(modifiers) && (key == KEY_Y || (has_shift(modifiers) && key == KEY_Z))
}

fn is_destructive_key(key: i32, modifiers: i32) -> bool {
    key == KEY_BACKSPACE || key == KEY_DELETE || (has_ctrl(modifiers) && key == KEY_X)
}

fn decode_utf16_lossy(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

fn decode_utf16_ptr(text: *const u16, text_len: i32) -> String {
    if text.is_null() || text_len <= 0 {
        return String::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(text, text_len as usize) };
    decode_utf16_lossy(slice)
}

unsafe fn item_from_ptr<'a>(rust_item: *mut c_void) -> Option<&'a mut SujianEditorItem> {
    if rust_item.is_null() {
        return None;
    }
    Some(unsafe { &mut *(rust_item as *mut SujianEditorItem) })
}

#[no_mangle]
extern "C" fn sujian_handle_key_and_text(
    rust_item: *mut c_void,
    key: i32,
    modifiers: i32,
    text: *const u16,
    text_len: i32,
) -> bool {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return false;
    };
    let text = decode_utf16_ptr(text, text_len);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_key_and_text(item, key, modifiers, text)
    })) {
        Ok(result) => result,
        Err(_) => {
            eprintln!(
                "[sujian_editor] panic in sujian_handle_key_and_text, caught at FFI boundary"
            );
            false
        }
    }
}

#[no_mangle]
extern "C" fn sujian_ime_commit(rust_item: *mut c_void, text: *const u16, text_len: i32) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_commit(item, text);
    }));
}

#[no_mangle]
extern "C" fn sujian_ime_preedit(
    rust_item: *mut c_void,
    text: *const u16,
    text_len: i32,
    cursor: i32,
) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_preedit(item, text, cursor);
    }));
}

#[no_mangle]
extern "C" fn sujian_ime_cancel(rust_item: *mut c_void) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_cancel(item);
    }));
}

#[no_mangle]
extern "C" fn sujian_request_repaint(rust_item: *mut c_void) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        item.input_request_repaint();
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeHost {
        enabled: bool,
        inserted: Vec<String>,
        operations: Vec<&'static str>,
        preedit_text: String,
        preedit_cursor: usize,
        suppress_next_ime_commit: bool,
        explicit_clear_count: usize,
        repaint_count: usize,
    }

    impl FakeHost {
        fn enabled() -> Self {
            Self {
                enabled: true,
                ..Self::default()
            }
        }
    }

    impl EditorInputHost for FakeHost {
        fn input_enabled(&self) -> bool {
            self.enabled
        }

        fn input_emit_explicit_clear_requested(&mut self) {
            self.explicit_clear_count += 1;
        }

        fn input_clipboard_copy(&mut self) -> bool {
            self.operations.push("copy");
            true
        }

        fn input_clipboard_paste(&mut self) {
            self.operations.push("paste");
        }

        fn input_undo(&mut self) {
            self.operations.push("undo");
        }

        fn input_redo(&mut self) {
            self.operations.push("redo");
        }

        fn input_select_all(&mut self) {
            self.operations.push("select_all");
        }

        fn input_delete_selection(&mut self) {
            self.operations.push("delete_selection");
        }

        fn input_delete_backward(&mut self) {
            self.operations.push("delete_backward");
        }

        fn input_delete_forward(&mut self) {
            self.operations.push("delete_forward");
        }

        fn input_insert_text(&mut self, text: String) {
            self.inserted.push(text);
        }

        fn input_move_cursor_horizontal(&mut self, forward: bool, extend: bool) {
            self.operations.push(if forward { "right" } else { "left" });
            if extend {
                self.operations.push("extend");
            }
        }

        fn input_move_cursor_vertical(&mut self, down: bool, extend: bool) {
            self.operations.push(if down { "down" } else { "up" });
            if extend {
                self.operations.push("extend");
            }
        }

        fn input_move_to_line_edge(&mut self, end: bool, extend: bool) {
            self.operations.push(if end { "end" } else { "home" });
            if extend {
                self.operations.push("extend");
            }
        }

        fn input_clear_preedit(&mut self) {
            self.preedit_text.clear();
            self.preedit_cursor = 0;
        }

        fn input_set_preedit(&mut self, text: String, cursor: usize) {
            self.preedit_text = text;
            self.preedit_cursor = cursor;
        }

        fn input_set_suppress_next_ime_commit(&mut self, value: bool) {
            self.suppress_next_ime_commit = value;
        }

        fn input_take_suppress_next_ime_commit(&mut self) -> bool {
            let value = self.suppress_next_ime_commit;
            if value {
                self.suppress_next_ime_commit = false;
            }
            value
        }

        fn input_request_repaint(&mut self) {
            self.repaint_count += 1;
        }
    }

    #[test]
    fn desktop_shortcuts_match_existing_keys() {
        assert!(is_copy_shortcut(KEY_C, CTRL_MODIFIER));
        assert!(is_copy_shortcut(KEY_INSERT, CTRL_MODIFIER));
        assert!(is_paste_shortcut(KEY_V, CTRL_MODIFIER));
        assert!(is_paste_shortcut(KEY_INSERT, SHIFT_MODIFIER));
        assert!(is_redo_shortcut(KEY_Y, CTRL_MODIFIER));
        assert!(is_redo_shortcut(KEY_Z, CTRL_MODIFIER | SHIFT_MODIFIER));
        assert!(!is_redo_shortcut(KEY_Z, CTRL_MODIFIER));
    }

    #[test]
    fn utf16_decode_covers_chinese_ime_text() {
        let units: Vec<u16> = "中文输入".encode_utf16().collect();
        assert_eq!(decode_utf16_lossy(&units), "中文输入");
    }

    #[test]
    fn preedit_cursor_is_clamped_to_existing_byte_len() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "中文".to_string(), 99);
        assert_eq!(host.preedit_text, "中文");
        assert_eq!(host.preedit_cursor, "中文".len());
    }

    #[test]
    fn destructive_keys_emit_explicit_clear_before_handling() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            KEY_BACKSPACE,
            0,
            String::new()
        ));
        assert_eq!(host.explicit_clear_count, 1);
        assert_eq!(host.operations, vec!["delete_backward"]);

        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            KEY_X,
            CTRL_MODIFIER,
            String::new()
        ));
        assert_eq!(host.explicit_clear_count, 1);
        assert_eq!(host.operations, vec!["copy", "delete_selection"]);
    }

    #[test]
    fn printable_text_inserts_when_key_is_not_handled() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "你".to_string()));
        assert_eq!(host.inserted, vec!["你"]);
    }

    #[test]
    fn suppressed_ime_commit_only_clears_preedit_once() {
        let mut host = FakeHost::enabled();
        host.preedit_text = "拼".to_string();
        host.preedit_cursor = 3;
        host.suppress_next_ime_commit = true;

        ime_commit(&mut host, "拼".to_string());

        assert!(host.inserted.is_empty());
        assert_eq!(host.preedit_text, "");
        assert_eq!(host.preedit_cursor, 0);
        assert!(!host.suppress_next_ime_commit);
    }
}
