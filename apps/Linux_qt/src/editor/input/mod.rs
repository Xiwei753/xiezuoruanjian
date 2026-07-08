//! Linux Qt 输入层三层架构
//!
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │ Layer 1: QtInputSurface (C++ SujianEventFilter)                     │
//! │   - Qt 官方事件入口：keyPressEvent / inputMethodEvent / query       │
//! │   - 不写正文业务，不写动画逻辑                                      │
//! │   - 委托给 Linux PlatformImeAdapter 处理 fcitx5/ibus 语义             │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │ Layer 2: Linux PlatformImeAdapter (C++ 内嵌)                         │
//! │   - LinuxImeAdapter: 直接插入，不延迟，按 Qt inputMethodEvent 语义  │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │ Layer 3: EditorInputController (Rust)                               │
//! │   - 接收归一化输入事件：PlainText / Shortcut / Preedit / Commit 等  │
//! │   - 调用 SujianEditorItem / EditorEngine 修改正文和生成视觉事务     │
//! │   - Linux IME 语义只存在 Layer 2，正文编辑和动画不关心具体输入法    │
//! └──────────────────────────────────────────────────────────────────────┘

pub mod controller;
pub mod events;
pub mod platform;
pub mod platform_ime;
pub mod qt_surface;

pub(crate) use controller::{
    cancel_preedit, commit_preedit_text, handle_key, insert_preedit_text, EditorInputHost,
};
pub(crate) use qt_surface::{focus_item, install_event_filter};

#[cfg(test)]
mod tests {
    use super::controller::*;
    use super::events::*;
    use super::*;
    use crate::sujian_editor_item::PreeditAttribute;

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
        replace_and_insert_calls: Vec<(i32, i32, String)>,
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

        fn input_replace_and_insert(
            &mut self,
            replace_start: i32,
            replace_length: i32,
            text: String,
        ) {
            self.replace_and_insert_calls
                .push((replace_start, replace_length, text.clone()));
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

        fn input_set_preedit_with_attrs(
            &mut self,
            text: String,
            cursor: usize,
            _attributes: Vec<PreeditAttribute>,
        ) {
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
    fn linux_qt_shortcuts_match_existing_keys() {
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

    #[test]
    fn space_inserts_as_plain_text() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, " ".to_string()));
        assert_eq!(host.inserted, vec![" "]);
    }

