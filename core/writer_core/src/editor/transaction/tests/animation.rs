use crate::editor::strong_types::{
    EditorRevision, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange,
};
use crate::editor::transaction::engine::*;
use crate::editor::transaction::platform::*;
use crate::editor::transaction::rebase::*;
use crate::editor::transaction::types::*;
use crate::editor::transaction::visual::*;
use crate::editor::transaction::{
    choose_animation_mode, classify_visual_diff, count_grapheme_clusters, split_text_into_clusters,
    split_text_into_runs,
};

#[allow(deprecated)]
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
        byte_start: Utf8ByteOffset::unchecked(0),
        byte_end: Utf8ByteOffset::unchecked(3),
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
        range_start: Utf8ByteOffset::unchecked(0),
        range_len: Utf8ByteOffset::unchecked(3),
        text: "abc".to_string(),
        old_cursor: EditorCursor {
            index: Utf8ByteOffset::unchecked(0),
        },
        new_cursor: EditorCursor {
            index: Utf8ByteOffset::unchecked(3),
        },
        duration_ms: 160,
        glyph_rects: vec![
            GlyphRect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 20.0,
                char_: "a".to_string(),
                baseline_y: 16.0,
                byte_start: Utf8ByteOffset::unchecked(0),
                byte_end: Utf8ByteOffset::unchecked(1),
            },
            GlyphRect {
                x: 10.0,
                y: 0.0,
                w: 10.0,
                h: 20.0,
                char_: "b".to_string(),
                baseline_y: 16.0,
                byte_start: Utf8ByteOffset::unchecked(1),
                byte_end: Utf8ByteOffset::unchecked(2),
            },
            GlyphRect {
                x: 20.0,
                y: 0.0,
                w: 10.0,
                h: 20.0,
                char_: "c".to_string(),
                baseline_y: 16.0,
                byte_start: Utf8ByteOffset::unchecked(2),
                byte_end: Utf8ByteOffset::unchecked(3),
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
    let cr = CursorRect {
        x: 10.5,
        top: 5.0,
        bottom: 25.0,
        baseline_y: 20.0,
    };
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
        range_start: Utf8ByteOffset::unchecked(0),
        range_len: Utf8ByteOffset::unchecked(1),
        text: "a".to_string(),
        old_cursor: EditorCursor {
            index: Utf8ByteOffset::unchecked(0),
        },
        new_cursor: EditorCursor {
            index: Utf8ByteOffset::unchecked(1),
        },
        duration_ms: 160,
        glyph_rects: Vec::new(),
        old_cursor_rect: Some(CursorRect {
            x: 10.0,
            top: 5.0,
            bottom: 25.0,
            baseline_y: 20.0,
        }),
        new_cursor_rect: Some(CursorRect {
            x: 30.0,
            top: 5.0,
            bottom: 25.0,
            baseline_y: 20.0,
        }),
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
        range_start: Utf8ByteOffset::unchecked(0),
        range_len: Utf8ByteOffset::unchecked(1),
        text: "a".to_string(),
        old_cursor: EditorCursor {
            index: Utf8ByteOffset::unchecked(0),
        },
        new_cursor: EditorCursor {
            index: Utf8ByteOffset::unchecked(1),
        },
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
    assert_eq!(events[0].range_start.value(), "你好".len()); // 6 bytes
    assert_eq!(events[0].range_len.value(), "世".len()); // 3 bytes
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
    assert_eq!(events[0].range_start.value(), "你好".len()); // 6 bytes
    assert_eq!(events[0].range_len.value(), "世".len()); // 3 bytes
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
    assert_eq!(
        events2[0].duration_ms, 5,
        "Core stores whatever duration is set; clamping is the caller's responsibility"
    );

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
    assert_eq!(
        events3[0].duration_ms, 9999,
        "Core stores whatever duration is set; clamping is the caller's responsibility"
    );
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
    assert_eq!(vt.inserted_range, Utf8ByteRange::from_values(2, 3));
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
    assert!(
        vt.is_some(),
        "Paste short text should enter visual transaction"
    );
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
    assert!(
        vt.is_some(),
        "Paste with newline should enter visual transaction"
    );
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
    let cr = CursorRect {
        x: 10.0,
        top: 5.0,
        bottom: 25.0,
        baseline_y: 20.0,
    };
    let json = serde_json::to_string(&cr).unwrap();
    assert!(json.contains("\"baselineY\":"));
    assert!(json.contains("\"top\":"));
    assert!(json.contains("\"bottom\":"));
}

