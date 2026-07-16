use writer_core::editor::{
    AnimationMode, CursorRect, EditorAnimationKind, EditorEngine, EditorSelection,
    EditorTransactionCause, GlyphRect, VisualCoordinateMode,
};

#[test]
fn unified_visual_transaction_replaces_legacy_insert_and_cursor_events() {
    let mut engine = EditorEngine::with_animation_limits(8, 120);
    let tx = engine.create_transaction(
        "ab",
        "abc",
        EditorSelection::collapsed("ab", 2),
        EditorSelection::collapsed("abc", 3),
        EditorTransactionCause::Typing,
    );

    assert!(tx.should_animate);
    let visual = engine.visual_transaction(&tx).unwrap();
    assert_eq!(visual.kind, EditorAnimationKind::Insert);
    assert_eq!(visual.inserted_range, Some((2, 3)));
    assert_eq!(visual.deleted_range, None);
    assert_eq!(visual.duration_ms, 120);
    assert_eq!(visual.coordinate_mode, VisualCoordinateMode::Baseline);
    assert!(visual.old_cursor_rect.is_none());
    assert!(visual.new_cursor_rect.is_none());

    let cursor = engine.cursor_only_transaction("abc", 2, 3).unwrap();
    assert_eq!(cursor.kind, EditorAnimationKind::Cursor);
    assert_eq!(cursor.inserted_range, None);
    assert_eq!(cursor.deleted_range, None);
    assert_eq!(cursor.duration_ms, 120);
}

#[test]
fn unified_visual_transaction_uses_utf8_byte_ranges() {
    let mut engine = EditorEngine::with_animation_limits(8, 120);
    let insert_tx = engine.create_transaction(
        "你好",
        "你好世",
        EditorSelection::collapsed("你好", "你好".len()),
        EditorSelection::collapsed("你好世", "你好世".len()),
        EditorTransactionCause::Typing,
    );
    let insert = engine.visual_transaction(&insert_tx).unwrap();
    assert_eq!(
        insert.inserted_range,
        Some(("你好".len(), "你好世".len()))
    );
    assert_eq!(insert.hidden_visual_ranges.len(), 1);
    assert_eq!(
        insert.hidden_visual_ranges[0].range_start,
        "你好".len()
    );
    assert_eq!(
        insert.hidden_visual_ranges[0].range_end,
        "你好世".len()
    );

    let delete_tx = engine.create_transaction(
        "你好世",
        "你好",
        EditorSelection::collapsed("你好世", "你好世".len()),
        EditorSelection::collapsed("你好", "你好".len()),
        EditorTransactionCause::Delete,
    );
    let delete = engine.visual_transaction(&delete_tx).unwrap();
    assert_eq!(delete.kind, EditorAnimationKind::Delete);
    assert_eq!(
        delete.deleted_range,
        Some(("你好".len(), "你好世".len()))
    );
    assert_eq!(delete.inserted_range, None);
}

#[test]
fn unified_visual_transaction_propagates_animation_duration() {
    let mut engine = EditorEngine::with_animation_limits(8, 120);
    let first_tx = engine.create_transaction(
        "a",
        "ab",
        EditorSelection::collapsed("a", 1),
        EditorSelection::collapsed("ab", 2),
        EditorTransactionCause::Typing,
    );
    assert_eq!(
        engine.visual_transaction(&first_tx).unwrap().duration_ms,
        120
    );

    engine.set_animation_duration_ms(500);
    let second_tx = engine.create_transaction(
        "ab",
        "abc",
        EditorSelection::collapsed("ab", 2),
        EditorSelection::collapsed("abc", 3),
        EditorTransactionCause::Typing,
    );
    assert_eq!(
        engine.visual_transaction(&second_tx).unwrap().duration_ms,
        500
    );
    assert_eq!(
        engine
            .cursor_only_transaction("abc", 2, 3)
            .unwrap()
            .duration_ms,
        500
    );
}

