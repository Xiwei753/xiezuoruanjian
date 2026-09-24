use super::super::coordinator::LinuxEditorAnimationCoordinator;
use super::*;
use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::animation::rebase::match_rebase_frames;
use crate::sujian_editor_item::animation::{
    PreparedTextVisualTransaction, PreparedVisualUnit, RebaseFrame, TextVisualOperationKind,
    TransactionTimeline, VisualUnitTiming,
};
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_snapshot::{LineSnapshotId, ShapingIdentity, SourceRect};
use crate::sujian_editor_item::render_plan::SelectionPreeditPlan;
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
use writer_core::editor::{OffsetMap, Utf8ByteOffset};

fn make_test_snapshot(
    virtual_text: &str,
    line_clusters: Vec<(usize, usize, f64, f64, ShapingIdentity)>,
) -> EditorLayoutSnapshot {
    use crate::editor::layout::{CaretAffinity, LayoutSnapshot, VisualLine};
    use crate::sujian_editor_item::layout_snapshot::{LineClusterSnapshot, PreparedLineSnapshot};
    let clusters: Vec<LineClusterSnapshot> = line_clusters
        .iter()
        .map(|(bs, be, x, _y, sid)| LineClusterSnapshot {
            byte_start: *bs,
            byte_end: *be,
            source_rect: SourceRect {
                x: *x,
                y: 0.0,
                w: (*be - *bs) as f64 * 10.0,
                h: 20.0,
            },
            shaping_identity: sid.clone(),
        })
        .collect();
    let line = PreparedLineSnapshot {
        id: LineSnapshotId::new(1, 0, 0),
        image: None,
        clusters,
        document_origin_y: 0.0,
        dpr: 1.0,
        byte_start: line_clusters.first().map(|c| c.0).unwrap_or(0),
        byte_end: line_clusters.last().map(|c| c.1).unwrap_or(0),
        visual_x: 0.0,
        visual_line_id: 0,
        visual_line_top: 0.0,
        visual_line_bottom: 20.0,
        cache_slot: 0,
        qtextline_idx: 0,
        // Issue #724 评论 5752140048 问题 4a: 测试用段落起始偏移 0。
        paragraph_document_byte_start: 0,
    };
    let layout_snapshot = LayoutSnapshot {
        text_revision: 0,
        text_ptr: 0,
        text_len: virtual_text.len(),
        width: 800.0,
        font_size: 16.0,
        font_family: "sans-serif".to_string(),
        line_spacing: 1.5,
        text_indent: 0.0,
        padding: 0.0,
        lines: vec![VisualLine {
            id: 0,
            byte_start: line.byte_start,
            byte_end: line.byte_end,
            qchar_start: 0,
            qchar_end: 0,
            hard_break: false,
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 20.0,
            para_text: virtual_text.to_string(),
            para_start: 0,
            qtextline_idx: 0,
            para_qchar_start: 0,
            para_qchar_end: 0,
            line_wrap_width: 800.0,
            line_indent_x: 0.0,
            para_indent: 0.0,
            x_end_trailing: 800.0,
            qt_ascent: 16.0,
            qt_descent: 4.0,
            cache_slot: 0,
        }],
        layout_generation: 0,
    };
    EditorLayoutSnapshot::new(
        layout_snapshot,
        vec![line],
        None,
        None,
        CaretAffinity::Downstream,
    )
    .with_virtual_text(virtual_text.to_string())
}