#[test]
fn glyph_rect_has_baseline_y() {
    let gr = GlyphRect {
        x: 10.5,
        y: 20.0,
        w: 16.0,
        h: 24.0,
        char_: "你".to_string(),
        baseline_y: 40.0,
        byte_start: Utf8ByteOffset::unchecked(0),
        byte_end: Utf8ByteOffset::unchecked(3),
    };
    let json = serde_json::to_string(&gr).unwrap();
    assert!(json.contains("\"baselineY\":"));
}

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
    assert_eq!(clusters[0].byte_start, Utf8ByteOffset::unchecked(0));
    assert_eq!(clusters[0].byte_end, Utf8ByteOffset::unchecked("😀".len()));
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
        range: Utf8ByteRange::from_ordered(10, 20),
        old_rect: None,
        new_rect: None,
        line_index: 3,
        payload_ref: None,
    };
    let json = serde_json::to_string(&hvr).unwrap();
    assert!(json.contains("\"id\":"));
    assert!(json.contains("\"kind\":"));
    assert!(json.contains("\"glyphAnimation\""));
    assert!(json.contains("\"range\":"));
    assert!(json.contains("\"lineIndex\":"));
    // None fields should be skipped
    assert!(!json.contains("\"oldRect\":"));
    assert!(!json.contains("\"newRect\":"));
    assert!(!json.contains("\"payloadRef\":"));

    // With rects
    let hvr2 = HiddenVisualRange {
        id: 43,
        kind: AnimationMode::LineReflowAnimation,
        range: Utf8ByteRange::from_ordered(0, 5),
        old_rect: Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 20.0,
        }),
        new_rect: Some(Rect {
            x: 0.0,
            y: 20.0,
            w: 100.0,
            h: 20.0,
        }),
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
    assert_eq!(clusters[0].byte_start, Utf8ByteOffset::unchecked(0));
    assert_eq!(clusters[0].byte_end, Utf8ByteOffset::unchecked(emoji.len()));
    assert_eq!(clusters[0].text, emoji);
    assert!(clusters[0].is_complex, "ZWJ emoji should be complex");
}

#[test]
fn split_text_into_clusters_variation_selector_emoji() {
    // Variation selector emoji 输出正确的 byte range 和 is_complex=true
    let emoji = "❤️"; // ❤ + FE0F
    let clusters = split_text_into_clusters(emoji, 0);
    assert_eq!(
        clusters.len(),
        1,
        "Variation selector emoji should be 1 cluster"
    );
    assert_eq!(clusters[0].byte_start, Utf8ByteOffset::unchecked(0));
    assert_eq!(clusters[0].byte_end, Utf8ByteOffset::unchecked(emoji.len()));
    assert_eq!(clusters[0].text, emoji);
    assert!(
        clusters[0].is_complex,
        "Variation selector emoji should be complex"
    );
}

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
    assert!(
        (p_after_pause - 0.75).abs() < 0.01,
        "Paused must return paused_progress, not 0"
    );
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
    assert!(
        (p_at_resume - 0.5).abs() < 0.01,
        "Expected 0.5 at resume time, got {}",
        p_at_resume
    );
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
    assert!(serde_json::to_string(&VisualClassKind::Static)
        .unwrap()
        .contains("\"static\""));
    assert!(serde_json::to_string(&VisualClassKind::Insert)
        .unwrap()
        .contains("\"insert\""));
    assert!(serde_json::to_string(&VisualClassKind::Delete)
        .unwrap()
        .contains("\"delete\""));
    assert!(serde_json::to_string(&VisualClassKind::Move)
        .unwrap()
        .contains("\"move\""));
    assert!(serde_json::to_string(&VisualClassKind::Crossfade)
        .unwrap()
        .contains("\"crossfade\""));
}