#[test]
fn unified_visual_transaction_skips_load_and_ime_preedit() {
    let mut engine = EditorEngine::new();
    let load = engine.create_transaction(
        "",
        "loaded",
        EditorSelection::collapsed("", 0),
        EditorSelection::collapsed("loaded", 6),
        EditorTransactionCause::Load,
    );
    assert!(!load.should_animate);
    assert!(engine.visual_transaction(&load).is_none());

    let preedit = engine.create_transaction(
        "ni",
        "nihao",
        EditorSelection::collapsed("ni", 2),
        EditorSelection::collapsed("nihao", 5),
        EditorTransactionCause::ImeComposition,
    );
    assert!(!preedit.should_animate);
    assert!(engine.visual_transaction(&preedit).is_none());
}

#[test]
fn unified_visual_transaction_preserves_layout_serde_contract() {
    let mut engine = EditorEngine::with_animation_limits(8, 160);
    let tx = engine.create_transaction(
        "a",
        "ab",
        EditorSelection::collapsed("a", 1),
        EditorSelection::collapsed("ab", 2),
        EditorTransactionCause::Typing,
    );
    let mut visual = engine.visual_transaction(&tx).unwrap();
    let empty_json = serde_json::to_value(&visual).unwrap();
    assert!(empty_json.get("insertGlyphRects").is_none());
    assert!(empty_json.get("deletedGlyphRects").is_none());
    assert!(empty_json.get("oldCursorRect").is_none());
    assert!(empty_json.get("newCursorRect").is_none());

    visual.insert_glyph_rects = Some(vec![GlyphRect {
        x: 10.0,
        y: 5.0,
        w: 8.0,
        h: 16.0,
        char_: "b".to_string(),
        baseline_y: 18.0,
        byte_start: 1,
        byte_end: 2,
    }]);
    visual.old_cursor_rect = Some(CursorRect {
        x: 10.0,
        top: 5.0,
        bottom: 21.0,
        baseline_y: 18.0,
    });
    visual.new_cursor_rect = Some(CursorRect {
        x: 18.0,
        top: 5.0,
        bottom: 21.0,
        baseline_y: 18.0,
    });

    let populated_json = serde_json::to_value(&visual).unwrap();
    assert_eq!(populated_json["insertGlyphRects"][0]["char"], "b");
    assert_eq!(
        populated_json["insertGlyphRects"][0]["baselineY"].as_f64(),
        Some(18.0)
    );
    assert!(populated_json.get("oldCursorRect").is_some());
    assert!(populated_json.get("newCursorRect").is_some());
}

#[test]
fn unified_visual_transaction_complex_grapheme_uses_cluster_mode() {
    let mut engine = EditorEngine::with_animation_limits(8, 120);
    let tx = engine.create_transaction(
        "ab",
        "ab😀",
        EditorSelection::collapsed("ab", 2),
        EditorSelection::collapsed("ab😀", "ab😀".len()),
        EditorTransactionCause::Typing,
    );
    let visual = engine.visual_transaction(&tx).unwrap();
    assert_eq!(visual.kind, EditorAnimationKind::Insert);
    assert_eq!(visual.animation_mode, AnimationMode::ClusterAnimation);
    let clusters = visual.cluster_rects.unwrap();
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0].text, "😀");
    assert!(clusters[0].is_complex);
}

#[test]
fn replacement_with_multiple_changes_has_no_single_visual_transaction() {
    let mut engine = EditorEngine::with_animation_limits(8, 120);
    let tx = engine.create_transaction(
        "alpha beta",
        "alpha gamma",
        EditorSelection::collapsed("alpha beta", "alpha beta".len()),
        EditorSelection::collapsed("alpha gamma", "alpha gamma".len()),
        EditorTransactionCause::Typing,
    );
    assert_eq!(tx.changes.len(), 2);
    assert!(engine.visual_transaction(&tx).is_none());
}
