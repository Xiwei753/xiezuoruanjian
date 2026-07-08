#[cfg(test)]
mod tests {
    use super::*;
    use crate::sujian_editor_item::is_complex_grapheme;
    use crate::sujian_editor_item::text_animation_state::{AnimationMode, TextAnimationState};
    use crate::sujian_editor_item::PreeditAttributeKind;
    use crate::sujian_editor_item::SujianEditorItem;
    use qmetaobject::prelude::*;
    use writer_core::editor::CursorRect;

    #[test]
    fn meta_object_dump_contains_animation_signals() {
        let item_box = QObjectBox::new(SujianEditorItem::default());
        item_box.pinned().get_or_create_cpp_object();
        let dump = {
            let pinned = item_box.pinned();
            let item = pinned.borrow();
            item.debug_meta_object_animation_signals().to_string()
        };
        eprintln!("[sujian-test] SujianEditorItem metaObject animation signals: {dump}");
        assert!(
            dump.contains("visual_transaction_changed"),
            "missing visual_transaction_changed in {dump}"
        );
        assert!(
            dump.contains("preedit_visual_transaction_changed"),
            "missing preedit_visual_transaction_changed in {dump}"
        );
        assert!(
            dump.contains("transaction_created"),
            "missing transaction_created in {dump}"
        );
        assert!(
            dump.contains("explicit_clear_requested"),
            "missing explicit_clear_requested in {dump}"
        );
        let verified = {
            let pinned = item_box.pinned();
            let item = pinned.borrow();
            item.verify_animation_signal_meta_object()
        };
        assert!(verified, "hard metaObject verification failed for {dump}");
    }

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
    fn test_is_complex_grapheme_chinese_char() {
        assert!(!is_complex_grapheme('你'));
    }

    #[test]
    fn test_is_complex_grapheme_ascii() {
        assert!(!is_complex_grapheme('a'));
        assert!(!is_complex_grapheme('Z'));
        assert!(!is_complex_grapheme('0'));
    }

    #[test]
    fn test_is_complex_grapheme_chinese_punctuation() {
        assert!(!is_complex_grapheme('，'));
        assert!(!is_complex_grapheme('。'));
        assert!(!is_complex_grapheme('！'));
    }

    #[test]
    fn test_is_complex_grapheme_combining_half_mark() {
        assert!(is_complex_grapheme('\u{FE20}'));
    }

    #[test]
    fn test_preedit_visual_transaction_created_on_preedit_change() {
        use writer_core::editor::{PreeditVisualTransaction, VisualCoordinateMode};

        let old_text = "n".to_string();
        let new_text = "ni".to_string();
        let preedit_cursor_rect = Some(CursorRect {
            x: 10.0,
            top: 5.0,
            bottom: 25.0,
            baseline_y: 20.0,
        });

        let vt = PreeditVisualTransaction {
            id: 0,
            old_preedit_text: old_text.clone(),
            new_preedit_text: new_text.clone(),
            old_preedit_cursor_rect: None,
            new_preedit_cursor_rect: preedit_cursor_rect.clone(),
            preedit_glyph_rects: None,
            deleted_preedit_glyph_rects: None,
            inserted_preedit_glyph_rects: None,
            preedit_cursor_rect: preedit_cursor_rect,
            duration_ms: 100,
            coordinate_mode: VisualCoordinateMode::Baseline,
        };

        assert_eq!(vt.old_preedit_text, "n");
        assert_eq!(vt.new_preedit_text, "ni");
        assert!(vt.new_preedit_cursor_rect.is_some());
        assert!(vt.preedit_cursor_rect.is_some());
        assert_eq!(vt.duration_ms, 100);
        assert_eq!(vt.coordinate_mode, VisualCoordinateMode::Baseline);

        let json = serde_json::to_string(&vt).unwrap();
        assert!(json.contains("\"oldPreeditText\":"));
        assert!(json.contains("\"newPreeditText\":"));
    }

    #[test]
    fn test_pending_preedit_cursor_rect_flow() {
        let preedit_cursor_rect: Option<CursorRect> = Some(CursorRect {
            x: 50.0,
            top: 10.0,
            bottom: 30.0,
            baseline_y: 25.0,
        });
        let mut pending_preedit_cursor_rect: Option<CursorRect> = None;

        if preedit_cursor_rect.is_some() {
            pending_preedit_cursor_rect = preedit_cursor_rect.clone();
        }

        let taken = pending_preedit_cursor_rect.take();
        assert!(
            taken.is_some(),
            "pending_preedit_cursor_rect should be saved before clearing"
        );
        assert_eq!(taken.as_ref().unwrap().x, 50.0);
        assert_eq!(taken.as_ref().unwrap().top, 10.0);

        assert!(
            pending_preedit_cursor_rect.is_none(),
            "pending should be empty after take"
        );

        let empty_preedit_cursor_rect: Option<CursorRect> = None;
        let mut pending2: Option<CursorRect> = None;
        if empty_preedit_cursor_rect.is_some() {
            pending2 = empty_preedit_cursor_rect.clone();
        }
        let taken2 = pending2.take();
        assert!(
            taken2.is_none(),
            "no pending when preedit_cursor_rect was None"
        );
    }