/// Issue #686 评论 5667184642：回归测试——吞字方向必须与光标位置匹配。
///
/// `conceal_to_left_edge = true` 表示向左边缘收缩（Backspace，光标在文字右侧）；
/// `conceal_to_left_edge = false` 表示向右边缘收缩（Delete 键，光标在文字左侧）。
/// 上一轮把比较式写反了（靠左算成 true），这里锁定正确语义。
///
/// 测试布局：old cluster [0,3) source_rect x=10 w=30，dpr=1 visual_x=0
/// → document rect x=10 w=30 → left=10, right=40。
fn make_delete_direction_snapshots() -> (EditorLayoutSnapshot, EditorLayoutSnapshot, OffsetMap) {
    let sid = ShapingIdentity {
        text_content_hash: 1,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 10,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let old_snapshot = make_test_snapshot("abc", vec![(0, 3, 10.0, 0.0, sid)]);
    // new 为空 → old cluster 成为纯 old run → delete_conceal
    let new_snapshot = make_test_snapshot("", vec![]);
    let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
    (old_snapshot, new_snapshot, offset_map)
}

#[test]
fn test_commit_same_shaping_different_geometry_creates_move() {
    let sid_common = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_old_preedit = ShapingIdentity {
        text_content_hash: 10,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 200,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_new_commit = ShapingIdentity {
        text_content_hash: 20,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 300,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let old_snapshot = make_test_snapshot(
        "世界好abc",
        vec![
            (0, 3, 10.0, 0.0, sid_common.clone()),
            (3, 6, 40.0, 0.0, sid_common.clone()),
            (6, 9, 70.0, 0.0, sid_common.clone()),
            (9, 12, 100.0, 0.0, sid_old_preedit.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "世界好xyz",
        vec![
            (0, 3, 50.0, 0.0, sid_common.clone()),
            (3, 6, 80.0, 0.0, sid_common.clone()),
            (6, 9, 110.0, 0.0, sid_common.clone()),
            (9, 12, 140.0, 0.0, sid_new_commit.clone()),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let key = coord.handle_composition_commit_or_cancel(
        &old_snapshot,
        &new_snapshot,
        0,
        12,
        true,
        false,
        0,
        12,
        0,
        12,
        Some(CursorRect {
            x: 10.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 20.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        0,
        LayoutRevision::initial(),
        Instant::now(),
        None,
        true,
        true,
        true,
    );
    assert!(key.is_some());
    let tx = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key.unwrap())
        .unwrap();
    let has_move = tx
        .units
        .iter()
        .any(|u| u.slice.kind == AnimatedSliceKind::ReflowMove);
    assert!(
        has_move,
        "commit with same shaping but different geometry should create ReflowMove slice"
    );
    assert!(
        tx.units
            .iter()
            .any(|u| !u.slice.static_hidden_document_rects.is_empty()),
        "Move slices should have static_hidden_document_rects"
    );
}

#[test]
fn test_commit_different_shaping_creates_crossfade_with_static_patch() {
    let sid_common = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_common_diff = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "font_other".into(),
        glyph_indexes_hash: 999,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_old_preedit = ShapingIdentity {
        text_content_hash: 10,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 200,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_new_commit = ShapingIdentity {
        text_content_hash: 20,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 300,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let old_snapshot = make_test_snapshot(
        "世界好abc",
        vec![
            (0, 3, 10.0, 0.0, sid_common.clone()),
            (3, 6, 40.0, 0.0, sid_common.clone()),
            (6, 9, 70.0, 0.0, sid_common.clone()),
            (9, 12, 100.0, 0.0, sid_old_preedit.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "世界好xyz",
        vec![
            (0, 3, 10.0, 0.0, sid_common.clone()),
            (3, 6, 40.0, 0.0, sid_common_diff.clone()),
            (6, 9, 70.0, 0.0, sid_common.clone()),
            (9, 12, 140.0, 0.0, sid_new_commit.clone()),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let key = coord.handle_composition_commit_or_cancel(
        &old_snapshot,
        &new_snapshot,
        0,
        12,
        true,
        false,
        0,
        12,
        0,
        12,
        Some(CursorRect {
            x: 10.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 20.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        0,
        LayoutRevision::initial(),
        Instant::now(),
        None,
        true,
        true,
        true,
    );
    assert!(key.is_some());
    let tx = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key.unwrap())
        .unwrap();
    let crossfade_count = tx
        .units
        .iter()
        .filter(|u| u.slice.kind == AnimatedSliceKind::ReflowCrossFade)
        .count();
    assert!(
        crossfade_count >= 2,
        "commit with different shaping should create paired Crossfade slices (old+new), got {}",
        crossfade_count
    );
    assert!(
        tx.units
            .iter()
            .any(|u| !u.slice.static_hidden_document_rects.is_empty()),
        "Crossfade new should have static_hidden_document_rects to prevent double-draw"
    );
}

#[test]
fn test_commit_same_shaping_same_geometry_is_static() {
    let sid_common = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_old_preedit = ShapingIdentity {
        text_content_hash: 10,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 200,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_new_commit = ShapingIdentity {
        text_content_hash: 20,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 300,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let old_snapshot = make_test_snapshot(
        "世界好abc",
        vec![
            (0, 3, 10.0, 0.0, sid_common.clone()),
            (3, 6, 40.0, 0.0, sid_common.clone()),
            (6, 9, 70.0, 0.0, sid_common.clone()),
            (9, 12, 100.0, 0.0, sid_old_preedit.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "世界好xyz",
        vec![
            (0, 3, 10.0, 0.0, sid_common.clone()),
            (3, 6, 40.0, 0.0, sid_common.clone()),
            (6, 9, 70.0, 0.0, sid_common.clone()),
            (9, 12, 100.0, 0.0, sid_new_commit.clone()),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let key = coord.handle_composition_commit_or_cancel(
        &old_snapshot,
        &new_snapshot,
        0,
        12,
        true,
        false,
        0,
        12,
        0,
        12,
        Some(CursorRect {
            x: 10.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 20.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        0,
        LayoutRevision::initial(),
        Instant::now(),
        None,
        true,
        true,
        true,
    );
    assert!(key.is_some());
    let tx = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key.unwrap())
        .unwrap();
    let first_cluster_slices: Vec<&AnimatedSlice> = tx
        .units
        .iter()
        .filter(|u| u.slice.byte_start == 0 && u.slice.byte_end == 3)
        .map(|u| &u.slice)
        .collect();
    assert!(
        first_cluster_slices.is_empty(),
        "same shaping + same geometry should be Static (no slice), got {} slices",
        first_cluster_slices.len()
    );
}

#[test]
fn test_commit_separate_preedit_and_committed_replace_ranges() {
    let sid_a = ShapingIdentity {
        text_content_hash: 1,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 10,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_b = ShapingIdentity {
        text_content_hash: 2,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 20,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_c = ShapingIdentity {
        text_content_hash: 3,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 30,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_preedit = ShapingIdentity {
        text_content_hash: 99,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 99,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let old_snapshot = make_test_snapshot(
        "abc_preedit_xyz",
        vec![
            (0, 3, 10.0, 0.0, sid_a.clone()),
            (3, 10, 50.0, 0.0, sid_preedit.clone()),
            (10, 13, 120.0, 0.0, sid_c.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "abc_QQ_xyz",
        vec![
            (0, 3, 10.0, 0.0, sid_a.clone()),
            (3, 5, 50.0, 0.0, sid_b.clone()),
            (5, 8, 120.0, 0.0, sid_c.clone()),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let key = coord.handle_composition_commit_or_cancel(
        &old_snapshot,
        &new_snapshot,
        0,
        12,
        true,
        false,
        0,
        12,
        0,
        12,
        Some(CursorRect {
            x: 10.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 20.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        0,
        LayoutRevision::initial(),
        Instant::now(),
        None,
        true,
        true,
        true,
    );
    assert!(key.is_some());
    let tx = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key.unwrap())
        .unwrap();
    let old_preedit_slices: Vec<&AnimatedSlice> = tx
        .units
        .iter()
        .filter(|u| u.slice.byte_start >= 3 && u.slice.byte_end <= 10)
        .map(|u| &u.slice)
        .collect();
    assert!(
        !old_preedit_slices.is_empty(),
        "preedit range should have animated slices"
    );
    let new_candidate_slices: Vec<&AnimatedSlice> = tx
        .units
        .iter()
        .filter(|u| u.slice.byte_start >= 3 && u.slice.byte_end <= 5)
        .map(|u| &u.slice)
        .collect();
    assert!(
        !new_candidate_slices.is_empty(),
        "candidate range should have animated slices"
    );
}

#[test]
fn test_commit_cancel_uses_preedit_range_for_old_clusters() {
    let sid_preedit = ShapingIdentity {
        text_content_hash: 99,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 99,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_after = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let old_snapshot = make_test_snapshot(
        "abc_preedit_after",
        vec![
            (0, 3, 10.0, 0.0, sid_after.clone()),
            (3, 10, 50.0, 0.0, sid_preedit.clone()),
            (10, 15, 120.0, 0.0, sid_after.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "abc_after",
        vec![
            (0, 3, 10.0, 0.0, sid_after.clone()),
            (3, 8, 120.0, 0.0, sid_after.clone()),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let key = coord.handle_composition_commit_or_cancel(
        &old_snapshot,
        &new_snapshot,
        3,
        10,
        false,
        false,
        3,
        3,
        3,
        3,
        Some(CursorRect {
            x: 10.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 20.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        0,
        LayoutRevision::initial(),
        Instant::now(),
        None,
        true,
        true,
        true,
    );
    assert!(key.is_some());
    let tx = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key.unwrap())
        .unwrap();
    let delete_slices: Vec<&AnimatedSlice> = tx
        .units
        .iter()
        .filter(|u| u.slice.kind == AnimatedSliceKind::DeleteConceal)
        .map(|u| &u.slice)
        .collect();
    assert!(
        !delete_slices.is_empty(),
        "cancel should create DeleteConceal for preedit range"
    );
}

#[test]
fn test_many_to_one_reflow_one_old_splits_to_two_new() {
    // Issue #658 评论 5630181473 问题 3: one old cluster [0,3) maps to
    // two new clusters [0,1) + [4,6) via OffsetMap (insert "XYZ" at position 1).
    // new cluster [1,4) is inserted text with no old counterpart → InsertReveal.
    // old cluster [0,3) connects to both [0,1) and [4,6) → N→M crossfade run.
    let sid_common = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_preedit = ShapingIdentity {
        text_content_hash: 10,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 200,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_new_a = ShapingIdentity {
        text_content_hash: 50,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 300,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_new_b = ShapingIdentity {
        text_content_hash: 51,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 301,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    // old: "abc" → one cluster covering [0,3)
    let old_snapshot = make_test_snapshot("abc", vec![(0, 3, 10.0, 0.0, sid_preedit.clone())]);
    // new: "aXYZbc" → three clusters: [0,1) "a", [1,4) "XYZ", [4,6) "bc"
    let new_snapshot = make_test_snapshot(
        "aXYZbc",
        vec![
            (0, 1, 10.0, 0.0, sid_new_a.clone()),
            (1, 4, 20.0, 0.0, sid_common.clone()),
            (4, 6, 40.0, 0.0, sid_new_b.clone()),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let key = coord.handle_composition_update(
        &old_snapshot,
        &new_snapshot,
        0,
        3,
        0,
        3,
        Some(CursorRect {
            x: 10.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 20.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        0,
        LayoutRevision::initial(),
        true,
        true,
        true,
    );
    assert!(key.is_some());
    let tx = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key.unwrap())
        .unwrap();
    // N→M run: 1 old [0,3) + 2 new [0,1),[4,6) → crossfade slices
    // old cluster [0,3) produces crossfade_old (uses first new's byte range [0,1))
    let crossfade_count = tx
        .units
        .iter()
        .filter(|u| u.slice.kind == AnimatedSliceKind::ReflowCrossFade)
        .count();
    // 1 crossfade_old (old [0,3)) + 2 crossfade_new (new [0,1) + new [4,6)) = 3
    assert_eq!(
        crossfade_count, 3,
        "expected 3 crossfade slices (1 old + 2 new), got {}",
        crossfade_count
    );
    // new cluster [1,4) is inserted text (no old counterpart) → InsertReveal
    let insert_count = tx
        .units
        .iter()
        .filter(|u| u.slice.kind == AnimatedSliceKind::InsertReveal)
        .filter(|u| u.slice.byte_start == 1 && u.slice.byte_end == 4)
        .count();
    assert_eq!(
        insert_count, 1,
        "inserted cluster [1,4) should produce exactly 1 InsertReveal, got {}",
        insert_count
    );
    // No DeleteConceal for these ranges
    let delete_count = tx
        .units
        .iter()
        .filter(|u| u.slice.kind == AnimatedSliceKind::DeleteConceal)
        .count();
    assert_eq!(delete_count, 0, "should not produce DeleteConceal");
}

#[test]
fn test_delete_conceal_direction_cursor_near_right_is_backspace() {
    let (old_snapshot, _new_snapshot, _offset_map) = make_delete_direction_snapshots();
    let key = VisualTransactionKey::new(1, 1);
    // 旧光标靠近右端 (x=39, right=40) → Backspace → conceal_to_left_edge=true
    let old_cursor = CursorRect {
        x: 39.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    // Issue #687: changed range 由 build_delete_conceal_slices 显式拥有。
    // old cluster [0,3) 是被删除的范围。
    let slices = build_delete_conceal_slices(key, &old_snapshot, (0, 3), Some(&old_cursor));
    let delete_slices: Vec<_> = slices
        .iter()
        .filter(|s| s.kind == AnimatedSliceKind::DeleteConceal)
        .collect();
    assert_eq!(
        delete_slices.len(),
        1,
        "deleted range [0,3) should produce exactly one DeleteConceal"
    );
    assert!(
        delete_slices[0].conceal_to_left_edge,
        "cursor near right (x=39, right=40) should be Backspace → conceal_to_left_edge=true"
    );
}

#[test]
fn test_delete_conceal_direction_cursor_near_left_is_delete() {
    let (old_snapshot, _new_snapshot, _offset_map) = make_delete_direction_snapshots();
    let key = VisualTransactionKey::new(1, 1);
    // 旧光标靠近左端 (x=11, left=10) → Delete 键 → conceal_to_left_edge=false
    let old_cursor = CursorRect {
        x: 11.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    // Issue #687: changed range 由 build_delete_conceal_slices 显式拥有。
    // old cluster [0,3) 是被删除的范围。
    let slices = build_delete_conceal_slices(key, &old_snapshot, (0, 3), Some(&old_cursor));
    let delete_slices: Vec<_> = slices
        .iter()
        .filter(|s| s.kind == AnimatedSliceKind::DeleteConceal)
        .collect();
    assert_eq!(
        delete_slices.len(),
        1,
        "deleted range [0,3) should produce exactly one DeleteConceal"
    );
    assert!(
        !delete_slices[0].conceal_to_left_edge,
        "cursor near left (x=11, left=10) should be Delete → conceal_to_left_edge=false"
    );
}

// ── Issue #756: 文字动画与光标动画互相独立 ─────────────────────────────────────
//
// 设置页的"协同动画"是显式模式（coordinated_animation_enabled），不再由
// typing_animation_enabled && smooth_cursor_enabled 隐式组成：
// - text_animation_enabled  = coordinated || typing_animation_enabled  → Reflow + 吞吐字
// - caret_animation_enabled = coordinated || smooth_cursor_enabled    → caret motion track
//
// 下面四组用例锁定两个开关的独立性，以及"两个开关同时开启 ≠ 协同"。

fn issue756_shaping_identity() -> ShapingIdentity {
    ShapingIdentity {
        text_content_hash: 756,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 756,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    }
}

/// 构造"在 ab 的 1 处插入 x"的一笔编辑 spec。
/// `cursor_rects=false` 模拟建不出有效 caret motion（无 old/new caret rect）的一笔。
/// Issue #756: 接收 coordinated/typing/smooth 三个独立开关，内部算出
/// text = coordinated || typing，caret = coordinated || smooth。
fn issue756_insert_spec(
    key: VisualTransactionKey,
    coordinated_animation_enabled: bool,
    typing_animation_enabled: bool,
    smooth_cursor_enabled: bool,
    cursor_rects: bool,
) -> VisualEditSpec {
    issue756_insert_spec_with_durations(
        key,
        coordinated_animation_enabled,
        typing_animation_enabled,
        smooth_cursor_enabled,
        cursor_rects,
        100,
        100,
    )
}

/// Issue #756 评论 5821042551: 支持独立的 text/caret duration。
/// 非协同时 text_duration_ms = typing duration，caret_duration_ms = smooth cursor duration。
/// 协同时两者都用 typing duration（共享 timeline）。
fn issue756_insert_spec_with_durations(
    key: VisualTransactionKey,
    coordinated_animation_enabled: bool,
    typing_animation_enabled: bool,
    smooth_cursor_enabled: bool,
    cursor_rects: bool,
    text_duration_ms: u64,
    caret_duration_ms: u64,
) -> VisualEditSpec {
    let text_animation_enabled = coordinated_animation_enabled || typing_animation_enabled;
    let caret_animation_enabled = coordinated_animation_enabled || smooth_cursor_enabled;
    // Issue #756 评论 5821042551: 协同时两个 duration 都用 typing duration（共享 timeline）；
    // 非协同时文字用 typing、光标用 smooth，各自独立。
    let actual_text_duration = if coordinated_animation_enabled {
        text_duration_ms
    } else {
        text_duration_ms
    };
    let actual_caret_duration = if coordinated_animation_enabled {
        text_duration_ms
    } else {
        caret_duration_ms
    };
    let sid = issue756_shaping_identity();
    let old_snapshot = make_test_snapshot(
        "ab",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "axb",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid.clone()),
            (2, 3, 20.0, 0.0, sid),
        ],
    );
    let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
    let (old_cursor_rect, new_cursor_rect) = if cursor_rects {
        (
            Some(CursorRect {
                x: 10.0,
                top: 0.0,
                bottom: 20.0,
                baseline_y: 16.0,
            }),
            Some(CursorRect {
                x: 20.0,
                top: 0.0,
                bottom: 20.0,
                baseline_y: 16.0,
            }),
        )
    } else {
        (None, None)
    };
    VisualEditSpec {
        key,
        operation_kind: TextVisualOperationKind::Insert,
        old_snapshot,
        new_snapshot,
        inserted_ranges: vec![(1, 2)],
        deleted_ranges: vec![],
        offset_map,
        old_cursor_rect,
        new_cursor_rect,
        old_cursor_visual_line_id: Some(0),
        new_cursor_visual_line_id: Some(0),
        old_cursor_line_top: 0.0,
        old_cursor_line_bottom: 20.0,
        new_cursor_line_top: 0.0,
        new_cursor_line_bottom: 20.0,
        cursor_owner_epoch: 1,
        layout_basis_revision: LayoutRevision::initial(),
        rebase_frames: Vec::new(),
        caret_handoff: None,
        visual_affected_byte_range_old: Some((0, 2)),
        visual_affected_byte_range_new: Some((0, 3)),
        text_duration_ms: actual_text_duration,
        caret_duration_ms: actual_caret_duration,
        text_animation_enabled,
        caret_animation_enabled,
        coordinated_animation_enabled,
        composition_commit_crossfade: None,
    }
}

fn issue756_count_kind(tx: &PreparedTextVisualTransaction, kind: AnimatedSliceKind) -> usize {
    tx.units.iter().filter(|u| u.slice.kind == kind).count()
}

/// 协同模式：文字与光标都开 → 吞吐字（InsertReveal）与 caret track 同时建立。
#[test]
fn issue756_coordinated_creates_caret_track_and_reveal_together() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, true, false, false, true));
    assert_eq!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal),
        1,
        "coordinated=true: 必须建立 InsertReveal（文字与光标绑死）"
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "coordinated=true: 必须建立 caret motion track"
    );
}

/// 只开打字动画（coordinated=false, typing=true, smooth=false）：文字动画照播，
/// 吞吐字用 typing timeline 推进（不消费 caret frame），光标不沿 track 滑动。
#[test]
fn issue756_typing_only_creates_text_without_caret_track() {
    let key = VisualTransactionKey::new(1, 756);
    // coordinated=false, typing=true, smooth=false：只有文字动画。
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, true, false, true));
    // Issue #756 问题 2: 打字动画开启时必须有吐字（用 typing timeline 推进，不消费 caret frame）。
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) > 0,
        "打字动画开启：必须有 InsertReveal 吞吐字（typing timeline 推进）"
    );
    assert!(
        tx.cursor_visual_track.is_none(),
        "smooth cursor 关闭且非协同：不建立 caret track，光标不得沿 track 滑动"
    );
    assert!(!tx.units.is_empty(), "打字动画开启：文字动画照播");
    // Issue #756: coordinated=false 时吞吐字是 Timed（typing-driven），不是 CaretDriven。
    assert!(
        !tx.coordinated,
        "coordinated=false: 事务不进入 coordinated ownership"
    );
    assert!(
        tx.units.iter().all(|u| !u.timing.is_caret_driven()),
        "coordinated=false: 吞吐字用 Timed timing，不消费 caret frame"
    );
}

/// 只开平滑光标（coordinated=false, typing=false）：caret track 照建，没有文字动画。
#[test]
fn issue756_smooth_only_creates_caret_track_without_text_animation() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, false, true, true));
    assert!(
        tx.units.is_empty(),
        "打字动画关闭且非协同：不得生成任何文字动画 unit，实际 {:?}",
        unit_kind_labels(&tx.units)
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "平滑光标开启：caret track 必须照建，不被 typing_animation_enabled 关掉"
    );
}

/// 两个独立开关同时开启 ≠ 协同：建不出 caret motion 时只影响光标动画，文字动画照建。
///
/// coordinated=true 的"缺 caret motion 就整笔不创建"由 coordinator 层强制
///（见 `issue756_process_transaction_requires_caret_motion_only_when_coordinated`）；
/// builder 层只按 spec 的两个开关产出 slice/track。
#[test]
fn issue756_typing_and_smooth_are_not_treated_as_coordinated() {
    let key = VisualTransactionKey::new(1, 756);
    // coordinated=false，但建不出 caret motion（无 old/new caret rect）。
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, true, true, false));
    assert!(
        tx.cursor_visual_track.is_none(),
        "没有 caret rect 时自然没有 caret track"
    );
    assert!(
        !tx.units.is_empty(),
        "coordinated=false: 缺 caret motion 不能把整笔文字动画一起关掉（旧逻辑的隐式协同）"
    );
}

/// 两个独立开关都关闭且非协同：没有任何动画。
#[test]
fn issue756_all_disabled_produces_no_units_and_no_track() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, false, false, true));
    assert!(tx.units.is_empty(), "两个开关都关闭且非协同：没有文字动画");
    assert!(
        tx.cursor_visual_track.is_none(),
        "两个开关都关闭且非协同：没有光标动画"
    );
}

/// Issue #756: 有 old/new caret rect 时两个独立开关同时开 ≠ 协同。
///
/// coordinated=false + typing=true + smooth=true + 有 caret rect：
/// - 有 InsertReveal（文字动画）
/// - 有 cursor_visual_track（光标动画）
/// - 不进入 coordinated ownership（吞吐字用 Timed timing，不消费 caret frame）
#[test]
fn issue756_typing_and_smooth_with_caret_rect_are_independent() {
    let key = VisualTransactionKey::new(1, 756);
    // coordinated=false, typing=true, smooth=true, 有 caret rect。
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, true, true, true));
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) > 0,
        "typing=true: 必须有 InsertReveal（文字动画）"
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "smooth=true 且有 caret rect: 必须有 cursor_visual_track（光标动画）"
    );
    assert!(
        !tx.coordinated,
        "coordinated=false: 不进入 coordinated ownership"
    );
    assert!(
        tx.units.iter().all(|u| !u.timing.is_caret_driven()),
        "coordinated=false: 吞吐字用 Timed timing，不消费 caret frame（两个动画并行但不绑死）"
    );
}

/// Issue #756 IME 四组组合测试：验证 text=coordinated||typing, caret=coordinated||smooth,
/// coordinated 决定吞吐字是否由 caret 驱动。用 build_prepared_transaction 直接测
/// composition 的 VisualEditSpec 构造，传入不同的 coordinated/typing/smooth 组合。

/// coordinated=true + typing=false + smooth=false：协同仍正常
///（text=true, caret=true, coordinated=true，吞吐字 CaretDriven）。
#[test]
fn issue756_ime_coordinated_only() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, true, false, false, true));
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) > 0,
        "coordinated=true: 必须有 InsertReveal"
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "coordinated=true: 必须有 caret track"
    );
    assert!(tx.coordinated, "coordinated=true: 事务标记 coordinated");
    assert!(
        tx.units.iter().any(|u| u.timing.is_caret_driven()),
        "coordinated=true: 吞吐字用 CaretDriven timing（消费 caret frame）"
    );
}

/// coordinated=false + typing=true + smooth=false：只有文字动画
///（text=true, caret=false, coordinated=false，吞吐字 Timed）。
#[test]
fn issue756_ime_typing_only() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, true, false, true));
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) > 0,
        "typing=true: 必须有 InsertReveal"
    );
    assert!(
        tx.cursor_visual_track.is_none(),
        "smooth=false 且非协同: 没有 caret track"
    );
    assert!(!tx.coordinated, "coordinated=false: 事务不标记 coordinated");
    assert!(
        tx.units.iter().all(|u| !u.timing.is_caret_driven()),
        "coordinated=false: 吞吐字用 Timed timing"
    );
}

/// coordinated=false + typing=false + smooth=true：只有光标动画
///（text=false, caret=true, coordinated=false，无吞吐字）。
#[test]
fn issue756_ime_smooth_only() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, false, true, true));
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) == 0,
        "typing=false 且非协同: 没有 InsertReveal"
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "smooth=true: 必须有 caret track"
    );
    assert!(!tx.coordinated, "coordinated=false: 事务不标记 coordinated");
}

/// coordinated=false + typing=true + smooth=true：两个动画并行但不进入 coordinated ownership
///（text=true, caret=true, coordinated=false，吞吐字 Timed）。
#[test]
fn issue756_ime_typing_and_smooth_not_coordinated() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, true, true, true));
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) > 0,
        "typing=true: 必须有 InsertReveal"
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "smooth=true: 必须有 caret track"
    );
    assert!(
        !tx.coordinated,
        "coordinated=false: 两个开关同时开不等于协同"
    );
    assert!(
        tx.units.iter().all(|u| !u.timing.is_caret_driven()),
        "coordinated=false: 吞吐字用 Timed timing，不消费 caret frame"
    );
}

/// Issue #756: 事务创建条件不再把"两个独立开关同时开启"当协同。
///
/// - coordinated=true：文字与光标绑死，必须有有效 caret motion，否则整笔（含文字动画）不创建。
/// - coordinated=false：typing/smooth 各自决定文字/光标动画，缺 caret motion 只影响光标动画。
#[test]
fn issue756_process_transaction_requires_caret_motion_only_when_coordinated() {
    use crate::sujian_editor_item::edit_motion::{EditorAnimationKind, PreparedEditMotion};
    use writer_core::editor::{EditorCursor, EditorSelection, Utf8ByteRange};

    let sid = issue756_shaping_identity();
    let old_snapshot = make_test_snapshot(
        "ab",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "axb",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid.clone()),
            (2, 3, 20.0, 0.0, sid),
        ],
    );
    // 建不出有效 caret motion：无 old/new caret rect。
    let vt = PreparedEditMotion {
        kind: EditorAnimationKind::Insert,
        inserted_range: Some(Utf8ByteRange::from_ordered(1, 2)),
        deleted_range: None,
        old_text: "ab".to_string(),
        new_text: "axb".to_string(),
        text_duration_ms: 100,
        caret_duration_ms: 100,
        old_selection: EditorSelection {
            anchor: EditorCursor::new("ab", 1),
            head: EditorCursor::new("ab", 1),
        },
        new_selection: EditorSelection {
            anchor: EditorCursor::new("axb", 2),
            head: EditorCursor::new("axb", 2),
        },
        old_cursor_rect: None,
        new_cursor_rect: None,
    };

    let mut coordinated_coord = LinuxEditorAnimationCoordinator::new();
    let coordinated_key = coordinated_coord.process_transaction(
        &vt,
        true,
        true,
        true,
        false,
        false,
        false,
        None,
        None,
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        &old_snapshot,
        &new_snapshot,
        1,
        LayoutRevision::initial(),
    );
    assert!(
        coordinated_key.is_none(),
        "coordinated=true 且 caret motion 建不起来：整笔文字动画也不启动"
    );

    let mut independent_coord = LinuxEditorAnimationCoordinator::new();
    let independent_key = independent_coord.process_transaction(
        &vt,
        true,
        true,
        false,
        false,
        false,
        false,
        None,
        None,
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        &old_snapshot,
        &new_snapshot,
        1,
        LayoutRevision::initial(),
    );
    assert!(
        independent_key.is_some(),
        "coordinated=false 且 typing/smooth 同时开启：不能被当成协同，"
    );
    assert!(
        independent_key.is_some(),
        "typing_animation_enabled 决定文字动画：缺 caret motion 不能把文字动画一起关掉"
    );
}

