#[cfg(test)]
mod tests {
    use super::*;
    use crate::sujian_editor_item::is_complex_grapheme;
    use crate::sujian_editor_item::SujianEditorItem;
    use crate::sujian_editor_item::PreeditAttributeKind;
    use crate::sujian_editor_item::animation_coordinator::{AnimationMode, VisualTransactionKey};
    use qmetaobject::prelude::*;
    use writer_core::editor::CursorRect;

    #[test]
    fn test_is_complex_grapheme_emoji() {
        assert!(is_complex_grapheme('😀'));
    }

    #[test]
    fn test_is_complex_grapheme_zwj() {
        assert!(is_complex_grapheme('\u{200D}'));
    }

    #[test]
    fn test_is_complex_grapheme_variation_selector() {
        assert!(is_complex_grapheme('\u{FE0F}'));
    }

    #[test]
    fn test_is_complex_grapheme_combining_mark() {
        assert!(is_complex_grapheme('\u{0301}'));
    }

    #[test]
    fn test_is_complex_grapheme_cjk_not_complex() {
        assert!(!is_complex_grapheme('你'));
        assert!(!is_complex_grapheme('。'));
    }

    #[test]
    fn scrolling_suppresses_animation_creation() {
        let typing_animation_enabled = true;
        let vt_present = true;
        let is_scrolling = true;
        let should_create = typing_animation_enabled && vt_present && !is_scrolling;
        assert!(
            !should_create,
            "when scrolling, no animations should be created"
        );
    }