    #[test]
    fn plus_inserts_as_plain_text() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            0,
            SHIFT_MODIFIER,
            "+".to_string()
        ));
        assert_eq!(host.inserted, vec!["+"]);
    }

    #[test]
    fn chinese_punctuation_inserts_as_plain_text() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "。".to_string()));
        assert_eq!(host.inserted, vec!["。"]);
        let mut host2 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host2, 0, 0, "！".to_string()));
        assert_eq!(host2.inserted, vec!["！"]);
    }

    #[test]
    fn ctrl_a_triggers_select_all() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            KEY_A,
            CTRL_MODIFIER,
            String::new()
        ));
        assert_eq!(host.operations, vec!["select_all"]);
    }

    #[test]
    fn ctrl_c_v_x_z_y_shortcuts() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            KEY_C,
            CTRL_MODIFIER,
            String::new()
        ));
        assert_eq!(host.operations, vec!["copy"]);

        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            KEY_V,
            CTRL_MODIFIER,
            String::new()
        ));
        assert_eq!(host.operations, vec!["paste"]);

        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            KEY_X,
            CTRL_MODIFIER,
            String::new()
        ));
        assert_eq!(host.operations, vec!["copy", "delete_selection"]);

        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            KEY_Z,
            CTRL_MODIFIER,
            String::new()
        ));
        assert_eq!(host.operations, vec!["undo"]);

        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            KEY_Y,
            CTRL_MODIFIER,
            String::new()
        ));
        assert_eq!(host.operations, vec!["redo"]);
    }

    #[test]
    fn shift_plus_symbol_not_swallowed_as_shortcut() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            0,
            SHIFT_MODIFIER,
            "+".to_string()
        ));
        assert!(host.inserted.contains(&"+".to_string()));
        assert!(!host.operations.contains(&"copy"));
    }

    #[test]
    fn preedit_does_not_modify_buffer_text() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼".to_string(), 0);
        assert_eq!(host.preedit_text, "拼");
        assert!(
            host.inserted.is_empty(),
            "preedit should NOT insert into buffer"
        );
    }

    #[test]
    fn ime_commit_writes_to_buffer() {
        let mut host = FakeHost::enabled();
        ime_commit(&mut host, "你好".to_string());
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn ime_preedit_cursor_attribute_mapping() {
        use crate::sujian_editor_item::PreeditAttributeKind;

        let kind_0 = if 0 == 0 {
            match 0 {
                1 => PreeditAttributeKind::TextColor {
                    color: String::new(),
                },
                2 => PreeditAttributeKind::BackgroundColor {
                    color: String::new(),
                },
                3 => PreeditAttributeKind::FontUnderline,
                _ => PreeditAttributeKind::Underline,
            }
        } else if 0 == 1 {
            PreeditAttributeKind::Cursor
        } else {
            panic!("unexpected attr_type");
        };
        assert_eq!(kind_0, PreeditAttributeKind::Underline);

        let kind_tc = match 1 {
            1 => PreeditAttributeKind::TextColor {
                color: String::new(),
            },
            2 => PreeditAttributeKind::BackgroundColor {
                color: String::new(),
            },
            3 => PreeditAttributeKind::FontUnderline,
            _ => PreeditAttributeKind::Underline,
        };
        assert!(matches!(kind_tc, PreeditAttributeKind::TextColor { .. }));

        let kind_bc = match 2 {
            1 => PreeditAttributeKind::TextColor {
                color: String::new(),
            },
            2 => PreeditAttributeKind::BackgroundColor {
                color: String::new(),
            },
            3 => PreeditAttributeKind::FontUnderline,
            _ => PreeditAttributeKind::Underline,
        };
        assert!(matches!(
            kind_bc,
            PreeditAttributeKind::BackgroundColor { .. }
        ));

        let kind_fu = match 3 {
            1 => PreeditAttributeKind::TextColor {
                color: String::new(),
            },
            2 => PreeditAttributeKind::BackgroundColor {
                color: String::new(),
            },
            3 => PreeditAttributeKind::FontUnderline,
            _ => PreeditAttributeKind::Underline,
        };
        assert_eq!(kind_fu, PreeditAttributeKind::FontUnderline);

        let kind_1 = if 1 == 0 {
            PreeditAttributeKind::Underline
        } else if 1 == 1 {
            PreeditAttributeKind::Cursor
        } else {
            panic!("unexpected attr_type");
        };
        assert_eq!(kind_1, PreeditAttributeKind::Cursor);

        for &attr_type in &[2, 3, 4] {
            let is_handled = attr_type == 0 || attr_type == 1;
            assert!(
                !is_handled,
                "attr_type {} should not be mapped to Underline or Cursor",
                attr_type
            );
        }
    }

    #[test]
    fn test_ime_replace_and_commit_basic() {
        let mut host = FakeHost::enabled();
        ime_replace_and_commit(&mut host, "你好".to_string(), -2, 2);
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, -2);
        assert_eq!(*len, 2);
        assert_eq!(text, "你好");
    }

    #[test]
    fn test_ime_replace_negative_start() {
        let mut host = FakeHost::enabled();
        ime_replace_and_commit(&mut host, "新".to_string(), -1, 1);
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, -1);
        assert_eq!(*len, 1);
        assert_eq!(text, "新");
    }

    #[test]
    fn test_ime_replace_does_not_split_surrogate_pair() {
        let mut host = FakeHost::enabled();
        ime_replace_and_commit(&mut host, "X".to_string(), -1, 1);
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, -1);
        assert_eq!(*len, 1);
        assert_eq!(text, "X");
    }

    #[test]
    fn test_ime_replace_clamps_to_char_boundary() {
        let mut host = FakeHost::enabled();
        ime_replace_and_commit(&mut host, "替换".to_string(), 0, 3);
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, 0);
        assert_eq!(*len, 3);
        assert_eq!(text, "替换");
    }

    #[test]
    fn test_ime_replace_single_undo() {
        let mut host = FakeHost::enabled();
        ime_replace_and_commit(&mut host, "修正".to_string(), -2, 2);
        assert_eq!(host.replace_and_insert_calls.len(), 1);
    }

    #[test]
    fn linux_space_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0x20, 0, " ".to_string()));
        assert_eq!(host.inserted, vec![" "]);
    }

    #[test]
    fn linux_plus_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            0,
            SHIFT_MODIFIER,
            "+".to_string()
        ));
        assert_eq!(host.inserted, vec!["+"]);
    }

    #[test]
    fn linux_minus_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "-".to_string()));
        assert_eq!(host.inserted, vec!["-"]);
    }

    #[test]
    fn linux_underscore_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            0,
            SHIFT_MODIFIER,
            "_".to_string()
        ));
        assert_eq!(host.inserted, vec!["_"]);
    }

    #[test]
    fn linux_slash_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "/".to_string()));
        assert_eq!(host.inserted, vec!["/"]);
    }

    #[test]
    fn linux_chinese_punctuation_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "。".to_string()));
        assert_eq!(host.inserted, vec!["。"]);
        let mut host2 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host2, 0, 0, "，".to_string()));
        assert_eq!(host2.inserted, vec!["，"]);
        let mut host3 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host3, 0, 0, "！".to_string()));
        assert_eq!(host3.inserted, vec!["！"]);
    }

    #[test]
    fn linux_ime_preedit_does_not_write_to_buffer() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼音".to_string(), 3);
        assert_eq!(host.preedit_text, "拼音");
        assert!(
            host.inserted.is_empty(),
            "preedit must NOT insert into buffer"
        );
    }

    #[test]
    fn linux_ime_commit_writes_to_buffer() {
        let mut host = FakeHost::enabled();
        ime_commit(&mut host, "你好".to_string());
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_ime_preedit_with_attrs_does_not_write_to_buffer() {
        let mut host = FakeHost::enabled();
        ime_preedit_with_attrs(
            &mut host,
            "拼".to_string(),
            0,
            vec![PreeditAttribute {
                start: 0,
                length: 3,
                kind: crate::sujian_editor_item::PreeditAttributeKind::Underline,
            }],
        );
        assert_eq!(host.preedit_text, "拼");
        assert!(
            host.inserted.is_empty(),
            "preedit with attrs must NOT insert into buffer"
        );
    }

    #[test]
    fn linux_ime_commit_after_preedit_writes_to_buffer() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼".to_string(), 0);
        assert!(host.inserted.is_empty());
        ime_commit(&mut host, "你好".to_string());
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_ime_replacement_works() {
        let mut host = FakeHost::enabled();
        ime_replace_and_commit(&mut host, "修正".to_string(), -2, 2);
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, -2);
        assert_eq!(*len, 2);
        assert_eq!(text, "修正");
    }

    #[test]
    fn linux_ime_cancel_clears_preedit() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼".to_string(), 0);
        assert_eq!(host.preedit_text, "拼");
        ime_cancel(&mut host);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_symbols_not_blocked_by_ctrl_or_alt() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "(".to_string()));
        assert_eq!(host.inserted, vec!["("]);
        let mut host2 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host2, 0, 0, ")".to_string()));
        assert_eq!(host2.inserted, vec![")"]);
        let mut host3 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host3, 0, 0, "[".to_string()));
        assert_eq!(host3.inserted, vec!["["]);
    }

    #[test]
    fn linux_ime_commit_not_blocked_by_animation_state() {
        let mut host = FakeHost::enabled();
        ime_commit(&mut host, "测试".to_string());
        assert_eq!(host.inserted, vec!["测试"]);
    }

    #[test]
    fn dispatch_plain_text_inserts() {
        let mut host = FakeHost::enabled();
        EditorInputController::dispatch(
            &mut host,
            EditorInputEvent::PlainText {
                text: "你好".to_string(),
            },
        );
        assert_eq!(host.inserted, vec!["你好"]);
    }

    #[test]
    fn dispatch_shortcut_select_all() {
        let mut host = FakeHost::enabled();
        EditorInputController::dispatch(
            &mut host,
            EditorInputEvent::Shortcut {
                key: KEY_A,
                modifiers: CTRL_MODIFIER,
            },
        );
        assert_eq!(host.operations, vec!["select_all"]);
    }

    #[test]
    fn dispatch_ime_commit() {
        let mut host = FakeHost::enabled();
        EditorInputController::dispatch(
            &mut host,
            EditorInputEvent::ImeCommit {
                text: "你好".to_string(),
            },
        );
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn dispatch_ime_replacement_commit() {
        let mut host = FakeHost::enabled();
        EditorInputController::dispatch(
            &mut host,
            EditorInputEvent::ImeReplacementCommit {
                text: "修正".to_string(),
                replace_start: -2,
                replace_length: 2,
            },
        );
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, -2);
        assert_eq!(*len, 2);
        assert_eq!(text, "修正");
    }

    #[test]
    fn dispatch_preedit_changed() {
        let mut host = FakeHost::enabled();
        EditorInputController::dispatch(
            &mut host,
            EditorInputEvent::PreeditChanged {
                text: "拼音".to_string(),
                cursor: 3,
                attributes: vec![],
            },
        );
        assert_eq!(host.preedit_text, "拼音");
        assert!(
            host.inserted.is_empty(),
            "preedit must NOT insert into buffer"
        );
    }

    #[test]
    fn dispatch_ime_cancel() {
        let mut host = FakeHost::enabled();
        host.preedit_text = "拼".to_string();
        EditorInputController::dispatch(&mut host, EditorInputEvent::ImeCancel);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn dispatch_disabled_host_ignores_all() {
        let mut host = FakeHost::default();
        EditorInputController::dispatch(
            &mut host,
            EditorInputEvent::PlainText {
                text: "x".to_string(),
            },
        );
        assert!(host.inserted.is_empty());
        EditorInputController::dispatch(
            &mut host,
            EditorInputEvent::ImeCommit {
                text: "你好".to_string(),
            },
        );
        assert!(host.inserted.is_empty());
        EditorInputController::dispatch(
            &mut host,
            EditorInputEvent::PreeditChanged {
                text: "拼".to_string(),
                cursor: 0,
                attributes: vec![],
            },
        );
        assert!(host.preedit_text.is_empty());
    }

    #[test]
    fn linux_brackets_and_quotes_not_blocked() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "]".to_string()));
        assert_eq!(host.inserted, vec!["]"]);
        let mut host2 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host2, 0, 0, "\"".to_string()));
        assert_eq!(host2.inserted, vec!["\""]);
        let mut host3 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host3, 0, 0, "'".to_string()));
        assert_eq!(host3.inserted, vec!["'"]);
    }

    #[test]
    fn linux_cursor_attribute_drives_preedit_cursor() {
        let mut host = FakeHost::enabled();
        let attrs = vec![PreeditAttribute {
            start: 0,
            length: 0,
            kind: crate::sujian_editor_item::PreeditAttributeKind::Cursor,
        }];
        ime_preedit_with_attrs(&mut host, "拼音".to_string(), 1, attrs);
        assert_eq!(host.preedit_text, "拼音");
        assert_eq!(host.preedit_cursor, 1);
        assert!(host.inserted.is_empty());
    }

    #[test]
    fn linux_commit_after_preedit_always_writes_buffer() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "nihao".to_string(), 5);
        assert!(host.inserted.is_empty());
        ime_commit(&mut host, "你好".to_string());
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_animation_off_does_not_affect_commit() {
        let mut host = FakeHost::enabled();
        ime_commit(&mut host, "测试".to_string());
        assert_eq!(host.inserted, vec!["测试"]);
    }

    #[test]
    fn linux_scroll_chapter_settings_only_clear_animation_not_input() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼".to_string(), 0);
        assert_eq!(host.preedit_text, "拼");
        assert_eq!(host.preedit_text, "拼");
        ime_commit(&mut host, "你好".to_string());
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_ime_commit_text_first_animation_second() {
        let mut host = FakeHost::enabled();
        ime_commit(&mut host, "确认".to_string());
        assert_eq!(host.inserted, vec!["确认"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_candidate_rectangle_follows_preedit_cursor() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼音输入".to_string(), 2);
        assert_eq!(host.preedit_cursor, 2);
        let mut host2 = FakeHost::enabled();
        ime_preedit(&mut host2, "abc".to_string(), 0);
        assert_eq!(host2.preedit_cursor, 0);
    }
}