/// Issue #756: coordinated=false 时 smooth 关闭不影响文字动画创建事务。
#[test]
fn issue756_process_transaction_typing_only_still_creates_transaction() {
    use crate::sujian_editor_item::edit_motion::{EditorAnimationKind, PreparedEditMotion};
    use writer_core::editor::{EditorCursor, EditorSelection, Utf8ByteRange};

    let sid = issue756_shaping_identity();
    let old_snapshot = make_test_snapshot("ab", vec![(0, 2, 0.0, 0.0, sid.clone())]);
    let new_snapshot = make_test_snapshot("axb", vec![(0, 3, 0.0, 0.0, sid)]);
    let vt = PreparedEditMotion {
        kind: EditorAnimationKind::Insert,
        inserted_range: Some(Utf8ByteRange::from_ordered(1, 2)),
        deleted_range: None,
        old_text: "ab".to_string(),
        new_text: "axb".to_string(),
        text_duration_ms: 100,
        caret_duration_ms: 100,
        old_selection: EditorSelection {
            anchor: EditorCursor::new("ab", 1),
            head: EditorCursor::new("ab", 1),
        },
        new_selection: EditorSelection {
            anchor: EditorCursor::new("axb", 2),
            head: EditorCursor::new("axb", 2),
        },
        old_cursor_rect: None,
        new_cursor_rect: None,
    };

    let mut coord = LinuxEditorAnimationCoordinator::new();
    let key = coord.process_transaction(
        &vt,
        true,
        false,
        false,
        false,
        false,
        false,
        None,
        None,
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        &old_snapshot,
        &new_snapshot,
        1,
        LayoutRevision::initial(),
    );
    assert!(
        key.is_some(),
        "coordinated=false + typing=true + smooth=false：旧逻辑要求两个开关同时开启，"
    );

    let mut none_coord = LinuxEditorAnimationCoordinator::new();
    let none_key = none_coord.process_transaction(
        &vt,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
        None,
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        &old_snapshot,
        &new_snapshot,
        1,
        LayoutRevision::initial(),
    );
    assert!(
        none_key.is_none(),
        "coordinated=false 且 typing/smooth 都关闭：不创建任何事务"
    );
}