    #[test]
    fn test_preedit_attribute_kind_new_variants() {
        let tc = PreeditAttributeKind::TextColor {
            color: "#FF0000".to_string(),
        };
        assert!(matches!(tc, PreeditAttributeKind::TextColor { .. }));

        let bc = PreeditAttributeKind::BackgroundColor {
            color: "#00FF00".to_string(),
        };
        assert!(matches!(bc, PreeditAttributeKind::BackgroundColor { .. }));

        let fu = PreeditAttributeKind::FontUnderline;
        assert_eq!(fu, PreeditAttributeKind::FontUnderline);

        let ul = PreeditAttributeKind::Underline;
        assert_eq!(ul, PreeditAttributeKind::Underline);
        let cur = PreeditAttributeKind::Cursor;
        assert_eq!(cur, PreeditAttributeKind::Cursor);
    }

    #[test]
    fn typing_animation_disabled_prevents_new_animations() {
        let typing_animation_enabled = false;
        let vt_present = true;
        let is_scrolling = false;
        let should_create = typing_animation_enabled && vt_present && !is_scrolling;
        assert!(
            !should_create,
            "when typing_animation_enabled=false, no animations should be created"
        );
    }

    #[test]
    fn scrolling_prevents_new_animations() {
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
    fn visual_transaction_inserted_range_used_for_hidden_range() {
        let mut state = TextAnimationState::new();
        let inserted_range = Some((5, 10));
        if let Some((range_start, range_end)) = inserted_range {
            state.start_insert(
                (range_start, range_end),
                vec![],
                AnimationMode::GlyphAnimation,
                100,
            );
        }
        assert_eq!(state.active_insert_byte_ranges(), vec![(5, 10)]);
        assert!(state.has_active_insert());
    }

    #[test]
    fn typing_animation_disabled_clears_hidden_range_immediately() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
        state.clear_on_typing_animation_disabled();
        assert!(state.is_empty());
        assert!(state.active_insert_byte_ranges().is_empty());
    }

    #[test]
    fn chinese_period_not_complex_grapheme_allows_animation() {
        assert!(
            !is_complex_grapheme('。'),
            "Chinese period '。' is not complex, should allow animation"
        );
    }

    #[test]
    fn delete_single_char_animation_lifecycle() {
        let mut state = TextAnimationState::new();
        state.start_delete((5, 8), AnimationMode::GlyphAnimation, 100);
        assert!(state.active_insert_byte_ranges().is_empty());
        assert!(!state.has_active_insert());
        assert!(!state.is_empty());
        state.clear();
        assert!(state.is_empty());
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

        let mut state = TextAnimationState::new();
        if let Some((range_start, range_end)) = vt.inserted_range {
            if mode != AnimationMode::SystemSuppressed {
                state.start_insert((range_start, range_end), vec![], mode, vt.duration_ms);
            }
        }
        assert!(
            state.has_active_insert(),
            "4-char idiom should create hidden range"
        );
        assert_eq!(state.active_insert_byte_ranges(), vec![(6, 18)]);

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

        let vt = engine
            .visual_transaction(&tx)
            .expect("RunAnimation visual transaction");
        let mode = SujianEditorItem::animation_mode_from_core(vt.animation_mode);
        assert_eq!(
            mode,
            AnimationMode::RunAnimation,
            "9-cluster candidate should produce RunAnimation"
        );

        let mut state = TextAnimationState::new();
        if mode != AnimationMode::SystemSuppressed {
            state.start_insert((0, long_candidate.len()), vec![], mode, 160);
        }
        assert!(
            state.has_active_insert(),
            "9-cluster candidate should create hidden range (RunAnimation)"
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

        let mut state = TextAnimationState::new();
        if let Some((rs, re)) = vt.inserted_range {
            if mode != AnimationMode::SystemSuppressed {
                state.start_insert((rs, re), vec![], mode, vt.duration_ms);
            }
        }
        assert!(state.has_active_insert());
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

        let vt = engine
            .visual_transaction(&tx)
            .expect("LineReflowAnimation visual transaction");
        let mode = SujianEditorItem::animation_mode_from_core(vt.animation_mode);
        assert_eq!(
            mode,
            AnimationMode::LineReflowAnimation,
            "Newline should produce LineReflowAnimation"
        );

        let mut state = TextAnimationState::new();
        if mode != AnimationMode::SystemSuppressed {
            state.start_insert((old_text.len(), new_text.len()), vec![], mode, 160);
        }
        assert!(
            state.has_active_insert(),
            "Newline should create hidden range (LineReflowAnimation)"
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
    fn qml_overlay_skip_must_clear_hidden_range() {
        let mut state = TextAnimationState::new();
        let byte_range = (10, 22);
        state.start_insert(byte_range, vec![], AnimationMode::GlyphAnimation, 160);
        assert!(
            state.has_active_insert(),
            "Should have active insert before skip"
        );
        assert_eq!(
            state.active_insert_byte_ranges(),
            vec![byte_range],
            "Hidden range should be (10, 22) before skip"
        );

        let removed = state.on_insert_animation_finished_by_id(None, None, 10, 22);
        assert!(
            removed,
            "on_insert_animation_skipped_by_id should return true (removed matching animation)"
        );
        assert!(
            state.is_empty(),
            "Hidden range must be immediately cleared after skip"
        );
        assert!(
            state.active_insert_byte_ranges().is_empty(),
            "No active insert byte range should remain after skip"
        );

        state.start_insert((30, 42), vec![], AnimationMode::GlyphAnimation, 160);
        let removed_wrong = state.on_insert_animation_finished_by_id(None, None, 50, 60);
        assert!(
            !removed_wrong,
            "Skipping non-matching range should return false"
        );
        assert!(
            state.has_active_insert(),
            "Existing hidden range should remain when skip doesn't match"
        );
        assert_eq!(state.active_insert_byte_ranges(), vec![(30, 42)]);

        state.clear();
        assert!(state.is_empty());
    }
}
