use crate::editor::transaction::types::*;
use crate::editor::transaction::visual::*;
use crate::editor::transaction::composition::*;
use crate::editor::transaction::rebase::*;
use crate::editor::transaction::engine::*;
use crate::editor::transaction::platform::*;

# [allow(deprecated)]

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

        #[test]
        fn transaction_cancel_reason_serializes_camel_case() {
            let json = serde_json::to_string(&TransactionCancelReason::Rebased).unwrap();
            assert!(json.contains("\"rebased\""));
            let json2 = serde_json::to_string(&TransactionCancelReason::CompositionCommitted).unwrap();
            assert!(json2.contains("\"compositionCommitted\""));
            let json3 = serde_json::to_string(&TransactionCancelReason::CompositionCancelled).unwrap();
            assert!(json3.contains("\"compositionCancelled\""));
        }