// ── Issue #756 评论 5821042551: 两个独立 duration 真正独立 ─────────────────────
//
// 非协同时 text_duration_ms 和 caret_duration_ms 必须各自独立：
// - Timed 文字 unit 用 typing duration
// - cursor_visual_track 用 smooth cursor duration
// 协同时两者都用 typing duration（共享 timeline）。
//
// 事务完成条件：只要存在 cursor_visual_track 就必须等待它结束，
// 不再仅限于 CaretDriven 事务。

/// Issue #756 评论 5821042551: 非协同 typing=100ms + smooth=300ms，
/// caret track 的 duration 必须是 300ms（smooth），不是 100ms（typing）。
#[test]
fn issue756_comment5821042551_independent_durations_typing_short_smooth_long() {
    let key = VisualTransactionKey::new(1, 756);
    // coordinated=false, typing=true, smooth=true, typing=100ms, smooth=300ms
    let tx = build_prepared_transaction(issue756_insert_spec_with_durations(
        key,
        false,
        true,
        true,
        true,
        100,
        300,
    ));
    // 文字 unit 用 typing duration (100ms)
    for unit in &tx.units {
        match unit.timing {
            VisualUnitTiming::Timed { duration_ms, .. } => {
                assert_eq!(
                    duration_ms, 100,
                    "非协同: Timed 文字 unit 必须用 typing duration (100ms)，不是 smooth (300ms)"
                );
            }
            VisualUnitTiming::CaretDriven { .. } => {
                panic!("非协同: 吞吐字不应是 CaretDriven");
            }
        }
    }
    // cursor_visual_track 用 smooth cursor duration (300ms)
    let track = tx
        .cursor_visual_track
        .as_ref()
        .expect("smooth=true: 必须有 cursor_visual_track");
    assert_eq!(
        track.duration_ms, 300,
        "非协同: cursor_visual_track 必须用 smooth cursor duration (300ms)，不是 typing (100ms)"
    );
}

