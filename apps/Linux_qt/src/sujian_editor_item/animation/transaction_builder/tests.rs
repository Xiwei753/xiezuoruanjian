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
        None,
        None,
        None,
        None,
        0.0,
        0.0,
        0.0,
        0.0,
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
        None,
        None,
        None,
        None,
        0.0,
        0.0,
        0.0,
        0.0,
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
        None,
        None,
        None,
        None,
        0.0,
        0.0,
        0.0,
        0.0,
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
        None,
        None,
        None,
        None,
        0.0,
        0.0,
        0.0,
        0.0,
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
        None,
        None,
        None,
        None,
        0.0,
        0.0,
        0.0,
        0.0,
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
        None,
        None,
        None,
        None,
        0.0,
        0.0,
        0.0,
        0.0,
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
    let text_animation_enabled = coordinated_animation_enabled || typing_animation_enabled;
    let caret_animation_enabled = coordinated_animation_enabled || smooth_cursor_enabled;
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
        unit_duration_ms: 100,
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
        duration_ms: 100,
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
        duration_ms: 100,
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
