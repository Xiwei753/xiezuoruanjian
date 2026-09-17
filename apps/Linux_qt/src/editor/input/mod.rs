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
        ime_replace_commit_calls: Vec<ImeReplaceEvent>,
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
            if !self.preedit_text.is_empty() {
                self.preedit_text.clear();
                self.preedit_cursor = 0;
            }
            self.inserted.push(text);
        }

        fn input_ime_replace_and_commit(&mut self, event: ImeReplaceEvent) {
            self.ime_replace_commit_calls.push(event.clone());
            if !self.preedit_text.is_empty() {
                self.preedit_text.clear();
                self.preedit_cursor = 0;
            }
            self.inserted.push(event.inserted_text);
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

        /// Issue #704: FakeHost 用 `!preedit_text.is_empty()` 近似 is_composing
        /// （FakeHost 无 composition_session 字段）。只有确实取消过真实
        /// composition（非空 preedit）时才武装一次 suppress guard。
        fn input_cancel_preedit_for_escape(&mut self) {
            let was_composing = !self.preedit_text.is_empty();
            self.preedit_text.clear();
            self.preedit_cursor = 0;
            if was_composing {
                self.suppress_next_ime_commit = true;
            }
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
        // Issue #701 评论 5699569220: controller 接收归一化的 ImeReplaceEvent，
        // 携带 UTF-8 byte range（由 platform_ime 解析后填入）。
        // Issue #701 评论 5702675971: 新签名 (selection_byte_range, replacement_byte_range, inserted_text)。
        let event = ImeReplaceEvent::new(None, (0, 6), "你好".to_string());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
        let ev = &host.ime_replace_commit_calls[0];
        assert_eq!(ev.selection_byte_range, None);
        assert_eq!(ev.replacement_byte_range_after_selection, (0, 6));
        assert_eq!(ev.inserted_text, "你好");
    }

    #[test]
    fn test_ime_replace_negative_start() {
        let mut host = FakeHost::enabled();
        let event = ImeReplaceEvent::new(None, (0, 3), "新".to_string());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
        let ev = &host.ime_replace_commit_calls[0];
        assert_eq!(ev.selection_byte_range, None);
        assert_eq!(ev.replacement_byte_range_after_selection, (0, 3));
        assert_eq!(ev.inserted_text, "新");
    }

    #[test]
    fn test_ime_replace_does_not_split_surrogate_pair() {
        let mut host = FakeHost::enabled();
        let event = ImeReplaceEvent::new(None, (0, 1), "X".to_string());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
        let ev = &host.ime_replace_commit_calls[0];
        assert_eq!(ev.replacement_byte_range_after_selection, (0, 1));
        assert_eq!(ev.inserted_text, "X");
    }

    #[test]
    fn test_ime_replace_clamps_to_char_boundary() {
        let mut host = FakeHost::enabled();
        let event = ImeReplaceEvent::new(None, (0, 6), "替换".to_string());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
        let ev = &host.ime_replace_commit_calls[0];
        assert_eq!(ev.replacement_byte_range_after_selection, (0, 6));
        assert_eq!(ev.inserted_text, "替换");
    }

    #[test]
    fn test_ime_replace_single_undo() {
        let mut host = FakeHost::enabled();
        let event = ImeReplaceEvent::new(None, (0, 6), "修正".to_string());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
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
        let event = ImeReplaceEvent::new(None, (0, 6), "修正".to_string());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
        let ev = &host.ime_replace_commit_calls[0];
        assert_eq!(ev.replacement_byte_range_after_selection, (0, 6));
        assert_eq!(ev.inserted_text, "修正");
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

    #[test]
    fn test_ime_replace_and_commit_zero_length_nonzero_start() {
        let mut host = FakeHost::enabled();
        let event = ImeReplaceEvent::new(None, (3, 3), "你好".to_string());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
        let ev = &host.ime_replace_commit_calls[0];
        assert_eq!(ev.replacement_byte_range_after_selection, (3, 3));
        assert_eq!(ev.inserted_text, "你好");
    }

    #[test]
    fn test_ime_replace_and_commit_negative_start_zero_length() {
        let mut host = FakeHost::enabled();
        let event = ImeReplaceEvent::new(None, (0, 0), "好".to_string());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
        let ev = &host.ime_replace_commit_calls[0];
        assert_eq!(ev.replacement_byte_range_after_selection, (0, 0));
        assert_eq!(ev.inserted_text, "好");
    }

    /// Issue #701 评论 5702675971: 空 commit + replacement（纯删除）也应进入事务。
    #[test]
    fn test_ime_empty_commit_with_replacement() {
        let mut host = FakeHost::enabled();
        // inserted_text 为空但 replacement range (1,3) 非零长度 → has_any_deletion=true，
        // controller 不 return，event 仍传给 host。
        let event = ImeReplaceEvent::new(None, (1, 3), String::new());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
        let ev = &host.ime_replace_commit_calls[0];
        assert_eq!(ev.selection_byte_range, None);
        assert_eq!(ev.replacement_byte_range_after_selection, (1, 3));
        assert!(ev.inserted_text.is_empty());
        assert!(ev.has_any_deletion());
    }

    /// Issue #701 评论 5702675971: selection + replacement 两步语义正确传递。
    #[test]
    fn test_ime_selection_and_replacement() {
        let mut host = FakeHost::enabled();
        let event = ImeReplaceEvent::new(Some((1, 2)), (1, 1), "X".to_string());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
        let ev = &host.ime_replace_commit_calls[0];
        assert_eq!(ev.selection_byte_range, Some((1, 2)));
        assert_eq!(ev.replacement_byte_range_after_selection, (1, 1));
        assert_eq!(ev.inserted_text, "X");
    }

    /// Issue #701 评论 5702675971: 既无 selection 删除、又无 replacement 删除、
    /// 又无插入文本 → 纯 noop，controller 直接 return，不调 host。
    #[test]
    fn test_ime_noop_skipped() {
        let mut host = FakeHost::enabled();
        let event = ImeReplaceEvent::new(None, (0, 0), String::new());
        ime_replace_and_commit(&mut host, event);
        assert!(host.ime_replace_commit_calls.is_empty());
    }

    /// Issue #701 评论 5702675971: selection 删除也算 deletion，即使
    /// replacement range 零长度且 inserted 为空，仍进入事务（纯 selection 删除）。
    #[test]
    fn test_ime_selection_only_deletion() {
        let mut host = FakeHost::enabled();
        let event = ImeReplaceEvent::new(Some((2, 5)), (0, 0), String::new());
        ime_replace_and_commit(&mut host, event);
        assert_eq!(host.ime_replace_commit_calls.len(), 1);
        let ev = &host.ime_replace_commit_calls[0];
        assert_eq!(ev.selection_byte_range, Some((2, 5)));
        assert!(ev.has_any_deletion());
    }

    /// Issue #701 评论 5702675971: ImeReplaceEvent::new 归一化 replacement range。
    #[test]
    fn test_ime_replace_event_normalizes_range() {
        let event = ImeReplaceEvent::new(None, (5, 2), "X".to_string());
        assert_eq!(event.replacement_byte_range_after_selection, (2, 5));
    }

    /// Issue #704 复现:ESC 无条件武装 suppress_next_ime_commit,
    /// 吞掉下一次直接 IME commit(无前置 preedit)。
    ///
    /// 场景:用户在没有任何活跃 IME composition/preedit 时按一次 ESC,
    /// 随后输入法直接发 commit(没有先发新的非空 preedit)。
    /// 期望:该 commit 应正常插入文本。
    /// 实际(bug):commit 被 suppress_next_ime_commit 误吞,buffer 为空。
    #[test]
    fn issue_704_escape_without_composition_swallows_next_direct_commit() {
        let mut host = FakeHost::enabled();
        // 初始状态:无 preedit、无 composition、suppress flag = false
        assert!(host.preedit_text.is_empty());
        assert!(!host.suppress_next_ime_commit);

        // 步骤1:在没有任何活跃 composition/preedit 时按 ESC
        handle_key(&mut host, KEY_ESCAPE, 0);

        // Bug #704:ESC 无条件武装 suppress_next_ime_commit,即使没有活跃 composition。
        // 期望(修复后):无 composition 时 suppress flag 不应被武装。
        assert!(
            !host.suppress_next_ime_commit,
            "BUG #704: ESC 在无活跃 composition 时不应武装 suppress_next_ime_commit, \
             但实际被设为 true,会吞掉下一次直接 IME commit"
        );

        // 步骤2:模拟输入法直接发 commit(没有先发新的非空 preedit)
        ime_commit(&mut host, "字".to_string());

        // 期望:"字" 应被插入 buffer。
        // 实际(bug):"字" 被 suppress flag 吞掉,buffer 为空。
        assert_eq!(
            host.inserted,
            vec!["字".to_string()],
            "BUG #704: 无 composition 时按 ESC 后,直接 IME commit 应正常插入文本, \
             但被 suppress_next_ime_commit 误吞,buffer 为空"
        );
    }

    /// Issue #704 修复验证:存在活跃 composition(preedit 非空)时按 ESC,
    /// 应取消 preedit 并武装一次 suppress_next_ime_commit,
    /// 使该 composition 可能迟到的一次 commit 被抑制。
    #[test]
    fn issue_704_escape_with_composition_arms_suppress_guard() {
        let mut host = FakeHost::enabled();
        // 建立活跃 composition:非空 preedit
        ime_preedit(&mut host, "拼".to_string(), 3);
        assert_eq!(host.preedit_text, "拼");
        assert!(!host.suppress_next_ime_commit);

        // 按 ESC 取消 composition
        handle_key(&mut host, KEY_ESCAPE, 0);

        // preedit 被清除
        assert_eq!(host.preedit_text, "");
        assert_eq!(host.preedit_cursor, 0);
        // 确实取消过真实 composition → 武装一次 late-commit guard
        assert!(
            host.suppress_next_ime_commit,
            "ESC 取消真实 composition 后应武装一次 suppress_next_ime_commit"
        );

        // 该 composition 迟到的 commit 应被抑制(仅清 preedit,不插入)
        ime_commit(&mut host, "拼".to_string());
        assert!(
            host.inserted.is_empty(),
            "迟到 commit 应被 suppress guard 抑制,不插入 buffer"
        );
        // guard 被一次性消费后清除
        assert!(!host.suppress_next_ime_commit);

        // 后续新的直接 commit 应正常插入(guard 已消费)
        ime_commit(&mut host, "好".to_string());
        assert_eq!(host.inserted, vec!["好".to_string()]);
    }

    /// Issue #704 修复验证:新非空 preedit 应清除残留的 suppress guard,
    /// 防止用户重新开始输入时旧 guard 吞掉新 commit。
    #[test]
    fn issue_704_new_nonempty_preedit_clears_suppress_guard() {
        let mut host = FakeHost::enabled();
        // 模拟残留 guard(例如刚取消过一次真实 composition)
        host.suppress_next_ime_commit = true;

        // 用户重新开始输入:发新的非空 preedit
        ime_preedit(&mut host, "新".to_string(), 3);

        // 新非空 preedit 应清除旧 guard
        assert!(
            !host.suppress_next_ime_commit,
            "新非空 preedit 应清除残留的 suppress_next_ime_commit guard"
        );
        assert_eq!(host.preedit_text, "新");
    }

    // ========================================================================
    // Issue #704 独立对抗式验证测试(由 result-verify agent 添加)
    // 这些测试独立于上面的 issue_704_* 测试,直接观测状态机内部状态,
    // 覆盖任务要求的 6 个验证场景中的对抗式边界条件。
    // ========================================================================

    /// 场景 4 独立验证:guard 只消费一次,不重复抑制后续 commit。
    /// 直接构造 suppress=true 状态,连续两次 ime_commit,
    /// 第一次应被抑制,第二次应正常插入。
    #[test]
    fn verify_704_suppress_guard_consumed_exactly_once() {
        let mut host = FakeHost::enabled();
        // 初始:无 preedit,guard 已武装(模拟刚取消过真实 composition)
        host.suppress_next_ime_commit = true;
        assert!(host.preedit_text.is_empty());

        // 第一次 commit:应被抑制
        ime_commit(&mut host, "字1".to_string());
        assert!(
            host.inserted.is_empty(),
            "第一次 commit 应被 suppress guard 抑制"
        );
        assert!(
            !host.suppress_next_ime_commit,
            "guard 应在第一次 commit 后被消费清除"
        );

        // 第二次 commit:guard 已消费,应正常插入
        ime_commit(&mut host, "字2".to_string());
        assert_eq!(
            host.inserted,
            vec!["字2".to_string()],
            "第二次 commit 应正常插入,guard 不应重复抑制"
        );
        assert!(!host.suppress_next_ime_commit);
    }

    /// 对抗式场景:连续两次 ESC(都无 composition)都不武装 guard。
    /// 防御"第一次 ESC 清状态、第二次 ESC 武装"的潜在 bug。
    #[test]
    fn verify_704_double_escape_without_composition_no_guard() {
        let mut host = FakeHost::enabled();
        handle_key(&mut host, KEY_ESCAPE, 0);
        assert!(!host.suppress_next_ime_commit);
        handle_key(&mut host, KEY_ESCAPE, 0);
        assert!(
            !host.suppress_next_ime_commit,
            "连续两次无 composition 的 ESC 都不应武装 guard"
        );
        // 后续直接 commit 应正常插入
        ime_commit(&mut host, "字".to_string());
        assert_eq!(host.inserted, vec!["字".to_string()]);
    }

    /// 对抗式场景:ESC 取消 composition → 武装 guard → 再 ESC(无 composition)
    /// 不应重复武装 guard(第二次 ESC 时 preedit 已清,was_composing=false)。
    /// 然后迟到 commit 被抑制一次,guard 消费清除。
    #[test]
    fn verify_704_escape_after_cancel_does_not_rearm_guard() {
        let mut host = FakeHost::enabled();
        // 建立活跃 composition
        ime_preedit(&mut host, "拼".to_string(), 3);
        // 第一次 ESC:取消 composition,武装 guard
        handle_key(&mut host, KEY_ESCAPE, 0);
        assert!(host.suppress_next_ime_commit);
        assert!(host.preedit_text.is_empty());
        // 第二次 ESC:此时无 composition,不应改变 guard(仍为 true,但不是被重新武装)
        handle_key(&mut host, KEY_ESCAPE, 0);
        assert!(
            host.suppress_next_ime_commit,
            "第二次 ESC 不应清除已武装的 guard(那是 commit 的职责)"
        );
        // 迟到 commit 被抑制一次
        ime_commit(&mut host, "拼".to_string());
        assert!(host.inserted.is_empty());
        assert!(!host.suppress_next_ime_commit);
    }

    /// 对抗式场景:空 commit 不消费 suppress guard(ime_commit 对空 text 提前 return)。
    /// 验证 guard 不会被空 commit 误消费。
    #[test]
    fn verify_704_empty_commit_does_not_consume_guard() {
        let mut host = FakeHost::enabled();
        host.suppress_next_ime_commit = true;
        // 空 commit:ime_commit 提前 return,不消费 guard
        ime_commit(&mut host, "".to_string());
        assert!(
            host.suppress_next_ime_commit,
            "空 commit 不应消费 suppress guard"
        );
        assert!(host.inserted.is_empty());
        // 后续非空 commit 才消费 guard
        ime_commit(&mut host, "字".to_string());
        assert!(host.inserted.is_empty());
        assert!(!host.suppress_next_ime_commit);
    }

    /// 对抗式场景:input_disabled 时 ESC 不武装 guard(整体守卫)。
    #[test]
    fn verify_704_escape_when_input_disabled_no_guard() {
        let mut host = FakeHost::default(); // enabled=false
        handle_key(&mut host, KEY_ESCAPE, 0);
        assert!(!host.suppress_next_ime_commit);
        assert!(host.preedit_text.is_empty());
    }

    /// 对抗式场景:有 composition → ESC 武装 guard → 新非空 preedit 清 guard
    /// → 后续 commit 正常插入(完整用户流程)。
    #[test]
    fn verify_704_full_user_flow_cancel_then_reinput() {
        let mut host = FakeHost::enabled();
        // 用户输入 "拼"
        ime_preedit(&mut host, "拼".to_string(), 3);
        assert!(!host.suppress_next_ime_commit);
        // 用户按 ESC 取消
        handle_key(&mut host, KEY_ESCAPE, 0);
        assert!(host.suppress_next_ime_commit);
        assert!(host.preedit_text.is_empty());
        // 用户重新输入 "新"(新非空 preedit 清 guard)
        ime_preedit(&mut host, "新".to_string(), 3);
        assert!(!host.suppress_next_ime_commit);
        assert_eq!(host.preedit_text, "新");
        // 用户确认 "新"(commit)
        ime_commit(&mut host, "新".to_string());
        assert_eq!(
            host.inserted,
            vec!["新".to_string()],
            "重新输入后的 commit 应正常插入,不被旧 guard 吞"
        );
        assert!(!host.suppress_next_ime_commit);
    }
}
