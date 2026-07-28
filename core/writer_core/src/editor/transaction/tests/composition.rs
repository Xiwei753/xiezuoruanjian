use crate::editor::transaction::types::*;
use crate::editor::transaction::visual::*;
use crate::editor::transaction::composition::*;
use crate::editor::transaction::rebase::*;
use crate::editor::transaction::engine::*;
use crate::editor::strong_types::{Utf8ByteOffset, Utf8ByteRange, EditorRevision, EditorSessionId, EditorSessionGeneration};

# [allow(deprecated)]

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

        #[test]
        fn composition_visual_revision_serializes_camel_case() {
            let rev = CompositionVisualRevision {
                revision_id: 1,
                session_id: EditorSessionId::new(1),
                committed_revision_id: EditorRevision::new(10),
                committed_text: "hello".to_string(),
                composition_replace_range: Utf8ByteRange::from_values(5, 7),
                preedit_text: "ni".to_string(),
                preedit_cursor_offset: Utf8ByteOffset::unchecked(2),
                virtual_text: "helloni".to_string(),
                affected_paragraph_range: Utf8ByteRange::from_values(0, 7).unwrap(),
                line_snapshot_ids: vec![1, 2],
                cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
                decoration_ranges: vec![DecorationSlice {
                    kind: DecorationSliceKind::Underline,
                    byte_start: Utf8ByteOffset::unchecked(5),
                    byte_end: Utf8ByteOffset::unchecked(7),
                    rect: None,
                    color: None,
                }],
                ime_cursor_range: Utf8ByteRange::from_values(5, 7),
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
                session_id: EditorSessionId::new(0),
                committed_revision_id: EditorRevision::new(0),
                committed_text: "hello".to_string(),
                composition_replace_range: None,
                preedit_text: String::new(),
                preedit_cursor_offset: Utf8ByteOffset::unchecked(0),
                virtual_text: String::new(),
                affected_paragraph_range: Utf8ByteRange::from_values(0, 5).unwrap(),
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

        #[test]
        fn composition_visual_revision_new_builds_virtual_text() {
            let rev = CompositionVisualRevision::new(
                "hello".to_string(),
                Utf8ByteRange::from_values(2, 5),
                "y".to_string(),
                Utf8ByteRange::from_values(0, 5).unwrap(),
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
                Utf8ByteRange::from_values(0, 3).unwrap(),
            );
            assert_eq!(rev.virtual_text, "abcdef");
        }

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
            let tx = engine.composition_update_transaction("committed", Some((0, 5)),
                "old_preedit",
                "new_preedit",
            );
            assert_eq!(tx.old_revision.committed_text, "committed");
            assert_eq!(tx.new_revision.committed_text, "committed");
        }

        #[test]
        fn composition_commit_transaction_visual_same_no_repeat() {
            let mut engine = EditorEngine::new();
            let comp_rev = CompositionVisualRevision::new(
                "hello".to_string(),
                None,
                " world".to_string(),
                Utf8ByteRange::from_values(0, 5).unwrap(),
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
                Utf8ByteRange::from_values(0, 5).unwrap(),
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
                Utf8ByteRange::from_values(0, 5).unwrap(),
            );
            let tx = engine.composition_commit_or_cancel_transaction(
                "hello",
                "hello",
                comp_rev,
                false,
            );
            assert!(!tx.is_commit);
        }

        #[test]
        fn composition_update_does_not_change_committed_text() {
            let mut engine = EditorEngine::new();
            let tx = engine.composition_update_transaction("original", Some((0, 4)),
                "orig",
                "new_text",
            );
            assert_eq!(tx.old_revision.committed_text, "original");
            assert_eq!(tx.new_revision.committed_text, "original");
            // virtual_text 变化，但 committed_text 不变
            assert_ne!(tx.old_revision.virtual_text, tx.new_revision.virtual_text);
        }

        #[test]
        fn commit_same_visual_text_no_repeat_animation() {
            let mut engine = EditorEngine::new();
            let comp_rev = CompositionVisualRevision::new(
                "hello".to_string(),
                None,
                " world".to_string(),
                Utf8ByteRange::from_values(0, 5).unwrap(),
            );
            let tx = engine.composition_commit_or_cancel_transaction(
                "hello",
                "hello world",
                comp_rev,
                true,
            );
            assert!(tx.is_visual_same, "Same visual text must not repeat animation");
        }

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

        #[test]
        fn composition_commit_or_cancel_transaction_serializes_camel_case() {
            let mut engine = EditorEngine::new();
            let comp_rev = CompositionVisualRevision::new(
                "hello".to_string(),
                None,
                " world".to_string(),
                Utf8ByteRange::from_values(0, 5).unwrap(),
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

        #[test]
        fn composition_session_zero_length_replace_by_default() {
            let session = CompositionSession::new(
                1, 10, "你好世界".to_string(), "你好".len(),
            );
            assert_eq!(session.replace_start, Utf8ByteOffset::unchecked("你好".len()));
            assert_eq!(session.replace_end_exclusive, Utf8ByteOffset::unchecked("你好".len()));
        }

        #[test]
        fn composition_session_update_preedit_preserves_replace_range() {
            let mut session = CompositionSession::new(
                1, 10, "你好世界".to_string(), "你好".len(),
            );
            let rev1 = session.update_preedit("a".to_string(), 1);
            assert_eq!(rev1.virtual_text, "你好a世界");
            assert_eq!(session.replace_start, Utf8ByteOffset::unchecked("你好".len()));
            assert_eq!(session.replace_end_exclusive, Utf8ByteOffset::unchecked("你好".len()));

            let rev2 = session.update_preedit("abcdef".to_string(), 6);
            assert_eq!(rev2.virtual_text, "你好abcdef世界");
            assert_eq!(session.replace_start, Utf8ByteOffset::unchecked("你好".len()));
            assert_eq!(session.replace_end_exclusive, Utf8ByteOffset::unchecked("你好".len()),
                "replace_end must NOT change with preedit length");
        }

        #[test]
        fn composition_session_set_composing_region() {
            let mut session = CompositionSession::new(
                1, 10, "你好世界".to_string(), 0,
            );
            session.set_composing_region(3, 9);
            assert_eq!(session.replace_start, Utf8ByteOffset::unchecked(3));
            assert_eq!(session.replace_end_exclusive, Utf8ByteOffset::unchecked(9));
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
            assert_eq!(rev1.session_id, EditorSessionId::new(1));
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

        #[test]
        fn composition_visual_revision_from_previous_chains_correctly() {
            let rev1 = CompositionVisualRevision::new(
                "hello world".to_string(),
                Utf8ByteRange::from_values(6, 6),
                "n".to_string(),
                Utf8ByteRange::from_values(0, 11).unwrap(),
            );
            assert_eq!(rev1.virtual_text, "hello nworld");
            let rev2 = CompositionVisualRevision::from_previous(
                &rev1, "ni".to_string(), 2, Utf8ByteRange::from_values(0, 11).unwrap(),
            );
            assert_eq!(rev2.virtual_text, "hello niworld");
            assert_eq!(rev2.committed_text, "hello world");
            assert_eq!(rev2.composition_replace_range, Utf8ByteRange::from_values(6, 6));
            assert!(rev2.offset_map_from_previous.is_some());
        }

        #[test]
        fn composition_visual_revision_preedit_byte_range_in_virtual_text() {
            let rev = CompositionVisualRevision::new(
                "你好世界".to_string(),
                Utf8ByteRange::from_values(6, 6),
                "abc".to_string(),
                Utf8ByteRange::from_values(0, 12).unwrap(),
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
                Utf8ByteRange::from_values(0, 5).unwrap(),
            );
            let (start, end) = rev.preedit_byte_range_in_virtual_text();
            assert_eq!(start, 5);
            assert_eq!(end, 10);
        }

        #[test]
        fn offset_map_prefix_identity() {
            let map = OffsetMap::build("hello world", "hello WORLD");
            assert!(!map.entries.is_empty());
            let first = &map.entries[0];
            assert_eq!(first.kind, OffsetMapKind::Identity);
            assert_eq!(first.old_byte_offset, Utf8ByteOffset::unchecked(0));
            assert_eq!(first.new_byte_offset, Utf8ByteOffset::unchecked(0));
            assert_eq!(first.length, 6);
        }

        #[test]
        fn offset_map_suffix_shifted() {
            let map = OffsetMap::build("ab", "aXb");
            assert!(map.entries.len() >= 2);
            let suffix = map.entries.iter().find(|e| e.kind == OffsetMapKind::Shifted);
            assert!(suffix.is_some(), "Suffix after insert must be Shifted");
            let suffix = suffix.unwrap();
            assert_eq!(suffix.old_byte_offset, Utf8ByteOffset::unchecked(1));
            assert_eq!(suffix.new_byte_offset, Utf8ByteOffset::unchecked(2));
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

        #[test]
        fn composition_update_from_previous_creates_chained_revision() {
            let mut engine = EditorEngine::new();
            let rev1 = CompositionVisualRevision::new(
                "hello world".to_string(),
                Utf8ByteRange::from_values(6, 6),
                "n".to_string(),
                Utf8ByteRange::from_values(0, 11).unwrap(),
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
                Utf8ByteRange::from_values(6, 6),
                "n".to_string(),
                Utf8ByteRange::from_values(0, 6).unwrap(),
            );
            let tx1 = engine.composition_update_from_previous(&rev1, "ni", 2);
            assert_eq!(tx1.old_revision.preedit_text, "n");
            assert_eq!(tx1.new_revision.preedit_text, "ni");

            let tx2 = engine.composition_update_from_previous(&tx1.new_revision, "nih", 3);
            assert_eq!(tx2.old_revision.preedit_text, "ni");
            assert_eq!(tx2.new_revision.preedit_text, "nih");
            assert!(tx2.new_revision.offset_map_from_previous.is_some());
        }

        #[test]
        fn commit_with_replace_range_replaces_correctly() {
            let mut engine = EditorEngine::new();
            let comp_rev = CompositionVisualRevision::new(
                "hello world".to_string(),
                Utf8ByteRange::from_values(6, 11),
                "earth".to_string(),
                Utf8ByteRange::from_values(0, 11).unwrap(),
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
                Utf8ByteRange::from_values(6, 11),
                "earth".to_string(),
                Utf8ByteRange::from_values(0, 11).unwrap(),
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
                Utf8ByteRange::from_values(5, 5),
                " world".to_string(),
                Utf8ByteRange::from_values(0, 5).unwrap(),
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
                Utf8ByteRange::from_values(6, 8),
                "ni".to_string(),
                Utf8ByteRange::from_values(0, 8).unwrap(),
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

        #[test]
        fn composition_session_full_lifecycle() {
            let mut session = CompositionSession::new(
                1, 100, "你好世界".to_string(), "你好".len(),
            );

            let rev1 = session.update_preedit("n".to_string(), 1);
            assert_eq!(rev1.virtual_text, "你好n世界");
            assert_eq!(rev1.composition_replace_range, Utf8ByteRange::from_values(6, 6));

            let rev2 = session.update_preedit("ni".to_string(), 2);
            assert_eq!(rev2.virtual_text, "你好ni世界");
            assert_eq!(rev2.composition_replace_range, Utf8ByteRange::from_values(6, 6));
            assert!(rev2.offset_map_from_previous.is_some());

            let rev3 = session.update_preedit("nih".to_string(), 3);
            assert_eq!(rev3.virtual_text, "你好nih世界");
            assert_eq!(rev3.composition_replace_range, Utf8ByteRange::from_values(6, 6));
        }

        #[test]
        fn composition_session_with_composing_region() {
            let mut session = CompositionSession::new_with_replace_range(
                1, 100, "你好世界".to_string(), 3, 9,
            );
            let rev = session.update_preedit("abc".to_string(), 3);
            assert_eq!(rev.virtual_text, "你abc界");
            assert_eq!(rev.composition_replace_range, Utf8ByteRange::from_values(3, 9));
        }

        #[test]
        fn composition_visual_revision_new_fields_serialize() {
            let rev = CompositionVisualRevision {
                revision_id: 42,
                session_id: EditorSessionId::new(7),
                committed_revision_id: EditorRevision::new(100),
                committed_text: "hello".to_string(),
                composition_replace_range: Utf8ByteRange::from_values(5, 5),
                preedit_text: "world".to_string(),
                preedit_cursor_offset: Utf8ByteOffset::unchecked(3),
                virtual_text: "helloworld".to_string(),
                affected_paragraph_range: Utf8ByteRange::from_values(0, 5).unwrap(),
                line_snapshot_ids: Vec::new(),
                cursor_rect: None,
                decoration_ranges: Vec::new(),
                ime_cursor_range: None,
                offset_map_from_previous: Some(OffsetMap {
                    entries: vec![OffsetMapEntry {
                        old_byte_offset: Utf8ByteOffset::unchecked(0),
                        new_byte_offset: Utf8ByteOffset::unchecked(0),
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

        #[test]
        fn offset_map_serializes_camel_case() {
            let map = OffsetMap {
                entries: vec![
                    OffsetMapEntry {
                        old_byte_offset: Utf8ByteOffset::unchecked(0),
                        new_byte_offset: Utf8ByteOffset::unchecked(0),
                        length: 5,
                        kind: OffsetMapKind::Identity,
                    },
                    OffsetMapEntry {
                        old_byte_offset: Utf8ByteOffset::unchecked(8),
                        new_byte_offset: Utf8ByteOffset::unchecked(10),
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
            assert_eq!(session.last_submitted_generation, EditorSessionGeneration::new(0));
        }

        #[test]
        fn composition_session_emoji_boundary() {
            let text = "👨‍👩‍👧‍👦hello";
            let emoji_len = "👨‍👩‍👧‍👦".len();
            let mut session = CompositionSession::new(1, 1, text.to_string(), emoji_len);
            session.update_preedit("abc".to_string(), 0);
            assert_eq!(session.virtual_text(), "👨‍👩‍👧‍👦abchello");
        }