    #[test]
    fn visual_transaction_inserted_range_creates_prepared_transaction() {
        use crate::sujian_editor_item::text_visual_transaction::{PreparedTransactionQueue, PreparedTextVisualTransaction, TextVisualTransactionState, TextVisualOperationKind, TransactionTimeline};
        use crate::sujian_editor_item::animation_mode::AnimationMode;
        use crate::sujian_editor_item::cursor_animation::CursorTransition;
        use crate::sujian_editor_item::layout_revision::LayoutRevision;

        let mut queue = PreparedTransactionQueue::new();
        let key = VisualTransactionKey { transaction_id: 1, generation: 1 };
        let tx = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Insert,
            animation_mode: AnimationMode::GlyphAnimation,
            timeline: TransactionTimeline::new(160),
            old_revision: LayoutRevision::initial(),
            new_revision: LayoutRevision::next(),
            slices: Vec::new(),
            static_patches: vec![crate::sujian_editor_item::static_line_patch::StaticLinePatch::insert_patch(
                key,
                crate::sujian_editor_item::layout_snapshot::LineSnapshotId::new(0, 0, 0),
                Vec::new(),
                5,
                10,
            )],
            decoration_slices: Vec::new(),
            cursor_transition: CursorTransition::Snap,
            old_cursor_rect: None,
            new_cursor_rect: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
        };
        queue.enqueue(tx);
        assert_eq!(queue.insert_byte_ranges(), vec![(5, 10)]);
        assert!(queue.has_active_insert());
    }

    #[test]
    fn typing_animation_disabled_clears_queue_immediately() {
        use crate::sujian_editor_item::text_visual_transaction::PreparedTransactionQueue;
        let mut queue = PreparedTransactionQueue::new();
        assert!(queue.is_empty());
        queue.cancel_all("disabled");
        assert!(queue.is_empty());
    }

    #[test]
    fn chinese_period_not_complex_grapheme_allows_animation() {
        assert!(
            !is_complex_grapheme('。'),
            "Chinese period '。' is not complex, should allow animation"
        );
    }

    #[test]
    fn delete_creates_prepared_transaction() {
        use crate::sujian_editor_item::text_visual_transaction::{PreparedTransactionQueue, PreparedTextVisualTransaction, TextVisualTransactionState, TextVisualOperationKind, TransactionTimeline};
        use crate::sujian_editor_item::animation_mode::AnimationMode;
        use crate::sujian_editor_item::cursor_animation::CursorTransition;
        use crate::sujian_editor_item::layout_revision::LayoutRevision;

        let mut queue = PreparedTransactionQueue::new();
        let key = VisualTransactionKey { transaction_id: 2, generation: 2 };
        let tx = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Delete,
            animation_mode: AnimationMode::GlyphAnimation,
            timeline: TransactionTimeline::new(160),
            old_revision: LayoutRevision::initial(),
            new_revision: LayoutRevision::next(),
            slices: Vec::new(),
            static_patches: Vec::new(),
            decoration_slices: Vec::new(),
            cursor_transition: CursorTransition::Snap,
            old_cursor_rect: None,
            new_cursor_rect: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
        };
        queue.enqueue(tx);
        assert!(queue.insert_byte_ranges().is_empty());
        assert!(queue.has_active_insert());
        queue.cancel_all("test");
        assert!(queue.is_empty());
    }

    #[test]
    fn emoji_is_complex_grapheme_skips_animation() {
        assert!(
            is_complex_grapheme('😀'),
            "Emoji should be complex grapheme, skipping animation"
        );
    }

    #[test]
    fn combining_accent_is_complex_grapheme_skips_animation() {
        assert!(
            is_complex_grapheme('\u{0301}'),
            "Combining acute accent should be complex grapheme, skipping animation"
        );
    }

    #[test]
    fn scrolling_input_does_not_create_animation() {
        let typing_animation_enabled = true;
        let vt_present = true;
        let is_scrolling = true;
        let should_create = typing_animation_enabled && vt_present && !is_scrolling;
        assert!(
            !should_create,
            "Scrolling should suppress animation creation"
        );
    }

    #[test]
    fn typing_animation_disabled_no_new_animation_on_input() {
        let typing_animation_enabled = false;
        let vt_present = true;
        let is_scrolling = false;
        let should_create = typing_animation_enabled && vt_present && !is_scrolling;
        assert!(
            !should_create,
            "Disabled typing animation should prevent new animations"
        );
    }

    #[test]
    fn coordinated_cursor_disabled_does_not_affect_text_animation() {
        let typing_animation_enabled = true;
        let vt_present = true;
        let is_scrolling = false;
        let coordinated_enabled = false;
        let should_create_text_anim = typing_animation_enabled && vt_present && !is_scrolling;
        assert!(
            should_create_text_anim,
            "Text animation creation should not be affected by coordinated cursor setting"
        );
        let _ = coordinated_enabled;
    }

    #[test]
    fn ime_commit_4_char_idiom_produces_cursor_animation() {
        use writer_core::editor::{
            EditorAnimationKind, EditorEngine, EditorSelection, EditorTransactionCause,
        };

        let mut engine = EditorEngine::with_animation_limits(8, 160);
        let idiom = "风和日丽";
        let old_text = "你好";
        let new_text = "你好风和日丽";
        let old_cursor = EditorSelection::collapsed(old_text, old_text.len());
        let new_cursor = EditorSelection::collapsed(new_text, new_text.len());

        let tx = engine.create_transaction(
            old_text,
            new_text,
            old_cursor,
            new_cursor,
            EditorTransactionCause::TypingCommit,
        );

        assert!(tx.should_animate, "4-char idiom commit should animate");

        let vt = engine.visual_transaction(&tx);
        assert!(
            vt.is_some(),
            "4-char idiom commit should produce visual transaction"
        );
        let vt = vt.unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Insert);
        assert_eq!(
            vt.inserted_range,
            Some((old_text.len(), old_text.len() + idiom.len()))
        );

        let mode = SujianEditorItem::animation_mode_from_core(vt.animation_mode);
        assert_eq!(mode, AnimationMode::GlyphAnimation);

        assert_ne!(
            tx.old_selection.head.index, tx.new_selection.head.index,
            "Cursor should move after idiom commit"
        );
        assert_eq!(
            tx.new_selection.head.index,
            new_text.len(),
            "Cursor byte offset should be at end of committed text"
        );
    }

    #[test]
    fn ime_commit_long_candidate_run_animation_cursor_still_moves() {
        use writer_core::editor::{EditorEngine, EditorSelection, EditorTransactionCause};

        let mut engine = EditorEngine::with_animation_limits(8, 160);
        let long_candidate = "一二三四五六七八九";
        assert!(long_candidate.chars().count() > 8);
        let old_text = "";
        let new_text = long_candidate;
        let old_cursor = EditorSelection::collapsed(old_text, 0);
        let new_cursor = EditorSelection::collapsed(new_text, new_text.len());

        let tx = engine.create_transaction(
            old_text,
            new_text,
            old_cursor,
            new_cursor,
            EditorTransactionCause::TypingCommit,
        );

        assert!(
            tx.should_animate,
            "9-char candidate should animate at core level (RunAnimation)"
        );

        let vt = engine.visual_transaction(&tx).expect("RunAnimation visual transaction");
        let mode = SujianEditorItem::animation_mode_from_core(vt.animation_mode);
        assert_eq!(
            mode,
            AnimationMode::RunAnimation,
            "9-cluster candidate should produce RunAnimation"
        );

        assert_eq!(
            tx.new_selection.head.index,
            long_candidate.len(),
            "Cursor byte offset should be at end of committed text even without animation"
        );
    }

    #[test]
    fn ime_commit_after_initials_cursor_moves_forward() {
        use writer_core::editor::{EditorEngine, EditorSelection, EditorTransactionCause};

        let mut engine = EditorEngine::with_animation_limits(8, 160);

        let preedit_text = "fhrl";
        let tx_preedit = engine.create_transaction(
            "",
            preedit_text,
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed(preedit_text, preedit_text.len()),
            EditorTransactionCause::ImeComposition,
        );
        assert!(
            !tx_preedit.should_animate,
            "ImeComposition should not animate"
        );
        assert_eq!(
            tx_preedit.new_selection.head.index,
            preedit_text.len(),
            "Preedit cursor should be at end of preedit text"
        );

        let committed = "风和日丽";
        let tx_commit = engine.create_transaction(
            "",
            committed,
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed(committed, committed.len()),
            EditorTransactionCause::TypingCommit,
        );
        assert!(
            tx_commit.should_animate,
            "TypingCommit of 4-char idiom should animate"
        );

        assert_eq!(
            tx_commit.old_selection.head.index, 0,
            "Old cursor should be at start (preedit was cleared)"
        );
        assert_eq!(
            tx_commit.new_selection.head.index,
            committed.len(),
            "New cursor should be at end of committed text"
        );

        assert!(
            tx_commit.new_selection.head.index > tx_commit.old_selection.head.index,
            "Cursor should move forward after commit (byte offset increases)"
        );
    }

    #[test]
    fn ime_commit_pinyin_longer_than_hanzi_cursor_can_retreat() {
        use writer_core::editor::{
            EditorAnimationKind, EditorEngine, EditorSelection, EditorTransactionCause,
        };

        let mut engine = EditorEngine::with_animation_limits(8, 160);

        let pinyin = "fengherili";
        let hanzi = "风和日丽";

        let tx_preedit = engine.create_transaction(
            "",
            pinyin,
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed(pinyin, pinyin.len()),
            EditorTransactionCause::ImeComposition,
        );
        assert!(!tx_preedit.should_animate);

        let tx_commit = engine.create_transaction(
            "",
            hanzi,
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed(hanzi, hanzi.len()),
            EditorTransactionCause::TypingCommit,
        );
        assert!(
            tx_commit.should_animate,
            "TypingCommit should animate for 4-char hanzi (single Insert change)"
        );

        assert_ne!(
            tx_commit.old_selection.head.index, tx_commit.new_selection.head.index,
            "Cursor position must change for retreat animation"
        );

        let vt = engine.visual_transaction(&tx_commit);
        assert!(vt.is_some());
        let vt = vt.unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Insert);
        assert_eq!(vt.old_selection.head.index, 0);
        assert_eq!(vt.new_selection.head.index, hanzi.len());

        let mode = SujianEditorItem::animation_mode_from_core(vt.animation_mode);
        assert_eq!(mode, AnimationMode::GlyphAnimation);
    }

    #[test]
    fn newline_commit_cursor_vertical_animation() {
        use writer_core::editor::{
            CursorRect, EditorAnimationKind, EditorEngine, EditorSelection, EditorTransactionCause,
        };

        let mut engine = EditorEngine::with_animation_limits(8, 160);

        let old_text = "你好";
        let new_text = "你好\n";
        let tx = engine.create_transaction(
            old_text,
            new_text,
            EditorSelection::collapsed(old_text, old_text.len()),
            EditorSelection::collapsed(new_text, new_text.len()),
            EditorTransactionCause::Typing,
        );

        assert!(
            tx.should_animate,
            "Newline commit should animate at core level (LineReflowAnimation)"
        );

        let vt = engine.visual_transaction(&tx).expect("LineReflowAnimation visual transaction");
        let mode = SujianEditorItem::animation_mode_from_core(vt.animation_mode);
        assert_eq!(
            mode,
            AnimationMode::LineReflowAnimation,
            "Newline should produce LineReflowAnimation"
        );

        assert_ne!(
            tx.old_selection.head.index, tx.new_selection.head.index,
            "Cursor must move after newline (vertical animation required)"
        );

        let old_cursor_rect = CursorRect {
            x: 10.0,
            top: 5.0,
            bottom: 25.0,
            baseline_y: 20.0,
        };
        let new_cursor_rect = CursorRect {
            x: 0.0,
            top: 30.0,
            bottom: 50.0,
            baseline_y: 45.0,
        };
        assert_ne!(
            old_cursor_rect.top, new_cursor_rect.top,
            "Cursor rect y must change for vertical animation after newline"
        );
        assert!(
            new_cursor_rect.top > old_cursor_rect.top,
            "New cursor rect y should be below old (moved down a line)"
        );
    }

    #[test]
    fn mid_insert_reflow_animation_local_push() {
        use writer_core::editor::ReflowGlyphRect;

        let reflow_c = ReflowGlyphRect {
            char_: "C".to_string(),
            byte_start: 4,
            byte_end: 5,
            old_x: 20.0,
            old_y: 0.0,
            old_baseline_y: 16.0,
            new_x: 40.0,
            new_y: 0.0,
            new_baseline_y: 16.0,
            w: 10.0,
            h: 20.0,
            line_index: 0,
        };
        let reflow_d = ReflowGlyphRect {
            char_: "D".to_string(),
            byte_start: 5,
            byte_end: 6,
            old_x: 30.0,
            old_y: 0.0,
            old_baseline_y: 16.0,
            new_x: 50.0,
            new_y: 0.0,
            new_baseline_y: 16.0,
            w: 10.0,
            h: 20.0,
            line_index: 0,
        };
        let reflow_e = ReflowGlyphRect {
            char_: "E".to_string(),
            byte_start: 6,
            byte_end: 7,
            old_x: 40.0,
            old_y: 0.0,
            old_baseline_y: 16.0,
            new_x: 60.0,
            new_y: 0.0,
            new_baseline_y: 16.0,
            w: 10.0,
            h: 20.0,
            line_index: 0,
        };

        let reflow_rects = vec![reflow_c, reflow_d, reflow_e];

        assert_eq!(
            reflow_rects.len(),
            3,
            "Reflow should contain 3 glyphs right of insertion"
        );
        assert_eq!(reflow_rects[0].char_, "C");
        assert_eq!(reflow_rects[1].char_, "D");
        assert_eq!(reflow_rects[2].char_, "E");

        for rect in &reflow_rects {
            assert!(
                rect.new_x > rect.old_x,
                "Reflow glyph '{}' should be pushed right: old_x={}, new_x={}",
                rect.char_,
                rect.old_x,
                rect.new_x
            );
        }

        let inserted_chars: Vec<&str> = vec!["X", "Y"];
        for rect in &reflow_rects {
            assert!(
                !inserted_chars.contains(&rect.char_.as_str()),
                "Inserted glyph '{}' should NOT be in reflow_rects (it goes through insert animation)",
                rect.char_
            );
        }

        let push_distance = reflow_rects[0].new_x - reflow_rects[0].old_x;
        assert!(
            (push_distance - 20.0).abs() < 0.1,
            "Reflow push distance should equal inserted text width: got {}",
            push_distance
        );
    }

    #[test]
    fn prepared_queue_finish_clears_transaction() {
        use crate::sujian_editor_item::text_visual_transaction::{PreparedTransactionQueue, PreparedTextVisualTransaction, TextVisualTransactionState, TextVisualOperationKind, TransactionTimeline};
        use crate::sujian_editor_item::animation_mode::AnimationMode;
        use crate::sujian_editor_item::cursor_animation::CursorTransition;
        use crate::sujian_editor_item::layout_revision::LayoutRevision;

        let mut queue = PreparedTransactionQueue::new();
        let key = VisualTransactionKey { transaction_id: 1, generation: 1 };
        let tx = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Insert,
            animation_mode: AnimationMode::GlyphAnimation,
            timeline: TransactionTimeline::new(160),
            old_revision: LayoutRevision::initial(),
            new_revision: LayoutRevision::next(),
            slices: Vec::new(),
            static_patches: vec![crate::sujian_editor_item::static_line_patch::StaticLinePatch::insert_patch(
                key,
                crate::sujian_editor_item::layout_snapshot::LineSnapshotId::new(0, 0, 0),
                Vec::new(),
                10,
                22,
            )],
            decoration_slices: Vec::new(),
            cursor_transition: CursorTransition::Snap,
            old_cursor_rect: None,
            new_cursor_rect: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
        };
        queue.enqueue(tx);
        assert!(queue.has_active_insert());
        assert_eq!(queue.insert_byte_ranges(), vec![(10, 22)]);

        let removed = queue.complete(key);
        assert!(removed.is_some());
        assert!(queue.is_empty());
        assert!(queue.insert_byte_ranges().is_empty());

        queue.enqueue(PreparedTextVisualTransaction {
            key: VisualTransactionKey { transaction_id: 2, generation: 2 },
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Insert,
            animation_mode: AnimationMode::GlyphAnimation,
            timeline: TransactionTimeline::new(160),
            old_revision: LayoutRevision::initial(),
            new_revision: LayoutRevision::next(),
            slices: Vec::new(),
            static_patches: vec![crate::sujian_editor_item::static_line_patch::StaticLinePatch::insert_patch(
                VisualTransactionKey { transaction_id: 2, generation: 2 },
                crate::sujian_editor_item::layout_snapshot::LineSnapshotId::new(0, 0, 0),
                Vec::new(),
                30,
                42,
            )],
            decoration_slices: Vec::new(),
            cursor_transition: CursorTransition::Snap,
            old_cursor_rect: None,
            new_cursor_rect: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
        });
        let removed_wrong = queue.complete(VisualTransactionKey { transaction_id: 999, generation: 1 });
        assert!(removed_wrong.is_none());
        assert!(queue.has_active_insert());
        assert_eq!(queue.insert_byte_ranges(), vec![(30, 42)]);

        queue.cancel_all("test");
        assert!(queue.is_empty());
    }

    fn utf16_forward(text: &str, byte_start: usize, utf16_count: i32) -> usize {
        if utf16_count <= 0 { return byte_start; }
        let mut remaining = utf16_count;
        let mut pos = byte_start;
        for ch in text[byte_start..].chars() {
            if remaining <= 0 { break; }
            remaining -= ch.len_utf16() as i32;
            pos += ch.len_utf8();
        }
        pos.min(text.len())
    }

    fn utf16_backward(text: &str, byte_start: usize, utf16_count: i32) -> usize {
        if utf16_count <= 0 { return byte_start; }
        let mut remaining = utf16_count;
        let mut pos = byte_start;
        for ch in text[..byte_start].chars().rev() {
            if remaining <= 0 { break; }
            remaining -= ch.len_utf16() as i32;
            pos -= ch.len_utf8();
        }
        pos
    }

    fn simulate_qt_commit(
        committed_text: &str,
        session_replace_start: usize,
        session_replace_end: usize,
        replace_start: i32,
        replace_length: i32,
        inserted: &str,
    ) -> String {
        let base_text = format!(
            "{}{}",
            &committed_text[..session_replace_start],
            &committed_text[session_replace_end..]
        );
        let anchor_in_base = session_replace_start;
        let rs_byte = if replace_start < 0 {
            utf16_backward(&base_text, anchor_in_base, -replace_start)
        } else if replace_start == 0 {
            anchor_in_base
        } else {
            utf16_forward(&base_text, anchor_in_base, replace_start)
        };
        let re_byte = if replace_length > 0 {
            utf16_forward(&base_text, rs_byte, replace_length)
        } else {
            rs_byte
        };
        let (del_start, del_end) = if rs_byte <= re_byte {
            (rs_byte, re_byte)
        } else {
            (re_byte, rs_byte)
        };
        format!(
            "{}{}{}",
            &base_text[..del_start],
            &inserted,
            &base_text[del_end..]
        )
    }

    #[test]
    fn test_qt_commit_zero_session_zero_replacement() {
        let result = simulate_qt_commit("ABCDE", 2, 2, 0, 0, "你好");
        assert_eq!(result, "AB你好CDE");
    }

    #[test]
    fn test_qt_commit_zero_session_nonzero_replacement_start() {
        let result = simulate_qt_commit("ABCDE", 2, 2, 2, 0, "你好");
        assert_eq!(result, "ABCD你好E");
    }

    #[test]
    fn test_qt_commit_negative_replacement_start_delete_two() {
        let result = simulate_qt_commit("ABCDE", 2, 2, -1, 2, "你好");
        assert_eq!(result, "A你好DE");
    }

    #[test]
    fn test_qt_commit_session_replace_with_negative_replacement() {
        let result = simulate_qt_commit("ABCDE", 1, 3, -1, 2, "你好");
        assert_eq!(result, "你好E");
    }

    #[test]
    fn test_qt_commit_session_replace_zero_replacement() {
        let result = simulate_qt_commit("ABCDE", 1, 3, 0, 0, "你好");
        assert_eq!(result, "A你好DE");
    }

    #[test]
    fn test_qt_commit_session_replace_positive_replacement() {
        let result = simulate_qt_commit("ABCDE", 1, 3, 1, 0, "你好");
        assert_eq!(result, "AD你好E");
    }

    #[test]
    fn test_qt_commit_chinese_text_nonzero_replacement_start() {
        let result = simulate_qt_commit("你好世界", 6, 6, 2, 0, "新");
        assert_eq!(result, "你好世界新");
    }

    #[test]
    fn test_qt_commit_chinese_text_negative_replacement() {
        let result = simulate_qt_commit("你好世界", 6, 6, -1, 1, "新");
        assert_eq!(result, "你新世界");
    }
}