/// Issue #756 评论 5821042551: 反向 — 非协同 typing=300ms + smooth=100ms，
/// caret track 的 duration 必须是 100ms（smooth），不是 300ms（typing）。
#[test]
fn issue756_comment5821042551_independent_durations_typing_long_smooth_short() {
    let key = VisualTransactionKey::new(1, 756);
    // coordinated=false, typing=true, smooth=true, typing=300ms, smooth=100ms
    let tx = build_prepared_transaction(issue756_insert_spec_with_durations(
        key,
        false,
        true,
        true,
        true,
        300,
        100,
    ));
    // 文字 unit 用 typing duration (300ms)
    for unit in &tx.units {
        match unit.timing {
            VisualUnitTiming::Timed { duration_ms, .. } => {
                assert_eq!(
                    duration_ms, 300,
                    "非协同: Timed 文字 unit 必须用 typing duration (300ms)，不是 smooth (100ms)"
                );
            }
            VisualUnitTiming::CaretDriven { .. } => {
                panic!("非协同: 吞吐字不应是 CaretDriven");
            }
        }
    }
    // cursor_visual_track 用 smooth cursor duration (100ms)
    let track = tx
        .cursor_visual_track
        .as_ref()
        .expect("smooth=true: 必须有 cursor_visual_track");
    assert_eq!(
        track.duration_ms, 100,
        "非协同: cursor_visual_track 必须用 smooth cursor duration (100ms)，不是 typing (300ms)"
    );
}