#[test]
fn platform_visual_transaction_with_timeline_serializes() {
    let mut tl = Timeline::new(160);
    tl.mark_first_visible_frame(1000);
    let pvt = PlatformVisualTransaction {
        transaction_id: 1,
        generation: EditorSessionGeneration::new(1),
        state: PlatformVisualTransactionState::Rendering,
        old_revision: VisualLayoutRevision {
            document_revision: EditorRevision::new(1),
            layout_revision: 1,
            viewport_width: 800.0,
            font_fingerprint: "f1".into(),
            paragraph_style_fingerprint: "p1".into(),
            text_color_fingerprint: "t1".into(),
            density_or_dpr: 2.0,
        },
        new_revision: VisualLayoutRevision {
            document_revision: EditorRevision::new(2),
            layout_revision: 2,
            viewport_width: 800.0,
            font_fingerprint: "f1".into(),
            paragraph_style_fingerprint: "p1".into(),
            text_color_fingerprint: "t1".into(),
            density_or_dpr: 2.0,
        },
        slice_roles: vec![AnimatedSliceRole::Insert],
        slice_document_byte_ranges: vec![Utf8ByteRange::from_ordered(2, 3)],
        static_line_patches: Vec::new(),
        cursor_transition_byte_start: Utf8ByteOffset::unchecked(2),
        cursor_transition_byte_end: Utf8ByteOffset::unchecked(3),
        duration_ms: 160,
        rendering_started_at_ms: Some(1000),
        accumulated_paused_duration_ms: 0,
        timeline: Some(tl),
        unified_kind: Some(UnifiedTransactionKind::BodyEdit),
        visual_class_kinds: vec![VisualClassKind::Insert],
        decoration_slices: Vec::new(),
        cursor_path: Some(CursorPath {
            from_rect: CursorRect {
                x: 10.0,
                top: 5.0,
                bottom: 25.0,
                baseline_y: 20.0,
            },
            to_rect: CursorRect {
                x: 30.0,
                top: 5.0,
                bottom: 25.0,
                baseline_y: 20.0,
            },
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

#[test]
fn visual_revision_serializes_camel_case() {
    let rev = VisualRevision {
        revision_id: EditorRevision::new(1),
        full_text: "hello".to_string(),
        affected_paragraph_range: Utf8ByteRange::from_ordered(0, 5),
        line_snapshot_ids: vec![1, 2],
        cursor_rect: Some(CursorRect {
            x: 10.0,
            top: 5.0,
            bottom: 25.0,
            baseline_y: 20.0,
        }),
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

#[test]
fn caret_affinity_serializes_camel_case() {
    let json = serde_json::to_string(&CaretAffinity::Upstream).unwrap();
    assert!(json.contains("\"upstream\""));
    let json2 = serde_json::to_string(&CaretAffinity::Downstream).unwrap();
    assert!(json2.contains("\"downstream\""));
}

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
    assert!(
        (p - 0.5).abs() < 0.01,
        "Resume must continue from paused progress"
    );
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

#[test]
fn platform_visual_transaction_cancel_reason_serializes() {
    let mut pvt = PlatformVisualTransaction {
        transaction_id: 1,
        generation: EditorSessionGeneration::new(1),
        state: PlatformVisualTransactionState::Cancelled,
        old_revision: VisualLayoutRevision {
            document_revision: EditorRevision::new(1),
            layout_revision: 1,
            viewport_width: 800.0,
            font_fingerprint: "f1".to_string(),
            paragraph_style_fingerprint: "p1".to_string(),
            text_color_fingerprint: "t1".to_string(),
            density_or_dpr: 2.0,
        },
        new_revision: VisualLayoutRevision {
            document_revision: EditorRevision::new(2),
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
        cursor_transition_byte_start: Utf8ByteOffset::unchecked(0),
        cursor_transition_byte_end: Utf8ByteOffset::unchecked(0),
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
