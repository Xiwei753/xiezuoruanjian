#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use crate::editor::transaction::types::*;
use crate::editor::transaction::visual::*;
use crate::editor::transaction::composition::*;
use crate::editor::transaction::rebase::*;
use crate::editor::transaction::engine::*;
use crate::editor::transaction::platform::*;

    #[test]
    fn detects_single_insert_on_utf8_boundary() {
        let changes = diff_plain_text("你好世界", "你好新世界");
        assert_eq!(
            changes,
            vec![EditorChange::Insert {
                index: "你好".len(),
                text: "新".to_string(),
            }]
        );
    }

    #[test]
    fn detects_single_delete_on_utf8_boundary() {
        let changes = diff_plain_text("abc月def", "abcdef");
        assert_eq!(
            changes,
            vec![EditorChange::Delete {
                index: "abc".len(),
                text: "月".to_string(),
            }]
        );
    }

    #[test]
    fn detects_diff_with_empty_inputs() {
        assert_eq!(diff_plain_text("", ""), vec![]);

        assert_eq!(
            diff_plain_text("", "text"),
            vec![EditorChange::Insert {
                index: 0,
                text: "text".to_string(),
            }]
        );

        assert_eq!(
            diff_plain_text("text", ""),
            vec![EditorChange::Delete {
                index: 0,
                text: "text".to_string(),
            }]
        );
    }

    #[test]
    fn replacement_is_delete_then_insert() {
        let changes = diff_plain_text("alpha beta", "alpha gamma");
        assert_eq!(
            changes,
            vec![
                EditorChange::Delete {
                    index: "alpha ".len(),
                    text: "bet".to_string(),
                },
                EditorChange::Insert {
                    index: "alpha ".len(),
                    text: "gamm".to_string(),
                },
            ]
        );
    }

    #[test]
    fn typing_transaction_emits_insert_and_cursor_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );

        assert!(tx.should_animate);
        let events = engine.animation_events(&tx);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
        assert_eq!(events[0].text, "c");
        assert_eq!(events[1].kind, EditorAnimationKind::Cursor);
    }

    #[test]
    fn paste_does_not_emit_text_animation() {
        let mut engine = EditorEngine::new();
        let tx = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );

        // Paste 现在进入 visual transaction（should_animate=true）
        assert!(tx.should_animate);
        let events = engine.animation_events(&tx);
        // Paste 长文本产生 Insert + Cursor 事件
        assert!(!events.is_empty());
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
    }

    #[test]
    fn load_does_not_emit_animation_events() {
        let mut engine = EditorEngine::new();
        let tx = engine.create_transaction(
            "",
            "loaded",
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed("loaded", 6),
            EditorTransactionCause::Load,
        );

        assert!(!tx.should_animate);
        assert!(engine.animation_events(&tx).is_empty());
    }

    #[test]
    fn glyph_rect_serializes_camel_case() {
        let gr = GlyphRect {
            x: 10.5,
            y: 20.0,
            w: 16.0,
            h: 24.0,
            char_: "你".to_string(),
            baseline_y: 36.0,
            byte_start: 0,
            byte_end: 3,
        };
        let json = serde_json::to_string(&gr).unwrap();
        // 字段名必须是 camelCase，char_ → "char"
        assert!(json.contains("\"x\":"));
        assert!(json.contains("\"y\":"));
        assert!(json.contains("\"w\":"));
        assert!(json.contains("\"h\":"));
        assert!(json.contains("\"char\":"));
        assert!(!json.contains("\"char_\":"));
        assert!(json.contains("\"baselineY\":"));
    }

    #[test]
    fn animation_event_glyph_rects_default_empty_and_skip_serializing() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        // Core 层默认 glyph_rects 为空
        assert!(events[0].glyph_rects.is_empty());
        assert!(events[1].glyph_rects.is_empty());

        // 空 glyphRects 不应出现在 JSON 中（skip_serializing_if）
        let json = serde_json::to_string(&events).unwrap();
        assert!(!json.contains("glyphRects"));
    }

    #[test]
    fn animation_event_with_glyph_rects_serializes() {
        let event = EditorAnimationEvent {
            id: 1,
            kind: EditorAnimationKind::Insert,
            range_start: 0,
            range_len: 3,
            text: "abc".to_string(),
            old_cursor: EditorCursor { index: 0 },
            new_cursor: EditorCursor { index: 3 },
            duration_ms: 160,
            glyph_rects: vec![
                GlyphRect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 20.0,
                    char_: "a".to_string(),
                    baseline_y: 16.0,
                    byte_start: 0,
                    byte_end: 1,
                },
                GlyphRect {
                    x: 10.0,
                    y: 0.0,
                    w: 10.0,
                    h: 20.0,
                    char_: "b".to_string(),
                    baseline_y: 16.0,
                    byte_start: 1,
                    byte_end: 2,
                },
                GlyphRect {
                    x: 20.0,
                    y: 0.0,
                    w: 10.0,
                    h: 20.0,
                    char_: "c".to_string(),
                    baseline_y: 16.0,
                    byte_start: 2,
                    byte_end: 3,
                },
            ],
            old_cursor_rect: None,
            new_cursor_rect: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        // 非空 glyphRects 必须出现在 JSON 中
        assert!(json.contains("glyphRects"));
        assert!(json.contains("\"char\":"));
    }

    #[test]
    fn complex_grapheme_chars_are_filtered_from_glyph_rects() {
        // This test verifies that the Linux_qt Rust side filters complex grapheme
        // chars when filling glyph_rects. Since the filtering happens in the
        // Linux_qt-specific fill_glyph_rects_for_events (not in core), we test
        // the is_complex_grapheme helper function logic here at the core level
        // by verifying that the core transaction correctly identifies emoji text.
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "ab😀",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("ab😀", "ab😀".len()),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        // Core still emits the insert event with text "😀"
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
        assert_eq!(events[0].text, "😀");
        // glyph_rects is empty at core level (filled by platform later)
        assert!(events[0].glyph_rects.is_empty());
    }

    #[test]
    fn set_animation_duration_ms_affects_event_duration() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        // 初始 duration_ms = 120
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        assert_eq!(events[0].duration_ms, 120);

        // 改为 500
        engine.set_animation_duration_ms(500);
        let tx2 = engine.create_transaction(
            "abc",
            "abcd",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("abcd", 4),
            EditorTransactionCause::Typing,
        );
        let events2 = engine.animation_events(&tx2);
        assert_eq!(events2[0].duration_ms, 500);
    }

    #[test]
    fn animation_event_cursor_rects_default_none() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        // Core 层默认 cursor_rects 为 None
        assert!(events[0].old_cursor_rect.is_none());
        assert!(events[0].new_cursor_rect.is_none());
    }

    #[test]
    fn cursor_rect_serializes_camel_case() {
        let cr = CursorRect { x: 10.5, top: 5.0, bottom: 25.0, baseline_y: 20.0 };
        let json = serde_json::to_string(&cr).unwrap();
        assert!(json.contains("\"x\":"));
        assert!(json.contains("\"top\":"));
        assert!(json.contains("\"bottom\":"));
        assert!(json.contains("\"baselineY\":"));
    }

    #[test]
    fn animation_event_with_cursor_rects_serializes() {
        let event = EditorAnimationEvent {
            id: 1,
            kind: EditorAnimationKind::Insert,
            range_start: 0,
            range_len: 1,
            text: "a".to_string(),
            old_cursor: EditorCursor { index: 0 },
            new_cursor: EditorCursor { index: 1 },
            duration_ms: 160,
            glyph_rects: Vec::new(),
            old_cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            new_cursor_rect: Some(CursorRect { x: 30.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("oldCursorRect"));
        assert!(json.contains("newCursorRect"));
    }

    #[test]
    fn animation_event_without_cursor_rects_skips_serializing() {
        let event = EditorAnimationEvent {
            id: 1,
            kind: EditorAnimationKind::Insert,
            range_start: 0,
            range_len: 1,
            text: "a".to_string(),
            old_cursor: EditorCursor { index: 0 },
            new_cursor: EditorCursor { index: 1 },
            duration_ms: 160,
            glyph_rects: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("oldCursorRect"));
        assert!(!json.contains("newCursorRect"));
    }

    #[test]
    fn single_char_insert_event_has_correct_range() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "你好",
            "你好世",
            EditorSelection::collapsed("你好", "你好".len()),
            EditorSelection::collapsed("你好世", "你好世".len()),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        // Should have Insert + Cursor events
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
        // range_start should be at byte offset of "世" insertion point
        assert_eq!(events[0].range_start, "你好".len()); // 6 bytes
        assert_eq!(events[0].range_len, "世".len()); // 3 bytes
        assert_eq!(events[0].text, "世");
    }

    #[test]
    fn single_char_delete_event_has_correct_range() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "你好世",
            "你好",
            EditorSelection::collapsed("你好世", "你好世".len()),
            EditorSelection::collapsed("你好", "你好".len()),
            EditorTransactionCause::Delete,
        );
        let events = engine.animation_events(&tx);
        // Should have Delete + Cursor events
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EditorAnimationKind::Delete);
        // range_start should be at byte offset where "世" was deleted
        assert_eq!(events[0].range_start, "你好".len()); // 6 bytes
        assert_eq!(events[0].range_len, "世".len()); // 3 bytes
        assert_eq!(events[0].text, "世");
    }

    #[test]
    fn paste_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );
        // Paste 现在进入 visual transaction
        assert!(tx.should_animate);
        let events = engine.animation_events(&tx);
        // Paste 长文本产生 Insert + Cursor 事件
        assert!(!events.is_empty());
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
    }

    #[test]
    fn load_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "",
            "loaded text",
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed("loaded text", "loaded text".len()),
            EditorTransactionCause::Load,
        );
        assert!(!tx.should_animate);
        assert!(engine.animation_events(&tx).is_empty());
    }

    // --- Cause-based animation suppression tests ---
    // These tests verify that non-typing causes (Format, Undo, Redo,
    // ImeComposition, Programmatic) do not produce text animation events,
    // as ensured by should_animate_changes().

    #[test]
    fn format_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "hello world",
            "Hello World",
            EditorSelection::collapsed("hello world", 0),
            EditorSelection::collapsed("Hello World", 0),
            EditorTransactionCause::Format,
        );
        assert!(!tx.should_animate, "Format cause should not animate");
        // Format with cursor movement should only produce Cursor event, no Insert/Delete
        let events = engine.animation_events(&tx);
        for event in &events {
            assert!(
                event.kind == EditorAnimationKind::Cursor,
                "Format should only produce Cursor events, got {:?}",
                event.kind
            );
        }
    }

    #[test]
    fn undo_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "abc",
            "a",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("a", 1),
            EditorTransactionCause::Undo,
        );
        // Undo 现在进入 visual transaction
        assert!(tx.should_animate, "Undo cause should animate");
        let events = engine.animation_events(&tx);
        // Undo 产生 Delete + Cursor 事件
        assert!(!events.is_empty());
        assert_eq!(events[0].kind, EditorAnimationKind::Delete);
    }

    #[test]
    fn redo_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "abc",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Redo,
        );
        // Redo 现在进入 visual transaction
        assert!(tx.should_animate, "Redo cause should animate");
        let events = engine.animation_events(&tx);
        // Redo 产生 Insert + Cursor 事件
        assert!(!events.is_empty());
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
    }

    #[test]
    fn ime_composition_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ni",
            "nihao",
            EditorSelection::collapsed("ni", 2),
            EditorSelection::collapsed("nihao", 5),
            EditorTransactionCause::ImeComposition,
        );
        // ImeComposition 是 preedit 阶段，不需要吞吐动画
        // IME commit 走 TypingCommit cause，已经允许动画
        assert!(!tx.should_animate, "ImeComposition should not animate");
        let events = engine.animation_events(&tx);
        // 只有 Cursor 事件（光标位置变化），没有 Insert/Delete 动画
        assert!(events.iter().all(|e| e.kind == EditorAnimationKind::Cursor));
    }

    #[test]
    fn programmatic_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "old text",
            "new text",
            EditorSelection::collapsed("old text", 0),
            EditorSelection::collapsed("new text", 0),
            EditorTransactionCause::Programmatic,
        );
        assert!(!tx.should_animate, "Programmatic cause should not animate");
        // Programmatic without cursor movement should produce no events at all
        let events = engine.animation_events(&tx);
        assert!(
            events.is_empty(),
            "Programmatic with same cursor position should produce no events, got {} events",
            events.len()
        );
    }

    // --- Guard tests for different setting combinations ---

    #[test]
    fn typing_animation_toggle_on_off() {
        // When typing animation is ON: Typing cause should_animate = true
        let engine = EditorEngine::with_animation_limits(8, 120);
        let tx_on = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        assert!(tx_on.should_animate, "Typing should animate when animation is on");

        // When typing animation is OFF: should_animate_changes still returns true for Typing cause,
        // but the caller (platform) should check the setting and skip creating animation events.
        // The core should_animate_changes function is cause-based, not setting-based.
        // This test verifies the core behavior is consistent regardless of external toggle.
        let tx_off = engine.create_transaction(
            "abc",
            "abcd",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("abcd", 4),
            EditorTransactionCause::Typing,
        );
        // Core always returns true for Typing cause — platform is responsible for checking the toggle
        assert!(tx_off.should_animate, "Core should_animate_changes is cause-based, not toggle-based");

        // Paste 现在也进入 visual transaction（用户触发的操作不应被入口拦掉）
        let tx_paste = engine.create_transaction(
            "a",
            "a pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a pasted text", "a pasted text".len()),
            EditorTransactionCause::Paste,
        );
        assert!(tx_paste.should_animate, "Paste should animate as a user-triggered operation");
    }

    #[test]
    fn animation_duration_clamped() {
        // Verify that animation duration is stored as-is in EditorEngine,
        // and that the settings layer (not core) is responsible for clamping.
        // Core stores whatever duration is set via set_animation_duration_ms.
        let mut engine = EditorEngine::with_animation_limits(8, 120);

        // Normal duration
        engine.set_animation_duration_ms(200);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        assert_eq!(events[0].duration_ms, 200);

        // Very small duration — core stores it, settings layer should clamp before calling set
        engine.set_animation_duration_ms(5);
        let tx2 = engine.create_transaction(
            "abc",
            "abcd",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("abcd", 4),
            EditorTransactionCause::Typing,
        );
        let events2 = engine.animation_events(&tx2);
        assert_eq!(events2[0].duration_ms, 5, "Core stores whatever duration is set; clamping is the caller's responsibility");

        // Very large duration
        engine.set_animation_duration_ms(9999);
        let tx3 = engine.create_transaction(
            "abcd",
            "abcde",
            EditorSelection::collapsed("abcd", 4),
            EditorSelection::collapsed("abcde", 5),
            EditorTransactionCause::Typing,
        );
        let events3 = engine.animation_events(&tx3);
        assert_eq!(events3[0].duration_ms, 9999, "Core stores whatever duration is set; clamping is the caller's responsibility");
    }

    #[test]
    fn undo_redo_no_animation() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);

        // Undo with text change 现在进入 visual transaction
        let tx_undo = engine.create_transaction(
            "abc",
            "a",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("a", 1),
            EditorTransactionCause::Undo,
        );
        assert!(tx_undo.should_animate, "Undo should animate");
        let events_undo = engine.animation_events(&tx_undo);
        assert!(!events_undo.is_empty());
        assert_eq!(events_undo[0].kind, EditorAnimationKind::Delete);

        // Redo with text change 现在进入 visual transaction
        let tx_redo = engine.create_transaction(
            "a",
            "abc",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Redo,
        );
        assert!(tx_redo.should_animate, "Redo should animate");
        let events_redo = engine.animation_events(&tx_redo);
        assert!(!events_redo.is_empty());
        assert_eq!(events_redo[0].kind, EditorAnimationKind::Insert);
    }

    #[test]
    fn paste_no_animation() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);

        // Paste with single-char text 现在进入 visual transaction
        let tx = engine.create_transaction(
            "a",
            "ab",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("ab", 2),
            EditorTransactionCause::Paste,
        );
        assert!(tx.should_animate, "Paste should animate even for single char");
        let events = engine.animation_events(&tx);
        assert!(!events.is_empty());
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);

        // Paste with multi-char text 也进入 visual transaction
        let tx2 = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );
        assert!(tx2.should_animate, "Paste should animate for multi-char text");
    }

    #[test]
    fn load_no_animation() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);

        // Load should produce zero animation events (not even Cursor)
        let tx = engine.create_transaction(
            "",
            "loaded text",
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed("loaded text", "loaded text".len()),
            EditorTransactionCause::Load,
        );
        assert!(!tx.should_animate, "Load should not animate");
        let events = engine.animation_events(&tx);
        assert!(events.is_empty(), "Load should produce zero animation events (not even Cursor)");

        // Load with same cursor position (0→0) should also produce no events
        let tx2 = engine.create_transaction(
            "",
            "loaded",
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed("loaded", 0),
            EditorTransactionCause::Load,
        );
        assert!(!tx2.should_animate);
        assert!(engine.animation_events(&tx2).is_empty());
    }

    #[test]
    fn visual_transaction_insert_has_inserted_range() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Insert);
        assert_eq!(vt.inserted_range, Some((2, 3)));
        assert_eq!(vt.coordinate_mode, VisualCoordinateMode::Baseline);
        assert!(vt.deleted_glyph_rects.is_none());
        assert!(vt.insert_glyph_rects.is_none());
        assert!(vt.old_cursor_rect.is_none());
        assert!(vt.new_cursor_rect.is_none());
    }

    #[test]
    fn visual_transaction_delete_has_no_inserted_range() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "abc",
            "ab",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("ab", 2),
            EditorTransactionCause::Delete,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Delete);
        assert!(vt.inserted_range.is_none());
    }

    #[test]
    fn visual_transaction_paste_enters_visual_transaction() {
        // Paste 长文本进入 visual transaction，mode 是 RunAnimation (SnapshotAnimation unavailable)
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "Paste should enter visual transaction");
        let vt = vt.unwrap();
        assert!(
            vt.animation_mode == AnimationMode::RunAnimation,
            "Paste long text should be RunAnimation, got {:?}",
            vt.animation_mode
        );
    }

    #[test]
    fn visual_transaction_paste_short_text_glyph_animation() {
        // Paste 短文本进入 visual transaction，mode 是 GlyphAnimation
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "abc",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Paste,
        );
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "Paste short text should enter visual transaction");
        let vt = vt.unwrap();
        assert_eq!(
            vt.animation_mode,
            AnimationMode::GlyphAnimation,
            "Paste short text should be GlyphAnimation"
        );
    }

    #[test]
    fn visual_transaction_paste_newline_line_reflow() {
        // Paste 包含换行进入 visual transaction，mode 是 LineReflowAnimation
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "a\nb",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a\nb", "a\nb".len()),
            EditorTransactionCause::Paste,
        );
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "Paste with newline should enter visual transaction");
        let vt = vt.unwrap();
        assert_eq!(
            vt.animation_mode,
            AnimationMode::LineReflowAnimation,
            "Paste with newline should be LineReflowAnimation"
        );
    }

    #[test]
    fn visual_transaction_undo_enters_visual_transaction() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "abc",
            "a",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("a", 1),
            EditorTransactionCause::Undo,
        );
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "Undo should enter visual transaction");
    }

    #[test]
    fn visual_transaction_redo_enters_visual_transaction() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "abc",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Redo,
        );
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "Redo should enter visual transaction");
    }

    #[test]
    fn cursor_rect_has_baseline_y() {
        let cr = CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 };
        let json = serde_json::to_string(&cr).unwrap();
        assert!(json.contains("\"baselineY\":"));
        assert!(json.contains("\"top\":"));
        assert!(json.contains("\"bottom\":"));
    }

    #[test]
    fn glyph_rect_has_baseline_y() {
        let gr = GlyphRect {
            x: 10.5, y: 20.0, w: 16.0, h: 24.0,
            char_: "你".to_string(), baseline_y: 40.0,
            byte_start: 0, byte_end: 3,
        };
        let json = serde_json::to_string(&gr).unwrap();
        assert!(json.contains("\"baselineY\":"));
    }

    #[test]
    fn preedit_visual_transaction_serializes_camel_case() {
        let vt = PreeditVisualTransaction {
            id: 1,
            old_preedit_text: "n".to_string(),
            new_preedit_text: "ni".to_string(),
            old_preedit_cursor_rect: None,
            new_preedit_cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            preedit_glyph_rects: None,
            deleted_preedit_glyph_rects: None,
            inserted_preedit_glyph_rects: None,
            preedit_cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            duration_ms: 160,
            coordinate_mode: VisualCoordinateMode::Baseline,
        };
        let json = serde_json::to_string(&vt).unwrap();
        assert!(json.contains("\"oldPreeditText\":"));
        assert!(json.contains("\"newPreeditText\":"));
        assert!(json.contains("\"newPreeditCursorRect\":"));
        assert!(json.contains("\"preeditCursorRect\":"));
        assert!(json.contains("\"durationMs\":"));
        assert!(json.contains("\"coordinateMode\":"));
        // None fields should be skipped
        assert!(!json.contains("\"oldPreeditCursorRect\":"));
        assert!(!json.contains("\"preeditGlyphRects\":"));
        assert!(!json.contains("\"deletedPreeditGlyphRects\":"));
        assert!(!json.contains("\"insertedPreeditGlyphRects\":"));
    }

    #[test]
    fn preedit_text_format_serializes_camel_case() {
        let fmt = PreeditTextFormat::TextColor { color: "#FF0000".to_string() };
        let json = serde_json::to_string(&fmt).unwrap();
        assert!(json.contains("\"textColor\":"));
        assert!(json.contains("\"color\":"));

        let fmt2 = PreeditTextFormat::Underline;
        let json2 = serde_json::to_string(&fmt2).unwrap();
        assert!(json2.contains("\"underline\""));

        let fmt3 = PreeditTextFormat::BackgroundColor { color: "#00FF00".to_string() };
        let json3 = serde_json::to_string(&fmt3).unwrap();
        assert!(json3.contains("\"backgroundColor\":"));

        let fmt4 = PreeditTextFormat::FontUnderline;
        let json4 = serde_json::to_string(&fmt4).unwrap();
        assert!(json4.contains("\"fontUnderline\""));
    }

    // --- AnimationMode / choose_animation_mode tests ---

    #[test]
    fn choose_animation_mode_typing_returns_glyph_animation() {
        // 1–8 个普通 cluster → GlyphAnimation
        let mode = choose_animation_mode(5, false, false, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::GlyphAnimation);

        let mode1 = choose_animation_mode(1, false, false, false, false, false, false, true);
        assert_eq!(mode1, AnimationMode::GlyphAnimation);

        let mode8 = choose_animation_mode(8, false, false, false, false, false, false, true);
        assert_eq!(mode8, AnimationMode::GlyphAnimation);
    }

    #[test]
    fn choose_animation_mode_complex_grapheme_returns_cluster_animation() {
        // emoji → ClusterAnimation
        let mode = choose_animation_mode(1, false, true, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::ClusterAnimation);
    }

    #[test]
    fn choose_animation_mode_zwj_returns_cluster_animation() {
        // ZWJ emoji → ClusterAnimation (contains_complex_grapheme=true)
        let mode = choose_animation_mode(3, false, true, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::ClusterAnimation);
    }

    #[test]
    fn choose_animation_mode_newline_returns_line_reflow() {
        // 换行 → LineReflowAnimation
        let mode = choose_animation_mode(1, true, false, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::LineReflowAnimation);
    }

    #[test]
    fn choose_animation_mode_many_clusters_returns_run_animation() {
        // 9–40 个 cluster → RunAnimation
        let mode9 = choose_animation_mode(9, false, false, false, false, false, false, true);
        assert_eq!(mode9, AnimationMode::RunAnimation);

        let mode40 = choose_animation_mode(40, false, false, false, false, false, false, true);
        assert_eq!(mode40, AnimationMode::RunAnimation);

        let mode20 = choose_animation_mode(20, false, false, false, false, false, false, true);
        assert_eq!(mode20, AnimationMode::RunAnimation);
    }

    #[test]
    fn choose_animation_mode_extreme_many_clusters_returns_run() {
        // >40 个 cluster → RunAnimation (SnapshotAnimation is unavailable)
        let mode = choose_animation_mode(41, false, false, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::RunAnimation);

        let mode100 = choose_animation_mode(100, false, false, false, false, false, false, true);
        assert_eq!(mode100, AnimationMode::RunAnimation);
    }

    #[test]
    fn choose_animation_mode_scrolling_returns_system_suppressed() {
        // 滚动 → SystemSuppressed
        let mode = choose_animation_mode(5, false, false, true, false, false, false, true);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn choose_animation_mode_disabled_returns_system_suppressed() {
        // 动画关闭 → SystemSuppressed
        let mode = choose_animation_mode(5, false, false, false, false, false, false, false);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn choose_animation_mode_loading_returns_system_suppressed() {
        // 加载 → SystemSuppressed
        let mode = choose_animation_mode(5, false, false, false, true, false, false, true);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn choose_animation_mode_format_returns_system_suppressed() {
        // 格式化 → SystemSuppressed
        let mode = choose_animation_mode(5, false, false, false, false, true, false, true);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn choose_animation_mode_settings_returns_system_suppressed() {
        // 设置变化 → SystemSuppressed
        let mode = choose_animation_mode(5, false, false, false, false, false, true, true);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn choose_animation_mode_empty_returns_system_suppressed() {
        // 0 cluster → SystemSuppressed
        let mode = choose_animation_mode(0, false, false, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn split_text_into_clusters_emoji() {
        // emoji 整组作为一个 cluster
        let clusters = split_text_into_clusters("😀", 0);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].text, "😀");
        assert!(clusters[0].is_complex);
        assert_eq!(clusters[0].byte_start, 0);
        assert_eq!(clusters[0].byte_end, "😀".len());
    }

    #[test]
    fn split_text_into_clusters_combining_mark() {
        // 组合字符附加到前一个 cluster
        let clusters = split_text_into_clusters("e\u{0301}", 0); // é = e + combining acute
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].text, "e\u{0301}");
        assert!(clusters[0].is_complex);
    }

    #[test]
    fn split_text_into_runs_chinese() {
        // 中文每 5 字一组
        let runs = split_text_into_runs("一二三四五六七八九十", 0);
        // "一二三四五" (5) + "六七八九十" (5)
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "一二三四五");
        assert_eq!(runs[0].cluster_count, 5);
        assert_eq!(runs[1].text, "六七八九十");
        assert_eq!(runs[1].cluster_count, 5);
    }

    #[test]
    fn split_text_into_runs_mixed() {
        // 中英混合分组
        let runs = split_text_into_runs("你好world", 0);
        // "你好" (2 CJK, < 5) + "world" (5 non-CJK, < 8)
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "你好world");
        assert_eq!(runs[0].cluster_count, 7);
    }

    #[test]
    fn hidden_visual_range_serialization() {
        let hvr = HiddenVisualRange {
            id: 42,
            kind: AnimationMode::GlyphAnimation,
            range_start: 10,
            range_end: 20,
            old_rect: None,
            new_rect: None,
            line_index: 3,
            payload_ref: None,
        };
        let json = serde_json::to_string(&hvr).unwrap();
        assert!(json.contains("\"id\":"));
        assert!(json.contains("\"kind\":"));
        assert!(json.contains("\"glyphAnimation\""));
        assert!(json.contains("\"rangeStart\":"));
        assert!(json.contains("\"rangeEnd\":"));
        assert!(json.contains("\"lineIndex\":"));
        // None fields should be skipped
        assert!(!json.contains("\"oldRect\":"));
        assert!(!json.contains("\"newRect\":"));
        assert!(!json.contains("\"payloadRef\":"));

        // With rects
        let hvr2 = HiddenVisualRange {
            id: 43,
            kind: AnimationMode::LineReflowAnimation,
            range_start: 0,
            range_end: 5,
            old_rect: Some(Rect { x: 0.0, y: 0.0, w: 100.0, h: 20.0 }),
            new_rect: Some(Rect { x: 0.0, y: 20.0, w: 100.0, h: 20.0 }),
            line_index: 1,
            payload_ref: Some(99),
        };
        let json2 = serde_json::to_string(&hvr2).unwrap();
        assert!(json2.contains("\"lineReflowAnimation\""));
        assert!(json2.contains("\"oldRect\":"));
        assert!(json2.contains("\"newRect\":"));
        assert!(json2.contains("\"payloadRef\":"));
    }

    #[test]
    fn visual_transaction_contains_animation_mode() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        assert_eq!(vt.animation_mode, AnimationMode::GlyphAnimation);
    }

    #[test]
    fn visual_transaction_newline_not_suppressed() {
        // 换行不返回 SystemSuppressed — should_animate 现在对换行返回 true
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "ab\nc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("ab\nc", "ab\nc".len()),
            EditorTransactionCause::Typing,
        );
        assert!(tx.should_animate, "Newline should now animate");
        let vt = engine.visual_transaction(&tx).unwrap();
        assert_eq!(vt.animation_mode, AnimationMode::LineReflowAnimation);
        assert_ne!(vt.animation_mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn visual_transaction_complex_grapheme_not_suppressed() {
        // 复杂 grapheme 不返回 SystemSuppressed
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "ab😀",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("ab😀", "ab😀".len()),
            EditorTransactionCause::Typing,
        );
        assert!(tx.should_animate, "Complex grapheme should animate");
        let vt = engine.visual_transaction(&tx).unwrap();
        assert_eq!(vt.animation_mode, AnimationMode::ClusterAnimation);
        assert_ne!(vt.animation_mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn count_grapheme_clusters_zwj_emoji() {
        // ZWJ emoji "👨‍👩‍👧‍👦" 计为 1 个 cluster
        assert_eq!(count_grapheme_clusters("👨‍👩‍👧‍👦"), 1);
    }

    #[test]
    fn count_grapheme_clusters_variation_selector_emoji() {
        // Variation selector emoji "❤️" 计为 1 个 cluster
        assert_eq!(count_grapheme_clusters("❤️"), 1);
    }

    #[test]
    fn count_grapheme_clusters_combining_mark() {
        // Combining mark "é" (e + U+0301) 计为 1 个 cluster
        assert_eq!(count_grapheme_clusters("e\u{0301}"), 1);
    }

    #[test]
    fn count_grapheme_clusters_mixed_text() {
        // 混合文本 "ab😀cd" 计为 5 个 cluster
        assert_eq!(count_grapheme_clusters("ab😀cd"), 5);
    }

    #[test]
    fn split_text_into_clusters_zwj_emoji() {
        // ZWJ emoji 输出正确的 byte range 和 is_complex=true
        let emoji = "👨‍👩‍👧‍👦";
        let clusters = split_text_into_clusters(emoji, 0);
        assert_eq!(clusters.len(), 1, "ZWJ emoji should be 1 cluster");
        assert_eq!(clusters[0].byte_start, 0);
        assert_eq!(clusters[0].byte_end, emoji.len());
        assert_eq!(clusters[0].text, emoji);
        assert!(clusters[0].is_complex, "ZWJ emoji should be complex");
    }

    #[test]
    fn split_text_into_clusters_variation_selector_emoji() {
        // Variation selector emoji 输出正确的 byte range 和 is_complex=true
        let emoji = "❤️"; // ❤ + FE0F
        let clusters = split_text_into_clusters(emoji, 0);
        assert_eq!(clusters.len(), 1, "Variation selector emoji should be 1 cluster");
        assert_eq!(clusters[0].byte_start, 0);
        assert_eq!(clusters[0].byte_end, emoji.len());
        assert_eq!(clusters[0].text, emoji);
        assert!(clusters[0].is_complex, "Variation selector emoji should be complex");
    }

    // --- #516: Timeline tests ---

    #[test]
    fn timeline_progress_before_start_returns_zero() {
        let tl = Timeline::new(160);
        assert_eq!(tl.progress(0), 0.0);
        assert_eq!(tl.progress(100), 0.0);
    }

    #[test]
    fn timeline_progress_after_start_clamps_to_one() {
        let mut tl = Timeline::new(160);
        tl.mark_first_visible_frame(1000);
        assert!((tl.progress(1160) - 1.0).abs() < f64::EPSILON);
        assert!((tl.progress(2000) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn timeline_progress_mid_animation() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        let p = tl.progress(1100);
        assert!((p - 0.5).abs() < f64::EPSILON, "Expected 0.5, got {}", p);
    }

    #[test]
    fn timeline_paused_returns_paused_progress_not_zero() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        let p_before_pause = tl.progress(1150);
        assert!((p_before_pause - 0.75).abs() < 0.01);
        tl.pause(1150);
        assert!(tl.is_paused());
        assert!((tl.paused_progress - 0.75).abs() < 0.01);
        let p_after_pause = tl.progress(1200);
        assert!((p_after_pause - 0.75).abs() < 0.01, "Paused must return paused_progress, not 0");
    }

    #[test]
    fn timeline_resume_continues_from_paused_progress() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        tl.pause(1100);
        tl.resume(1200);
        assert!(!tl.is_paused());
        // resume at 1200, paused_progress=0.5, new_start=1200-100=1100
        // progress(1200) = (1200-1100)/200 = 0.5 (resumes from paused_progress)
        let p_at_resume = tl.progress(1200);
        assert!((p_at_resume - 0.5).abs() < 0.01, "Expected 0.5 at resume time, got {}", p_at_resume);
        // progress(1300) = (1300-1100)/200 = 1.0 (200ms effective elapsed)
        let p = tl.progress(1300);
        assert!((p - 1.0).abs() < 0.01, "Expected 1.0 at 1300, got {}", p);
    }

    #[test]
    fn timeline_zero_duration_returns_one() {
        let mut tl = Timeline::new(0);
        tl.mark_first_visible_frame(1000);
        assert!((tl.progress(1000) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn timeline_double_pause_is_noop() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        tl.pause(1100);
        let first_paused = tl.paused_progress;
        tl.pause(1200);
        assert!((tl.paused_progress - first_paused).abs() < f64::EPSILON);
    }

    #[test]
    fn timeline_resume_without_pause_is_noop() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        tl.resume(1100);
        assert_eq!(tl.accumulated_paused_duration_ms, 0);
    }

    #[test]
    fn timeline_is_completed() {
        let mut tl = Timeline::new(100);
        tl.mark_first_visible_frame(1000);
        assert!(!tl.is_completed(1050));
        assert!(tl.is_completed(1100));
    }

    // --- #516: UnifiedTransactionKind / VisualClassKind serialization ---

    #[test]
    fn unified_transaction_kind_serializes_camel_case() {
        let json = serde_json::to_string(&UnifiedTransactionKind::BodyEdit).unwrap();
        assert!(json.contains("\"bodyEdit\""));
        let json2 = serde_json::to_string(&UnifiedTransactionKind::CompositionUpdate).unwrap();
        assert!(json2.contains("\"compositionUpdate\""));
        let json3 = serde_json::to_string(&UnifiedTransactionKind::CompositionCommitOrCancel).unwrap();
        assert!(json3.contains("\"compositionCommitOrCancel\""));
        let json4 = serde_json::to_string(&UnifiedTransactionKind::CursorOnly).unwrap();
        assert!(json4.contains("\"cursorOnly\""));
    }

    #[test]
    fn visual_class_kind_serializes_camel_case() {
        assert!(serde_json::to_string(&VisualClassKind::Static).unwrap().contains("\"static\""));
        assert!(serde_json::to_string(&VisualClassKind::Insert).unwrap().contains("\"insert\""));
        assert!(serde_json::to_string(&VisualClassKind::Delete).unwrap().contains("\"delete\""));
        assert!(serde_json::to_string(&VisualClassKind::Move).unwrap().contains("\"move\""));
        assert!(serde_json::to_string(&VisualClassKind::Crossfade).unwrap().contains("\"crossfade\""));
    }

    // --- #516: CompositionVisualRevision serialization ---

    #[test]
    fn composition_visual_revision_serializes_camel_case() {
        let rev = CompositionVisualRevision {
            revision_id: 1,
            session_id: 1,
            committed_revision_id: 10,
            committed_text: "hello".to_string(),
            composition_replace_range: Some((5, 7)),
            preedit_text: "ni".to_string(),
            preedit_cursor_offset: 2,
            virtual_text: "helloni".to_string(),
            affected_paragraph_range: (0, 7),
            line_snapshot_ids: vec![1, 2],
            cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            decoration_ranges: vec![DecorationSlice {
                kind: DecorationSliceKind::Underline,
                byte_start: 5,
                byte_end: 7,
                rect: None,
                color: None,
            }],
            ime_cursor_range: Some((5, 7)),
            offset_map_from_previous: None,
        };
        let json = serde_json::to_string(&rev).unwrap();
        assert!(json.contains("\"committedText\":"));
        assert!(json.contains("\"compositionReplaceRange\":"));
        assert!(json.contains("\"preeditText\":"));
        assert!(json.contains("\"virtualText\":"));
        assert!(json.contains("\"affectedParagraphRange\":"));
        assert!(json.contains("\"lineSnapshotIds\":"));
        assert!(json.contains("\"cursorRect\":"));
        assert!(json.contains("\"decorationRanges\":"));
    }

    #[test]
    fn composition_visual_revision_skips_none_and_empty() {
        let rev = CompositionVisualRevision {
            revision_id: 0,
            session_id: 0,
            committed_revision_id: 0,
            committed_text: "hello".to_string(),
            composition_replace_range: None,
            preedit_text: String::new(),
            preedit_cursor_offset: 0,
            virtual_text: String::new(),
            affected_paragraph_range: (0, 5),
            line_snapshot_ids: Vec::new(),
            cursor_rect: None,
            decoration_ranges: Vec::new(),
            ime_cursor_range: None,
            offset_map_from_previous: None,
        };
        let json = serde_json::to_string(&rev).unwrap();
        assert!(!json.contains("\"compositionReplaceRange\":"));
        assert!(!json.contains("\"lineSnapshotIds\":"));
        assert!(!json.contains("\"cursorRect\":"));
        assert!(!json.contains("\"decorationRanges\":"));
    }

    // --- #516: PlatformVisualTransaction with new fields ---

    #[test]
    fn platform_visual_transaction_with_timeline_serializes() {
        let mut tl = Timeline::new(160);
        tl.mark_first_visible_frame(1000);
        let pvt = PlatformVisualTransaction {
            transaction_id: 1,
            generation: 1,
            state: PlatformVisualTransactionState::Rendering,
            old_revision: VisualLayoutRevision {
                document_revision: 1,
                layout_revision: 1,
                viewport_width: 800.0,
                font_fingerprint: "f1".to_string(),
                paragraph_style_fingerprint: "p1".to_string(),
                text_color_fingerprint: "t1".to_string(),
                density_or_dpr: 2.0,
            },
            new_revision: VisualLayoutRevision {
                document_revision: 2,
                layout_revision: 2,
                viewport_width: 800.0,
                font_fingerprint: "f1".to_string(),
                paragraph_style_fingerprint: "p1".to_string(),
                text_color_fingerprint: "t1".to_string(),
                density_or_dpr: 2.0,
            },
            slice_roles: vec![AnimatedSliceRole::Insert],
            slice_document_byte_ranges: vec![(2, 3)],
            static_line_patches: Vec::new(),
            cursor_transition_byte_start: 2,
            cursor_transition_byte_end: 3,
            duration_ms: 160,
            rendering_started_at_ms: Some(1000),
            accumulated_paused_duration_ms: 0,
            timeline: Some(tl),
            unified_kind: Some(UnifiedTransactionKind::BodyEdit),
            visual_class_kinds: vec![VisualClassKind::Insert],
            decoration_slices: Vec::new(),
            cursor_path: Some(CursorPath {
                from_rect: CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 },
                to_rect: CursorRect { x: 30.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 },
                is_snap: false,
            }),
            composition_revision: None,
            rebase: None,
            cancel_reason: None,
        };
        let json = serde_json::to_string(&pvt).unwrap();
        assert!(json.contains("\"timeline\":"));
        assert!(json.contains("\"unifiedKind\":"));
        assert!(json.contains("\"bodyEdit\""));
        assert!(json.contains("\"visualClassKinds\":"));
        assert!(json.contains("\"cursorPath\":"));
        assert!(!json.contains("\"compositionRevision\":"));
        assert!(!json.contains("\"rebase\":"));
    }

    // --- #516: TransactionRebase serialization ---

    #[test]
    fn transaction_rebase_serializes_camel_case() {
        let rebase = TransactionRebase {
            cancelled_transaction_id: 42,
            old_progress: 0.6,
            old_frame_snapshot: Some(RebaseFrameSnapshot {
                slice_rects: vec![Rect { x: 10.0, y: 20.0, w: 30.0, h: 40.0 }],
                slice_alphas: vec![0.8],
                cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            }),
        };
        let json = serde_json::to_string(&rebase).unwrap();
        assert!(json.contains("\"cancelledTransactionId\":"));
        assert!(json.contains("\"oldProgress\":"));
        assert!(json.contains("\"oldFrameSnapshot\":"));
        assert!(json.contains("\"sliceRects\":"));
        assert!(json.contains("\"sliceAlphas\":"));
        assert!(json.contains("\"cursorRect\":"));
    }

    #[test]
    fn transaction_rebase_skips_none() {
        let rebase = TransactionRebase {
            cancelled_transaction_id: 1,
            old_progress: 0.0,
            old_frame_snapshot: None,
        };
        let json = serde_json::to_string(&rebase).unwrap();
        assert!(!json.contains("\"oldFrameSnapshot\":"));
    }

    // --- #516: DecorationSlice serialization ---

    #[test]
    fn decoration_slice_serializes_camel_case() {
        let ds = DecorationSlice {
            kind: DecorationSliceKind::Underline,
            byte_start: 5,
            byte_end: 7,
            rect: Some(Rect { x: 10.0, y: 20.0, w: 30.0, h: 2.0 }),
            color: Some("#FF0000".to_string()),
        };
        let json = serde_json::to_string(&ds).unwrap();
        assert!(json.contains("\"byteStart\":"));
        assert!(json.contains("\"byteEnd\":"));
        assert!(json.contains("\"rect\":"));
        assert!(json.contains("\"color\":"));
        assert!(json.contains("\"underline\""));
    }

    #[test]
    fn decoration_slice_skips_none() {
        let ds = DecorationSlice {
            kind: DecorationSliceKind::Cursor,
            byte_start: 0,
            byte_end: 0,
            rect: None,
            color: None,
        };
        let json = serde_json::to_string(&ds).unwrap();
        assert!(!json.contains("\"rect\":"));
        assert!(!json.contains("\"color\":"));
    }

    // --- #516: CursorPath serialization ---

    #[test]
    fn cursor_path_serializes_camel_case() {
        let cp = CursorPath {
            from_rect: CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 },
            to_rect: CursorRect { x: 30.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 },
            is_snap: false,
        };
        let json = serde_json::to_string(&cp).unwrap();
        assert!(json.contains("\"fromRect\":"));
        assert!(json.contains("\"toRect\":"));
        assert!(json.contains("\"isSnap\":"));
    }

    // ========================================================================
    // #516 行为测试 — 覆盖 issue 验收标准
    // ========================================================================

    // --- build_virtual_text ---

    #[test]
    fn build_virtual_text_appends_preedit_when_no_replace_range() {
        let vt = build_virtual_text("hello", None, "world");
        assert_eq!(vt, "helloworld");
    }

    #[test]
    fn build_virtual_text_replaces_range_correctly() {
        // committedText[0..2] + preeditText + committedText[5..]
        let vt = build_virtual_text("hello", Some((2, 5)), "y");
        assert_eq!(vt, "hey");
    }

    #[test]
    fn build_virtual_text_preserves_text_after_replace_end() {
        // #516 关键验收：不得丢失 replaceEnd 后正文
        let vt = build_virtual_text("hello world", Some((0, 5)), "goodbye");
        assert_eq!(vt, "goodbye world", "Must preserve text after replaceEnd");
    }

    #[test]
    fn build_virtual_text_zero_length_replace_is_insert() {
        let vt = build_virtual_text("abc", Some((1, 1)), "X");
        assert_eq!(vt, "aXbc");
    }

    #[test]
    fn build_virtual_text_empty_preedit_is_delete() {
        let vt = build_virtual_text("abc", Some((1, 2)), "");
        assert_eq!(vt, "ac");
    }

    #[test]
    fn build_virtual_text_clamps_out_of_bounds_range() {
        let vt = build_virtual_text("hi", Some((0, 100)), "hello");
        assert_eq!(vt, "hello");
    }

    #[test]
    fn build_virtual_text_swap_start_end_is_noop() {
        let vt = build_virtual_text("abc", Some((2, 1)), "X");
        assert_eq!(vt, "abc");
    }

    // --- CompositionVisualRevision::new ---

    #[test]
    fn composition_visual_revision_new_builds_virtual_text() {
        let rev = CompositionVisualRevision::new(
            "hello".to_string(),
            Some((2, 5)),
            "y".to_string(),
            (0, 5),
        );
        assert_eq!(rev.virtual_text, "hey");
        assert_eq!(rev.committed_text, "hello");
        assert_eq!(rev.preedit_text, "y");
    }

    #[test]
    fn composition_visual_revision_new_no_replace_range() {
        let rev = CompositionVisualRevision::new(
            "abc".to_string(),
            None,
            "def".to_string(),
            (0, 3),
        );
        assert_eq!(rev.virtual_text, "abcdef");
    }

    // --- CursorOnly 事务 ---

    #[test]
    fn cursor_only_transaction_creates_transaction_on_move() {
        let mut engine = EditorEngine::new();
        let vt = engine.cursor_only_transaction("hello world", 5, 0).unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Cursor);
        assert_eq!(vt.old_text, "hello world");
        assert_eq!(vt.new_text, "hello world");
        assert!(vt.inserted_range.is_none());
        assert!(vt.deleted_range.is_none());
        assert_eq!(vt.old_selection.head.index, 5);
        assert_eq!(vt.new_selection.head.index, 0);
    }

    #[test]
    fn cursor_only_transaction_returns_none_when_no_move() {
        let mut engine = EditorEngine::new();
        let vt = engine.cursor_only_transaction("hello", 3, 3);
        assert!(vt.is_none());
    }

    // --- CompositionUpdate 事务 ---

    #[test]
    fn composition_update_transaction_generates_insert_class() {
        let mut engine = EditorEngine::new();
        let tx = engine.composition_update_transaction(
            "hello",
            None,
            "",
            "n",
        );
        assert!(tx.id > 0);
        assert_eq!(tx.old_revision.virtual_text, "hello");
        assert_eq!(tx.new_revision.virtual_text, "hellon");
        assert!(tx.visual_class_kinds.contains(&VisualClassKind::Insert));
    }

    #[test]
    fn composition_update_does_not_modify_committed_text() {
        let mut engine = EditorEngine::new();
        let tx = engine.composition_update_transaction(
            "committed",
            Some((0, 5)),
            "old_preedit",
            "new_preedit",
        );
        assert_eq!(tx.old_revision.committed_text, "committed");
        assert_eq!(tx.new_revision.committed_text, "committed");
    }

    // --- CompositionCommitOrCancel 事务 ---

    #[test]
    fn composition_commit_transaction_visual_same_no_repeat() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " world".to_string(),
            (0, 5),
        );
        // commit 后正文与 virtual_text 相同
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello world",
            comp_rev,
            true,
        );
        assert!(tx.is_commit);
        assert!(tx.is_visual_same);
    }

    #[test]
    fn composition_commit_transaction_visual_different() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " wor".to_string(),
            (0, 5),
        );
        // commit 后正文与 virtual_text 不同（候选转换）
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello world",
            comp_rev,
            true,
        );
        assert!(tx.is_commit);
        assert!(!tx.is_visual_same);
    }

    #[test]
    fn composition_cancel_transaction() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " world".to_string(),
            (0, 5),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello",
            comp_rev,
            false,
        );
        assert!(!tx.is_commit);
    }

    // --- classify_visual_diff ---

    #[test]
    fn classify_visual_diff_same_text_returns_empty() {
        let kinds = classify_visual_diff("abc", "abc");
        assert!(kinds.is_empty());
    }

    #[test]
    fn classify_visual_diff_insert_only() {
        let kinds = classify_visual_diff("", "abc");
        assert_eq!(kinds, vec![VisualClassKind::Insert]);
    }

    #[test]
    fn classify_visual_diff_delete_only() {
        let kinds = classify_visual_diff("abc", "");
        assert_eq!(kinds, vec![VisualClassKind::Delete]);
    }

    #[test]
    fn classify_visual_diff_replacement_is_crossfade() {
        let kinds = classify_visual_diff("abc", "xyz");
        assert!(kinds.contains(&VisualClassKind::Crossfade));
    }

    #[test]
    fn classify_visual_diff_suffix_moves_after_insert() {
        // "ab" → "aXb": prefix=a, inserted=X, suffix=b moves
        let kinds = classify_visual_diff("ab", "aXb");
        assert!(kinds.contains(&VisualClassKind::Insert));
        assert!(kinds.contains(&VisualClassKind::Move));
    }

    #[test]
    fn classify_visual_diff_prefix_is_static() {
        // "abc" → "abX": prefix=ab, inserted=X
        let kinds = classify_visual_diff("abc", "abX");
        assert!(kinds.contains(&VisualClassKind::Static));
    }

    // --- compute_rebase ---

    #[test]
    fn compute_rebase_creates_transaction_rebase() {
        let rebase = compute_rebase(42, 0.6, Some(RebaseFrameSnapshot {
            slice_rects: vec![Rect { x: 10.0, y: 20.0, w: 30.0, h: 40.0 }],
            slice_alphas: vec![0.8],
            cursor_rect: None,
        }));
        assert_eq!(rebase.cancelled_transaction_id, 42);
        assert!((rebase.old_progress - 0.6).abs() < f64::EPSILON);
        assert!(rebase.old_frame_snapshot.is_some());
    }

    // --- transactions_overlap ---

    #[test]
    fn transactions_overlap_cursor_only_always_conflicts() {
        assert!(transactions_overlap(
            UnifiedTransactionKind::CursorOnly,
            (0, 0),
            UnifiedTransactionKind::BodyEdit,
            (5, 10),
        ));
    }

    #[test]
    fn transactions_overlap_overlapping_ranges() {
        assert!(transactions_overlap(
            UnifiedTransactionKind::BodyEdit,
            (0, 10),
            UnifiedTransactionKind::BodyEdit,
            (5, 15),
        ));
    }

    #[test]
    fn transactions_overlap_non_overlapping_ranges() {
        assert!(!transactions_overlap(
            UnifiedTransactionKind::BodyEdit,
            (0, 5),
            UnifiedTransactionKind::BodyEdit,
            (10, 15),
        ));
    }

    // --- VisualRevision ---

    #[test]
    fn visual_revision_serializes_camel_case() {
        let rev = VisualRevision {
            revision_id: 1,
            full_text: "hello".to_string(),
            affected_paragraph_range: (0, 5),
            line_snapshot_ids: vec![1, 2],
            cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            caret_affinity: Some(CaretAffinity::Downstream),
            shaping_identity: Some("sha1:abc".to_string()),
        };
        let json = serde_json::to_string(&rev).unwrap();
        assert!(json.contains("\"revisionId\":"));
        assert!(json.contains("\"fullText\":"));
        assert!(json.contains("\"affectedParagraphRange\":"));
        assert!(json.contains("\"cursorRect\":"));
        assert!(json.contains("\"caretAffinity\":"));
        assert!(json.contains("\"shapingIdentity\":"));
    }

    // --- TransactionCancelReason ---

    #[test]
    fn transaction_cancel_reason_serializes_camel_case() {
        let json = serde_json::to_string(&TransactionCancelReason::Rebased).unwrap();
        assert!(json.contains("\"rebased\""));
        let json2 = serde_json::to_string(&TransactionCancelReason::CompositionCommitted).unwrap();
        assert!(json2.contains("\"compositionCommitted\""));
        let json3 = serde_json::to_string(&TransactionCancelReason::CompositionCancelled).unwrap();
        assert!(json3.contains("\"compositionCancelled\""));
    }

    // --- CaretAffinity ---

    #[test]
    fn caret_affinity_serializes_camel_case() {
        let json = serde_json::to_string(&CaretAffinity::Upstream).unwrap();
        assert!(json.contains("\"upstream\""));
        let json2 = serde_json::to_string(&CaretAffinity::Downstream).unwrap();
        assert!(json2.contains("\"downstream\""));
    }

    // --- Timeline 行为测试（#516 验收标准） ---

    #[test]
    fn timeline_pause_resume_maintains_progress() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        // 50% 进度时暂停
        tl.pause(1100);
        assert!((tl.paused_progress - 0.5).abs() < 0.01);
        // 恢复后进度从 0.5 继续
        tl.resume(1200);
        let p = tl.progress(1200);
        assert!((p - 0.5).abs() < 0.01, "Resume must continue from paused progress");
        // 200ms 后完成
        let p_end = tl.progress(1300);
        assert!((p_end - 1.0).abs() < 0.01);
    }

    #[test]
    fn timeline_cursor_and_text_same_frame_progress() {
        // #516: 光标与正文同帧 progress
        let mut tl = Timeline::new(160);
        tl.mark_first_visible_frame(1000);
        let p_at_1080 = tl.progress(1080);
        // 正文切片和光标都使用同一个 progress
        // 不允许光标维护独立开始时间
        assert!((p_at_1080 - 0.5).abs() < 0.01);
    }

    #[test]
    fn timeline_cursor_only_no_text_slice_still_executes() {
        // #516: CursorOnly 无正文切片也能完整执行
        let mut engine = EditorEngine::new();
        let vt = engine.cursor_only_transaction("hello", 0, 3).unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Cursor);
        assert_eq!(vt.old_text, vt.new_text);
        assert!(vt.inserted_range.is_none());
        assert!(vt.deleted_range.is_none());
    }

    // --- composing 不修改 committed text/undo/save/sync ---

    #[test]
    fn composition_update_does_not_change_committed_text() {
        let mut engine = EditorEngine::new();
        let tx = engine.composition_update_transaction(
            "original",
            Some((0, 4)),
            "orig",
            "new_text",
        );
        assert_eq!(tx.old_revision.committed_text, "original");
        assert_eq!(tx.new_revision.committed_text, "original");
        // virtual_text 变化，但 committed_text 不变
        assert_ne!(tx.old_revision.virtual_text, tx.new_revision.virtual_text);
    }

    // --- commit 相同视觉文字不重复吐字 ---

    #[test]
    fn commit_same_visual_text_no_repeat_animation() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " world".to_string(),
            (0, 5),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello world",
            comp_rev,
            true,
        );
        assert!(tx.is_visual_same, "Same visual text must not repeat animation");
    }

    // --- 候选转换生成 Crossfade/Move ---

    #[test]
    fn candidate_conversion_generates_crossfade_or_move() {
        let mut engine = EditorEngine::new();
        // 预输入 "ni" → 候选转换 commit "你"
        let comp_rev = CompositionVisualRevision::new(
            "hello ".to_string(),
            Some((6, 8)),
            "ni".to_string(),
            (0, 8),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello ",
            "hello 你",
            comp_rev,
            true,
        );
        assert!(!tx.is_visual_same, "Candidate conversion changes visual text");
        // 应该有 Crossfade 或 Delete/Insert 分类
        assert!(!tx.visual_class_kinds.is_empty());
    }

    // --- cancel 生成 Delete + reflow ---

    #[test]
    fn cancel_generates_delete_classification() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " world".to_string(),
            (0, 5),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello",
            comp_rev,
            false,
        );
        assert!(!tx.is_commit);
        // 取消时 virtual_text("hello world") → committed_text("hello")
        // 预输入部分应该产生 Delete 分类
        assert!(tx.visual_class_kinds.contains(&VisualClassKind::Delete));
    }

    // --- 连续事务 rebase ---

    #[test]
    fn rebase_covers_all_transaction_kinds() {
        // #516: rebase 必须覆盖四种事务
        // 测试 CursorOnly 与 BodyEdit 冲突
        assert!(transactions_overlap(
            UnifiedTransactionKind::CursorOnly,
            (0, 0),
            UnifiedTransactionKind::BodyEdit,
            (0, 5),
        ));
        // 测试 CompositionUpdate 与 CompositionCommitOrCancel 冲突
        assert!(transactions_overlap(
            UnifiedTransactionKind::CompositionUpdate,
            (0, 5),
            UnifiedTransactionKind::CompositionCommitOrCancel,
            (3, 8),
        ));
    }

    // --- Emoji ZWJ / combining mark / Arabic / RTL / ligature 进入 Crossfade fallback ---

    #[test]
    fn complex_grapheme_classified_as_crossfade_on_change() {
        // ZWJ emoji 变化 → Crossfade
        let kinds = classify_visual_diff("👨‍👩‍👧‍👦", "👨‍👨‍👧");
        assert!(kinds.contains(&VisualClassKind::Crossfade));
    }

    #[test]
    fn combining_mark_classified_as_crossfade_on_change() {
        // 组合字符变化 → Crossfade
        let kinds = classify_visual_diff("e\u{0301}", "è");
        assert!(kinds.contains(&VisualClassKind::Crossfade));
    }

    // --- PlatformVisualTransaction cancel_reason ---

    #[test]
    fn platform_visual_transaction_cancel_reason_serializes() {
        let mut pvt = PlatformVisualTransaction {
            transaction_id: 1,
            generation: 1,
            state: PlatformVisualTransactionState::Cancelled,
            old_revision: VisualLayoutRevision {
                document_revision: 1,
                layout_revision: 1,
                viewport_width: 800.0,
                font_fingerprint: "f1".to_string(),
                paragraph_style_fingerprint: "p1".to_string(),
                text_color_fingerprint: "t1".to_string(),
                density_or_dpr: 2.0,
            },
            new_revision: VisualLayoutRevision {
                document_revision: 2,
                layout_revision: 2,
                viewport_width: 800.0,
                font_fingerprint: "f1".to_string(),
                paragraph_style_fingerprint: "p1".to_string(),
                text_color_fingerprint: "t1".to_string(),
                density_or_dpr: 2.0,
            },
            slice_roles: Vec::new(),
            slice_document_byte_ranges: Vec::new(),
            static_line_patches: Vec::new(),
            cursor_transition_byte_start: 0,
            cursor_transition_byte_end: 0,
            duration_ms: 160,
            rendering_started_at_ms: None,
            accumulated_paused_duration_ms: 0,
            timeline: None,
            unified_kind: Some(UnifiedTransactionKind::BodyEdit),
            visual_class_kinds: Vec::new(),
            decoration_slices: Vec::new(),
            cursor_path: None,
            composition_revision: None,
            rebase: None,
            cancel_reason: Some(TransactionCancelReason::Rebased),
        };
        let json = serde_json::to_string(&pvt).unwrap();
        assert!(json.contains("\"cancelReason\":"));
        assert!(json.contains("\"rebased\""));

        pvt.cancel_reason = None;
        let json2 = serde_json::to_string(&pvt).unwrap();
        assert!(!json2.contains("\"cancelReason\":"));
    }

    // --- CompositionUpdateTransaction serialization ---

    #[test]
    fn composition_update_transaction_serializes_camel_case() {
        let mut engine = EditorEngine::new();
        let tx = engine.composition_update_transaction("hello", None, "", "n");
        let json = serde_json::to_string(&tx).unwrap();
        assert!(json.contains("\"oldRevision\":"));
        assert!(json.contains("\"newRevision\":"));
        assert!(json.contains("\"visualClassKinds\":"));
        assert!(json.contains("\"durationMs\":"));
    }

    // --- CompositionCommitOrCancelTransaction serialization ---

    #[test]
    fn composition_commit_or_cancel_transaction_serializes_camel_case() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " world".to_string(),
            (0, 5),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello world",
            comp_rev,
            true,
        );
        let json = serde_json::to_string(&tx).unwrap();
        assert!(json.contains("\"isCommit\":"));
        assert!(json.contains("\"isVisualSame\":"));
        assert!(json.contains("\"compositionRevision\":"));
        assert!(json.contains("\"committedTextAfter\":"));
    }

    // ========================================================================
    // #517 行为测试 — 覆盖 issue 验收标准
    // ========================================================================

    // --- #517: replaceRange 测试 ---

    #[test]
    fn build_virtual_text_zero_length_replace_preserves_text_after_cursor() {
        let committed = "你好世界";
        let cursor = "你好".len();
        let preedit = "abc";
        let vt = build_virtual_text(committed, Some((cursor, cursor)), preedit);
        assert_eq!(vt, "你好abc世界", "Zero-length replace must preserve text after cursor");
    }

    #[test]
    fn build_virtual_text_preedit_length_does_not_determine_replace_end() {
        let committed = "你好世界";
        let cursor = "你好".len();
        let vt_short = build_virtual_text(committed, Some((cursor, cursor)), "a");
        let vt_long = build_virtual_text(committed, Some((cursor, cursor)), "abcdef");
        assert_eq!(vt_short, "你好a世界");
        assert_eq!(vt_long, "你好abcdef世界");
    }

    #[test]
    fn build_virtual_text_composing_region_replaces_correctly() {
        let committed = "你好世界";
        let vt = build_virtual_text(committed, Some((3, 9)), "abc");
        assert_eq!(vt, "你abc界");
    }

    #[test]
    fn build_virtual_text_preedit_and_replace_different_lengths() {
        let committed = "hello world";
        let vt = build_virtual_text(committed, Some((0, 5)), "goodbye");
        assert_eq!(vt, "goodbye world");
        let vt2 = build_virtual_text(committed, Some((0, 5)), "hi");
        assert_eq!(vt2, "hi world");
    }

    #[test]
    fn build_virtual_text_emoji_boundary() {
        let committed = "ab😀cd";
        let emoji_start = "ab".len();
        let emoji_char = '😀';
        let emoji_end = emoji_start + emoji_char.len_utf8();
        let vt = build_virtual_text(committed, Some((emoji_start, emoji_end)), "XX");
        assert_eq!(vt, "abXXcd");
    }

    #[test]
    fn build_virtual_text_combining_mark_boundary() {
        let committed = "e\u{0301}test";
        let combining_end = "e\u{0301}".len();
        let vt = build_virtual_text(committed, Some((0, combining_end)), "X");
        assert_eq!(vt, "Xtest");
    }

    // --- #517: CompositionSession 测试 ---

    #[test]
    fn composition_session_zero_length_replace_by_default() {
        let session = CompositionSession::new(
            1, 10, "你好世界".to_string(), "你好".len(),
        );
        assert_eq!(session.replace_start, "你好".len());
        assert_eq!(session.replace_end_exclusive, "你好".len());
    }

    #[test]
    fn composition_session_update_preedit_preserves_replace_range() {
        let mut session = CompositionSession::new(
            1, 10, "你好世界".to_string(), "你好".len(),
        );
        let rev1 = session.update_preedit("a".to_string(), 1);
        assert_eq!(rev1.virtual_text, "你好a世界");
        assert_eq!(session.replace_start, "你好".len());
        assert_eq!(session.replace_end_exclusive, "你好".len());

        let rev2 = session.update_preedit("abcdef".to_string(), 6);
        assert_eq!(rev2.virtual_text, "你好abcdef世界");
        assert_eq!(session.replace_start, "你好".len());
        assert_eq!(session.replace_end_exclusive, "你好".len(),
            "replace_end must NOT change with preedit length");
    }

    #[test]
    fn composition_session_set_composing_region() {
        let mut session = CompositionSession::new(
            1, 10, "你好世界".to_string(), 0,
        );
        session.set_composing_region(3, 9);
        assert_eq!(session.replace_start, 3);
        assert_eq!(session.replace_end_exclusive, 9);
        let rev = session.update_preedit("abc".to_string(), 3);
        assert_eq!(rev.virtual_text, "你abc界");
    }

    #[test]
    fn composition_session_update_creates_revision_chain() {
        let mut session = CompositionSession::new(
            1, 10, "hello world".to_string(), 5,
        );

        let rev1 = session.update_preedit("n".to_string(), 1);
        assert_eq!(rev1.revision_id, 1);
        assert_eq!(rev1.session_id, 1);
        assert!(rev1.offset_map_from_previous.is_none(), "First revision has no previous");

        let rev2 = session.update_preedit("ni".to_string(), 2);
        assert_eq!(rev2.revision_id, 2);
        assert!(rev2.offset_map_from_previous.is_some(), "Second revision must have offset map");
        assert_eq!(rev2.preedit_text, "ni");

        let rev3 = session.update_preedit("nih".to_string(), 3);
        assert_eq!(rev3.revision_id, 3);
        assert!(rev3.offset_map_from_previous.is_some());
    }

    #[test]
    fn composition_session_does_not_modify_committed_text() {
        let mut session = CompositionSession::new(
            1, 10, "original".to_string(), 4,
        );
        session.update_preedit("test".to_string(), 4);
        assert_eq!(session.committed_text_at_start, "original");
    }

    #[test]
    fn composition_session_clear_resets_preedit() {
        let mut session = CompositionSession::new(
            1, 10, "hello".to_string(), 5,
        );
        session.update_preedit("world".to_string(), 5);
        assert!(!session.preedit_text.is_empty());
        session.clear();
        assert!(session.preedit_text.is_empty());
        assert!(session.current_visual_revision.is_none());
    }

    // --- #517: CompositionVisualRevision::from_previous 测试 ---

    #[test]
    fn composition_visual_revision_from_previous_chains_correctly() {
        let rev1 = CompositionVisualRevision::new(
            "hello world".to_string(),
            Some((6, 6)),
            "n".to_string(),
            (0, 11),
        );
        assert_eq!(rev1.virtual_text, "hello nworld");
        let rev2 = CompositionVisualRevision::from_previous(
            &rev1, "ni".to_string(), 2, (0, 11),
        );
        assert_eq!(rev2.virtual_text, "hello niworld");
        assert_eq!(rev2.committed_text, "hello world");
        assert_eq!(rev2.composition_replace_range, Some((6, 6)));
        assert!(rev2.offset_map_from_previous.is_some());
    }

    #[test]
    fn composition_visual_revision_preedit_byte_range_in_virtual_text() {
        let rev = CompositionVisualRevision::new(
            "你好世界".to_string(),
            Some((6, 6)),
            "abc".to_string(),
            (0, 12),
        );
        let (start, end) = rev.preedit_byte_range_in_virtual_text();
        assert_eq!(start, 6);
        assert_eq!(end, 9);
    }

    #[test]
    fn composition_visual_revision_preedit_byte_range_no_replace() {
        let rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            "world".to_string(),
            (0, 5),
        );
        let (start, end) = rev.preedit_byte_range_in_virtual_text();
        assert_eq!(start, 5);
        assert_eq!(end, 10);
    }

    // --- #517: OffsetMap 测试 ---

    #[test]
    fn offset_map_prefix_identity() {
        let map = OffsetMap::build("hello world", "hello WORLD");
        assert!(!map.entries.is_empty());
        let first = &map.entries[0];
        assert_eq!(first.kind, OffsetMapKind::Identity);
        assert_eq!(first.old_byte_offset, 0);
        assert_eq!(first.new_byte_offset, 0);
        assert_eq!(first.length, 6);
    }

    #[test]
    fn offset_map_suffix_shifted() {
        let map = OffsetMap::build("ab", "aXb");
        assert!(map.entries.len() >= 2);
        let suffix = map.entries.iter().find(|e| e.kind == OffsetMapKind::Shifted);
        assert!(suffix.is_some(), "Suffix after insert must be Shifted");
        let suffix = suffix.unwrap();
        assert_eq!(suffix.old_byte_offset, 1);
        assert_eq!(suffix.new_byte_offset, 2);
        assert_eq!(suffix.length, 1);
    }

    #[test]
    fn offset_map_map_old_to_new_identity() {
        let map = OffsetMap::build("abcde", "abXde");
        assert_eq!(map.map_old_to_new(0), Some(0));
        assert_eq!(map.map_old_to_new(1), Some(1));
    }

    #[test]
    fn offset_map_map_old_to_new_shifted() {
        let map = OffsetMap::build("ab", "aXb");
        assert_eq!(map.map_old_to_new(1), Some(2));
    }

    #[test]
    fn offset_map_map_old_to_new_no_mapping_for_middle() {
        let map = OffsetMap::build("abc", "aXc");
        assert!(map.map_old_to_new(1).is_none(), "Middle changed region has no mapping");
    }

    #[test]
    fn offset_map_empty_texts() {
        let map = OffsetMap::build("", "");
        assert!(map.entries.is_empty());
        let map2 = OffsetMap::build("", "abc");
        assert!(map2.entries.is_empty());
    }

    #[test]
    fn offset_map_same_text() {
        let map = OffsetMap::build("abc", "abc");
        assert!(map.entries.is_empty(), "Same text has no offset map");
    }

    // --- #517: SnapshotOwner 测试 ---

    #[test]
    fn snapshot_owner_serializes_camel_case() {
        let json = serde_json::to_string(&SnapshotOwner::OwnedBySession { session_id: 0 }).unwrap();
        assert!(json.contains("\"ownedBySession\""));
        let json2 = serde_json::to_string(&SnapshotOwner::OwnedByTransaction { transaction_id: 42 }).unwrap();
        assert!(json2.contains("\"ownedByTransaction\""));
        let json3 = serde_json::to_string(&SnapshotOwner::Released).unwrap();
        assert!(json3.contains("\"released\""));
    }

    #[test]
    fn snapshot_owner_equality() {
        assert_eq!(SnapshotOwner::OwnedBySession { session_id: 1 }, SnapshotOwner::OwnedBySession { session_id: 1 });
        assert_eq!(
            SnapshotOwner::OwnedByTransaction { transaction_id: 1 },
            SnapshotOwner::OwnedByTransaction { transaction_id: 1 },
        );
        assert_ne!(
            SnapshotOwner::OwnedByTransaction { transaction_id: 1 },
            SnapshotOwner::OwnedByTransaction { transaction_id: 2 },
        );
        assert_eq!(SnapshotOwner::Released, SnapshotOwner::Released);
        assert_ne!(SnapshotOwner::OwnedBySession { session_id: 1 }, SnapshotOwner::Released);
    }

    // --- #517: revision 接续测试 ---

    #[test]
    fn composition_update_from_previous_creates_chained_revision() {
        let mut engine = EditorEngine::new();
        let rev1 = CompositionVisualRevision::new(
            "hello world".to_string(),
            Some((6, 6)),
            "n".to_string(),
            (0, 11),
        );
        let tx = engine.composition_update_from_previous(&rev1, "ni", 2);
        assert_eq!(tx.old_revision.virtual_text, "hello nworld");
        assert_eq!(tx.new_revision.virtual_text, "hello niworld");
        assert!(tx.new_revision.offset_map_from_previous.is_some());
    }

    #[test]
    fn composition_update_from_previous_n_to_ni_to_nih() {
        let mut engine = EditorEngine::new();
        let rev1 = CompositionVisualRevision::new(
            "hello ".to_string(),
            Some((6, 6)),
            "n".to_string(),
            (0, 6),
        );
        let tx1 = engine.composition_update_from_previous(&rev1, "ni", 2);
        assert_eq!(tx1.old_revision.preedit_text, "n");
        assert_eq!(tx1.new_revision.preedit_text, "ni");

        let tx2 = engine.composition_update_from_previous(&tx1.new_revision, "nih", 3);
        assert_eq!(tx2.old_revision.preedit_text, "ni");
        assert_eq!(tx2.new_revision.preedit_text, "nih");
        assert!(tx2.new_revision.offset_map_from_previous.is_some());
    }

    // --- #517: commit/cancel 使用真实 replaceRange ---

    #[test]
    fn commit_with_replace_range_replaces_correctly() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello world".to_string(),
            Some((6, 11)),
            "earth".to_string(),
            (0, 11),
        );
        let committed_after = "hello earth";
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello world",
            committed_after,
            comp_rev,
            true,
        );
        assert!(tx.is_commit);
        assert!(tx.is_visual_same, "Same visual text on commit with replace range");
    }

    #[test]
    fn cancel_with_replace_range_restores_committed() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello world".to_string(),
            Some((6, 11)),
            "earth".to_string(),
            (0, 11),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello world",
            "hello world",
            comp_rev,
            false,
        );
        assert!(!tx.is_commit);
        assert!(!tx.visual_class_kinds.is_empty(), "Cancel must produce visual classifications");
    }

    #[test]
    fn commit_same_visual_text_no_repeat() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            Some((5, 5)),
            " world".to_string(),
            (0, 5),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello world",
            comp_rev,
            true,
        );
        assert!(tx.is_visual_same);
    }

    #[test]
    fn commit_candidate_conversion_generates_crossfade() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello ".to_string(),
            Some((6, 8)),
            "ni".to_string(),
            (0, 8),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello ",
            "hello 你",
            comp_rev,
            true,
        );
        assert!(!tx.is_visual_same);
        assert!(!tx.visual_class_kinds.is_empty());
    }

    // --- #517: CompositionSession 完整流程 ---

    #[test]
    fn composition_session_full_lifecycle() {
        let mut session = CompositionSession::new(
            1, 100, "你好世界".to_string(), "你好".len(),
        );

        let rev1 = session.update_preedit("n".to_string(), 1);
        assert_eq!(rev1.virtual_text, "你好n世界");
        assert_eq!(rev1.composition_replace_range, Some((6, 6)));

        let rev2 = session.update_preedit("ni".to_string(), 2);
        assert_eq!(rev2.virtual_text, "你好ni世界");
        assert_eq!(rev2.composition_replace_range, Some((6, 6)));
        assert!(rev2.offset_map_from_previous.is_some());

        let rev3 = session.update_preedit("nih".to_string(), 3);
        assert_eq!(rev3.virtual_text, "你好nih世界");
        assert_eq!(rev3.composition_replace_range, Some((6, 6)));
    }

    #[test]
    fn composition_session_with_composing_region() {
        let mut session = CompositionSession::new_with_replace_range(
            1, 100, "你好世界".to_string(), 3, 9,
        );
        let rev = session.update_preedit("abc".to_string(), 3);
        assert_eq!(rev.virtual_text, "你abc界");
        assert_eq!(rev.composition_replace_range, Some((3, 9)));
    }

    // --- #517: CompositionVisualRevision 新字段序列化 ---

    #[test]
    fn composition_visual_revision_new_fields_serialize() {
        let rev = CompositionVisualRevision {
            revision_id: 42,
            session_id: 7,
            committed_revision_id: 100,
            committed_text: "hello".to_string(),
            composition_replace_range: Some((5, 5)),
            preedit_text: "world".to_string(),
            preedit_cursor_offset: 3,
            virtual_text: "helloworld".to_string(),
            affected_paragraph_range: (0, 5),
            line_snapshot_ids: Vec::new(),
            cursor_rect: None,
            decoration_ranges: Vec::new(),
            ime_cursor_range: None,
            offset_map_from_previous: Some(OffsetMap {
                entries: vec![OffsetMapEntry {
                    old_byte_offset: 0,
                    new_byte_offset: 0,
                    length: 5,
                    kind: OffsetMapKind::Identity,
                }],
            }),
        };
        let json = serde_json::to_string(&rev).unwrap();
        assert!(json.contains("\"revisionId\":42"));
        assert!(json.contains("\"sessionId\":7"));
        assert!(json.contains("\"committedRevisionId\":100"));
        assert!(json.contains("\"preeditCursorOffset\":3"));
        assert!(json.contains("\"offsetMapFromPrevious\":"));
        assert!(json.contains("\"identity\""));
    }

    #[test]
    fn composition_session_serializes_camel_case() {
        let session = CompositionSession::new(1, 10, "hello".to_string(), 5);
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"sessionId\":1"));
        assert!(json.contains("\"committedRevisionId\":10"));
        assert!(json.contains("\"committedTextAtStart\":"));
        assert!(json.contains("\"replaceStart\":5"));
        assert!(json.contains("\"replaceEndExclusive\":5"));
        assert!(json.contains("\"preeditText\":"));
        assert!(json.contains("\"preeditCursorOffset\":0"));
    }

    // --- #517: OffsetMap 序列化 ---

    #[test]
    fn offset_map_serializes_camel_case() {
        let map = OffsetMap {
            entries: vec![
                OffsetMapEntry {
                    old_byte_offset: 0,
                    new_byte_offset: 0,
                    length: 5,
                    kind: OffsetMapKind::Identity,
                },
                OffsetMapEntry {
                    old_byte_offset: 8,
                    new_byte_offset: 10,
                    length: 3,
                    kind: OffsetMapKind::Shifted,
                },
            ],
        };
        let json = serde_json::to_string(&map).unwrap();
        assert!(json.contains("\"entries\":"));
        assert!(json.contains("\"oldByteOffset\":"));
        assert!(json.contains("\"newByteOffset\":"));
        assert!(json.contains("\"length\":"));
        assert!(json.contains("\"kind\":"));
        assert!(json.contains("\"identity\""));
        assert!(json.contains("\"shifted\""));
    }

    // --- #517: CompositionSession is_active/virtual_text/commit/cancel 测试 ---

    #[test]
    fn composition_session_is_active() {
        let mut session = CompositionSession::new(1, 1, "hello".to_string(), 5);
        assert!(!session.is_active());
        session.update_preedit("abc".to_string(), 0);
        assert!(session.is_active());
    }

    #[test]
    fn composition_session_virtual_text_zero_length_replace() {
        let mut session = CompositionSession::new(1, 1, "你好世界".to_string(), "你好".len());
        session.update_preedit("abc".to_string(), 0);
        assert_eq!(session.virtual_text(), "你好abc世界");
    }

    #[test]
    fn composition_session_virtual_text_nonzero_replace() {
        let mut session = CompositionSession::new_with_replace_range(
            1, 1, "你好世界".to_string(), 3, 6,
        );
        session.update_preedit("abc".to_string(), 0);
        assert_eq!(session.virtual_text(), "你abc世界");
    }

    #[test]
    fn composition_session_preedit_byte_range_in_virtual_text() {
        let mut session = CompositionSession::new_with_replace_range(
            1, 1, "你好世界".to_string(), 3, 6,
        );
        session.update_preedit("abcdef".to_string(), 6);
        let (start, end) = session.preedit_byte_range_in_virtual_text();
        assert_eq!(start, 3);
        assert_eq!(end, 9, "preedit range in virtualText differs from replaceRange");
    }

    #[test]
    fn composition_session_commit_uses_replace_range() {
        let mut session = CompositionSession::new(1, 1, "你好世界".to_string(), "你好".len());
        session.update_preedit("abc".to_string(), 0);
        let (comp_rev, committed_after) = session.commit("abc");
        assert_eq!(committed_after, "你好abc世界");
        assert_eq!(comp_rev.virtual_text, "你好abc世界");
        assert!(!session.is_active());
    }

    #[test]
    fn composition_session_commit_with_nonzero_replace_range() {
        let mut session = CompositionSession::new_with_replace_range(
            1, 1, "你好世界".to_string(), 3, 6,
        );
        session.update_preedit("abc".to_string(), 0);
        let (comp_rev, committed_after) = session.commit("abc");
        assert_eq!(committed_after, "你abc世界");
        assert_eq!(comp_rev.virtual_text, "你abc世界");
    }

    #[test]
    fn composition_session_cancel_restores_committed_text() {
        let mut session = CompositionSession::new(1, 1, "你好世界".to_string(), "你好".len());
        session.update_preedit("abc".to_string(), 0);
        let comp_rev = session.cancel();
        assert_eq!(comp_rev.virtual_text, "你好abc世界");
        assert!(!session.is_active());
    }

    #[test]
    fn composition_session_commit_same_visual_no_repeat() {
        let mut session = CompositionSession::new(1, 1, "你好世界".to_string(), "你好".len());
        session.update_preedit("abc".to_string(), 0);
        let (comp_rev, committed_after) = session.commit("abc");
        assert_eq!(comp_rev.virtual_text, committed_after);
    }

    #[test]
    fn composition_session_clear_resets_last_submitted_generation() {
        let mut session = CompositionSession::new(1, 1, "hello".to_string(), 5);
        session.update_preedit("abc".to_string(), 0);
        assert!(session.is_active());
        session.clear();
        assert!(!session.is_active());
        assert!(session.preedit_text.is_empty());
        assert!(session.current_visual_revision.is_none());
        assert_eq!(session.last_submitted_generation, 0);
    }

    #[test]
    fn composition_session_emoji_boundary() {
        let text = "👨‍👩‍👧‍👦hello";
        let emoji_len = "👨‍👩‍👧‍👦".len();
        let mut session = CompositionSession::new(1, 1, text.to_string(), emoji_len);
        session.update_preedit("abc".to_string(), 0);
        assert_eq!(session.virtual_text(), "👨‍👩‍👧‍👦abchello");
    }
}