/// Issue #756 评论 5821042551: 协同时两个 duration 都用 typing duration（共享 timeline）。
#[test]
fn issue756_comment5821042551_coordinated_shares_typing_duration() {
    let key = VisualTransactionKey::new(1, 756);
    // coordinated=true, typing=false, smooth=false, typing=100ms, smooth=300ms
    // 协同时 caret_duration_ms 应等于 text_duration_ms（共享 timeline）
    let tx = build_prepared_transaction(issue756_insert_spec_with_durations(
        key,
        true,
        false,
        false,
        true,
        100,
        300,
    ));
    // 协同: 吞吐字是 CaretDriven
    assert!(
        tx.units.iter().any(|u| u.timing.is_caret_driven()),
        "协同: 吞吐字用 CaretDriven timing"
    );
    // 协同: cursor_visual_track 用 typing duration (100ms)，不是 smooth (300ms)
    let track = tx
        .cursor_visual_track
        .as_ref()
        .expect("coordinated=true: 必须有 cursor_visual_track");
    assert_eq!(
        track.duration_ms, 100,
        "协同: cursor_visual_track 用 typing duration (100ms)（共享 timeline），不是 smooth (300ms)"
    );
}

/// Issue #756 评论 5821042551: 非协同 smooth-only（typing=false, smooth=true）
/// 也必须有独立的 caret_duration_ms，不受 typing duration 影响。
#[test]
fn issue756_comment5821042551_smooth_only_has_independent_caret_duration() {
    let key = VisualTransactionKey::new(1, 756);
    // coordinated=false, typing=false, smooth=true, typing=100ms, smooth=300ms
    let tx = build_prepared_transaction(issue756_insert_spec_with_durations(
        key,
        false,
        false,
        true,
        true,
        100,
        300,
    ));
    // smooth-only: 没有文字 unit
    assert!(
        tx.units.is_empty(),
        "smooth-only: 没有文字动画 unit"
    );
    // caret track 用 smooth cursor duration (300ms)
    let track = tx
        .cursor_visual_track
        .as_ref()
        .expect("smooth=true: 必须有 cursor_visual_track");
    assert_eq!(
        track.duration_ms, 300,
        "smooth-only: cursor_visual_track 用 smooth cursor duration (300ms)，不是 typing (100ms)"
    );
}

/// Issue #756 评论 5821042551: 验证事务存在 cursor_visual_track 时
/// 事务完成必须等待 caret track 结束。非协同 typing+smooth 没有 CaretDriven unit，
/// 但 cursor_visual_track 存在，事务不能在文字 unit 结束后就提前完成。
///
/// 此测试验证 build_prepared_transaction 产出的 cursor_visual_track 有独立的 duration，
/// 且事务 timeline 的 duration 与文字 unit 的 duration 一致（不是 caret track 的）。
/// render_plan_builder 中的完成条件逻辑（caret_track_complete）由
/// `cursor_visual_track.is_none() || caret_motion_retired || caret_track_done` 决定，
/// 只要 cursor_visual_track 存在就必须等它完成。
#[test]
fn issue756_comment5821042551_transaction_has_caret_track_must_wait_for_completion() {
    let key = VisualTransactionKey::new(1, 756);
    // coordinated=false, typing=true, smooth=true, typing=100ms, smooth=300ms
    let tx = build_prepared_transaction(issue756_insert_spec_with_durations(
        key,
        false,
        true,
        true,
        true,
        100,
        300,
    ));

    // 事务必须有 cursor_visual_track（smooth=true）
    assert!(
        tx.cursor_visual_track.is_some(),
        "smooth=true: 事务必须有 cursor_visual_track，完成条件必须等待它"
    );

    // cursor_visual_track 的 duration (300ms) 比文字 unit 的 duration (100ms) 长，
    // 所以事务不能在 100ms 时就完成——必须等 caret track 在 300ms 时完成。
    let track = tx.cursor_visual_track.as_ref().unwrap();
    assert_eq!(
        track.duration_ms, 300,
        "caret track duration 必须是 300ms（比文字 100ms 更长）"
    );

    // 事务 timeline 用文字 duration（100ms），因为文字动画是主动画
    // 但事务完成条件必须额外等 caret track 完成
    assert_eq!(
        tx.timeline.duration_ms, 100,
        "事务 timeline 用文字 duration (100ms)"
    );
}

/// Issue #756 评论 5821042551: composition update 在 smooth-only 参数组合下
/// 必须创建事务（coordinated=false + typing=false + smooth=true）。
/// 此测试覆盖 handle_composition_update 的真实入口参数组合，
/// 验证底层 composition handler 在 smooth-only 时也能正确创建事务。
#[test]
fn issue756_comment5821042551_composition_update_smooth_only_creates_transaction() {
    let sid = issue756_shaping_identity();
    let old_snapshot = make_test_snapshot(
        "ab",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "axb",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid.clone()),
            (2, 3, 20.0, 0.0, sid),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // coordinated=false, typing=false, smooth=true → caret=true, text=false
    // 这是 smooth-only 的 composition update，正文立即显示，但 caret 用独立 smooth track 移动。
    let key = coord.handle_composition_update(
        &old_snapshot,
        &new_snapshot,
        0,
        2,
        0,
        2,
        Some(CursorRect {
            x: 10.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 20.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        1,
        LayoutRevision::initial(),
        // text=false, caret=true, coordinated=false
        false,
        true,
        false,
    );
    assert!(
        key.is_some(),
        "smooth-only composition update: 必须创建事务（coordinated=false + typing=false + smooth=true）"
    );
    let tx = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key.unwrap())
        .unwrap();
    // smooth-only: 没有文字 unit（text=false）
    assert!(
        tx.units.is_empty(),
        "smooth-only composition: 不应有文字 unit"
    );
    // smooth-only: 必须有 caret track（caret=true）
    assert!(
        tx.cursor_visual_track.is_some(),
        "smooth-only composition: 必须有 cursor_visual_track"
    );
}

/// Issue #756 评论 5821042551: composition commit 在 smooth-only 参数组合下
/// 必须走 composition 专属路径（coordinated=false + typing=false + smooth=true）。
/// 此测试覆盖 handle_composition_commit_or_cancel 的真实入口参数组合。
#[test]
fn issue756_comment5821042551_composition_commit_smooth_only_creates_transaction() {
    let sid_preedit = ShapingIdentity {
        text_content_hash: 99,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 99,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_after = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let old_snapshot = make_test_snapshot(
        "abc_preedit_after",
        vec![
            (0, 3, 10.0, 0.0, sid_after.clone()),
            (3, 10, 50.0, 0.0, sid_preedit.clone()),
            (10, 15, 120.0, 0.0, sid_after.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "abc_QQ_after",
        vec![
            (0, 3, 10.0, 0.0, sid_after.clone()),
            (3, 5, 50.0, 0.0, sid_after.clone()),
            (5, 10, 80.0, 0.0, sid_after),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // coordinated=false, typing=false, smooth=true → caret=true, text=false
    let key = coord.handle_composition_commit_or_cancel(
        &old_snapshot,
        &new_snapshot,
        3,
        10,
        true,
        false,
        3,
        5,
        3,
        10,
        Some(CursorRect {
            x: 50.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 80.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        1,
        LayoutRevision::initial(),
        Instant::now(),
        None,
        // text=false, caret=true, coordinated=false
        false,
        true,
        false,
    );
    assert!(
        key.is_some(),
        "smooth-only composition commit: 必须创建事务（coordinated=false + typing=false + smooth=true）"
    );
    let tx = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key.unwrap())
        .unwrap();
    // smooth-only commit: 必须有 caret track
    assert!(
        tx.cursor_visual_track.is_some(),
        "smooth-only composition commit: 必须有 cursor_visual_track"
    );
    // Issue #756 评论 5821793349: smooth-only commit 不应有文字 unit
    assert!(
        tx.units.is_empty(),
        "smooth-only composition commit: tx.units 必须为空（text_animation_enabled=false）"
    );
    // smooth-only commit: coordinated 必须为 false
    assert!(
        !tx.coordinated,
        "smooth-only composition commit: coordinated 必须为 false"
    );
}

/// Issue #756 评论 5821793349: composition commit 在 typing=true 时必须保留
/// crossfade 文字 unit（DeleteConceal/InsertReveal/ReflowCrossFade/ReflowMove），
/// 避免把 text_animation_enabled 收口修过头。
#[test]
fn issue756_comment5821793349_composition_commit_typing_enabled_keeps_crossfade_units() {
    let sid_preedit = ShapingIdentity {
        text_content_hash: 99,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 99,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_after = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let old_snapshot = make_test_snapshot(
        "abc_preedit_after",
        vec![
            (0, 3, 10.0, 0.0, sid_after.clone()),
            (3, 10, 50.0, 0.0, sid_preedit.clone()),
            (10, 15, 120.0, 0.0, sid_after.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "abc_QQ_after",
        vec![
            (0, 3, 10.0, 0.0, sid_after.clone()),
            (3, 5, 50.0, 0.0, sid_after.clone()),
            (5, 10, 80.0, 0.0, sid_after),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // coordinated=false, typing=true, smooth=true → text=true, caret=true
    let key = coord.handle_composition_commit_or_cancel(
        &old_snapshot,
        &new_snapshot,
        3,
        10,
        true,
        false,
        3,
        5,
        3,
        10,
        Some(CursorRect {
            x: 50.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 80.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        1,
        LayoutRevision::initial(),
        Instant::now(),
        None,
        // text=true, caret=true, coordinated=false
        true,
        true,
        false,
    );
    assert!(
        key.is_some(),
        "typing-enabled composition commit: 必须创建事务"
    );
    let tx = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key.unwrap())
        .unwrap();
    // typing-enabled commit: 必须有文字 unit（crossfade）
    assert!(
        !tx.units.is_empty(),
        "typing-enabled composition commit: tx.units 不应为空（text_animation_enabled=true，crossfade 文字动画必须保留）"
    );
    // 仍应有 caret track
    assert!(
        tx.cursor_visual_track.is_some(),
        "typing-enabled composition commit: 必须有 cursor_visual_track"
    );
    // coordinated 仍为 false
    assert!(
        !tx.coordinated,
        "typing-enabled composition commit: coordinated 必须为 false"
    );
}

/// Issue #756 评论 5822051193: coordinated=true 的 composition update，
/// 最终无法建立 cursor_visual_track（new_cursor_rect=None）时，
/// 必须返回 None 且不 enqueue 任何文字 transaction。
#[test]
fn issue756_comment5822051193_composition_update_coordinated_no_cursor_track_returns_none() {
    let sid = issue756_shaping_identity();
    let old_snapshot = make_test_snapshot(
        "ab",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "axb",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid.clone()),
            (2, 3, 20.0, 0.0, sid),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // coordinated=true, text=true, caret=true
    // new_cursor_rect=None → build_cursor_visual_track 第 26 行 `let to = new_cursor_rect?;`
    // 直接返回 None → cursor_visual_track 为 None → 门禁触发 → 返回 None
    let key = coord.handle_composition_update(
        &old_snapshot,
        &new_snapshot,
        1,
        1,
        1,
        2,
        Some(CursorRect {
            x: 10.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        None,
        Some(0),
        None,
        0.0,
        20.0,
        0.0,
        20.0,
        1,
        LayoutRevision::initial(),
        // text=true, caret=true, coordinated=true
        true,
        true,
        true,
    );
    assert!(
        key.is_none(),
        "coordinated=true 且 cursor_visual_track 为 None 时必须返回 None（不允许文字 unit 单独留下继续播放）"
    );
    assert!(
        coord.prepared_queue.active_transactions().is_empty(),
        "coordinated=true 且 cursor_visual_track 为 None 时不应 enqueue 任何事务"
    );
}

/// Issue #756 评论 5822051193: coordinated=true 的 composition commit/cancel，
/// 最终无法建立 cursor_visual_track（new_cursor_rect=None）时，
/// 必须返回 None 且不 enqueue 任何 crossfade/Reflow 文字 transaction。
#[test]
fn issue756_comment5822051193_composition_commit_coordinated_no_cursor_track_returns_none() {
    let sid_preedit = ShapingIdentity {
        text_content_hash: 99,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 99,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_after = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let old_snapshot = make_test_snapshot(
        "abc_preedit_after",
        vec![
            (0, 3, 10.0, 0.0, sid_after.clone()),
            (3, 10, 50.0, 0.0, sid_preedit.clone()),
            (10, 15, 120.0, 0.0, sid_after.clone()),
        ],
    );
    let new_snapshot = make_test_snapshot(
        "abc_QQ_after",
        vec![
            (0, 3, 10.0, 0.0, sid_after.clone()),
            (3, 5, 50.0, 0.0, sid_after.clone()),
            (5, 10, 80.0, 0.0, sid_after),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // coordinated=true, text=true, caret=true
    // new_cursor_rect=None, prepared_handoff=None
    // 队列为空 → take_rebase_frames 返回 (vec![], None) → caret_handoff=None
    // build_cursor_visual_track(new_cursor_rect=None) → 返回 None
    // → cursor_visual_track 为 None → 门禁触发 → 返回 None
    let key = coord.handle_composition_commit_or_cancel(
        &old_snapshot,
        &new_snapshot,
        3,
        10,
        true,
        false,
        3,
        5,
        3,
        10,
        Some(CursorRect {
            x: 50.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        None,
        Some(0),
        None,
        0.0,
        20.0,
        0.0,
        20.0,
        1,
        LayoutRevision::initial(),
        Instant::now(),
        None,
        // text=true, caret=true, coordinated=true
        true,
        true,
        true,
    );
    assert!(
        key.is_none(),
        "coordinated=true 且 cursor_visual_track 为 None 时必须返回 None（commit/cancel 路径同样收口）"
    );
    assert!(
        coord.prepared_queue.active_transactions().is_empty(),
        "coordinated=true 且 cursor_visual_track 为 None 时不应 enqueue 任何事务"
    );
}

/// Issue #756 评论 5822051193: coordinated=true 的 composition commit，
/// 有 caret_handoff 且 new_cursor_rect 存在时，能成功建立 cursor_visual_track，
/// 必须返回 Some，避免把合法 rebase 场景误杀。
#[test]
fn issue756_comment5822051193_composition_commit_coordinated_with_handoff_creates_transaction() {
    let sid = issue756_shaping_identity();
    let sid_preedit = ShapingIdentity {
        text_content_hash: 99,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 99,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_commit = ShapingIdentity {
        text_content_hash: 200,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 300,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };

    // 步骤 1: 先创建活跃的 composition update 事务（coordinated=true，有 old/new cursor rect）
    // update: "ab" → "axb"（在 1 处插入 preedit "x"）
    let update_old = make_test_snapshot(
        "ab",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid.clone()),
        ],
    );
    let update_new = make_test_snapshot(
        "axb",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid_preedit.clone()),
            (2, 3, 20.0, 0.0, sid.clone()),
        ],
    );
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let cursor_owner_epoch = 1u64;
    let update_key = coord.handle_composition_update(
        &update_old,
        &update_new,
        1,
        1,
        1,
        2,
        Some(CursorRect {
            x: 10.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 20.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        cursor_owner_epoch,
        LayoutRevision::initial(),
        // text=true, caret=true, coordinated=true
        true,
        true,
        true,
    );
    assert!(
        update_key.is_some(),
        "前置条件: coordinated=true 且有 old/new cursor rect 的 composition update 必须创建事务"
    );

    // 步骤 2: 调 prepare_composition_commit_handoff 产生 handoff
    // commit: "axb" → "aYb"（preedit "x" commit 成 "Y"）
    let commit_old = update_new.clone();
    let commit_new = make_test_snapshot(
        "aYb",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid_commit.clone()),
            (2, 3, 20.0, 0.0, sid),
        ],
    );
    let now = Instant::now();
    let prepared_handoff = coord.prepare_composition_commit_handoff(
        &commit_old,
        &commit_new,
        1,
        2,
        true,
        1,
        2,
        1,
        2,
        cursor_owner_epoch,
        now,
    );
    // 旧 composition update 事务在队列里，is_composition() 为 true → conflicting 非空
    // 旧事务有 old/new cursor rect → sample_coordinated_cursor_rect_at 返回 Some
    // cursor_owner_epoch 一致 → caret_handoff 被选中 → Some
    assert!(
        prepared_handoff.caret_handoff.is_some(),
        "前置条件: 有活跃 composition update 事务且 cursor_owner_epoch 一致时必须采到 caret_handoff"
    );

    // 步骤 3: 调 handle_composition_commit_or_cancel（is_commit=true, prepared_handoff=Some）
    // new_cursor_rect=Some → build_cursor_visual_track 用 handoff 分支返回 Some
    // → cursor_visual_track 为 Some → 门禁不触发 → 返回 Some
    let key = coord.handle_composition_commit_or_cancel(
        &commit_old,
        &commit_new,
        1,
        2,
        true,
        false,
        1,
        2,
        1,
        2,
        Some(CursorRect {
            x: 10.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(CursorRect {
            x: 20.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }),
        Some(0),
        Some(0),
        0.0,
        20.0,
        0.0,
        20.0,
        cursor_owner_epoch,
        LayoutRevision::initial(),
        now,
        Some(prepared_handoff),
        // text=true, caret=true, coordinated=true
        true,
        true,
        true,
    );
    assert!(
        key.is_some(),
        "coordinated=true 且有 caret_handoff 且 new_cursor_rect=Some 时必须返回 Some（合法 rebase 场景不能误杀）"
    );
    let tx = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key.unwrap())
        .unwrap();
    assert!(
        tx.cursor_visual_track.is_some(),
        "有 handoff 且 new_cursor_rect=Some 时必须成功建立 cursor_visual_track"
    );
}
