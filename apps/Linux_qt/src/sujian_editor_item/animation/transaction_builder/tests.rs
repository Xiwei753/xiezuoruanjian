use super::super::coordinator::LinuxEditorAnimationCoordinator;
use super::*;
use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::animation::{
    PreparedTextVisualTransaction, TextVisualOperationKind, VisualUnitTiming,
};
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_snapshot::{LineSnapshotId, ShapingIdentity, SourceRect};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
use writer_core::editor::OffsetMap;

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
    // Issue #808 评论 5918236360 问题4: Composition 多字候选按 visual_line_id
    // 合并为行级共同 extent。candidate range [3,5) 的 slice 与 [5,8) 的 slice
    // 合并后 byte range 变成 [3,8)，测试应检查合并后的 slice 覆盖 candidate range。
    let new_candidate_slices: Vec<&AnimatedSlice> = tx
        .units
        .iter()
        .filter(|u| u.slice.kind == AnimatedSliceKind::InsertReveal)
        .filter(|u| u.slice.byte_start <= 3 && u.slice.byte_end >= 5)
        .map(|u| &u.slice)
        .collect();
    assert!(
        !new_candidate_slices.is_empty(),
        "candidate range [3,5) should have animated slice (merged to byte range covering [3,5))"
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
    // Issue #808: new_cursor_rect 是吞字遮罩锚点，本测试只验证收拢方向，传 None。
    let slices = build_delete_conceal_slices(
        key,
        &old_snapshot,
        (0, 3),
        Some(&old_cursor),
        None,
        false,
        None,
    );
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
    // Issue #808: new_cursor_rect 是吞字遮罩锚点，本测试只验证收拢方向，传 None。
    let slices = build_delete_conceal_slices(
        key,
        &old_snapshot,
        (0, 3),
        Some(&old_cursor),
        None,
        false,
        None,
    );
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
    // Issue #756 评论 5821042551 / Issue #785: 文字与光标各自独立 duration。
    // 协同只表示同事务/同首帧/同 rebase，不共享 duration。
    let actual_text_duration = text_duration_ms;
    let actual_caret_duration = caret_duration_ms;
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
    let tx = build_prepared_transaction(issue756_insert_spec(key, true, false, false, true))
        .expect("issue756 transaction must be built");
    assert_eq!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal),
        1,
        "coordinated=true: 必须建立 InsertReveal（文字与光标各有独立 timeline）"
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
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, true, false, true))
        .expect("issue756 transaction must be built");
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
        tx.units.iter().all(|u| !u.timing.is_caret_driven()),
        "coordinated=false: 吞吐字用 Timed timing，不消费 caret frame"
    );
}

/// 只开平滑光标（coordinated=false, typing=false）：caret track 照建，没有文字动画。
#[test]
fn issue756_smooth_only_creates_caret_track_without_text_animation() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, false, true, true))
        .expect("issue756 transaction must be built");
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
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, true, true, false))
        .expect("issue756 transaction must be built");
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
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, false, false, true))
        .expect("issue756 transaction must be built");
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
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, true, true, true))
        .expect("issue756 transaction must be built");
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) > 0,
        "typing=true: 必须有 InsertReveal（文字动画）"
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "smooth=true 且有 caret rect: 必须有 cursor_visual_track（光标动画）"
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
///（text=true, caret=true, coordinated=true）。
/// Issue #815 评论 6042062633 修改 4: 协同吞吐字切 `CaretTrack`，由本事务的
/// cursor track 当前帧驱动；Reflow 仍是独立 `Timed`。
#[test]
fn issue756_ime_coordinated_only() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, true, false, false, true))
        .expect("issue756 transaction must be built");
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) > 0,
        "coordinated=true: 必须有 InsertReveal"
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "coordinated=true: 必须有 caret track"
    );
    assert!(
        tx.units
            .iter()
            .filter(|u| u.timing.is_caret_track())
            .count()
            > 0,
        "Issue #815: coordinated=true: InsertReveal/DeleteConceal 必须切 CaretTrack，\
         由 cursor track 当前帧驱动"
    );
    assert!(
        tx.units.iter().any(|u| {
            matches!(
                u.slice.kind,
                AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade
            ) && !u.timing.is_caret_track()
        }) || !tx.units.iter().any(|u| matches!(
            u.slice.kind,
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade
        )),
        "Issue #815: coordinated=true: Reflow 必须保持独立 Timed，不被 caret track 接管"
    );
}

/// coordinated=false + typing=true + smooth=false：只有文字动画
///（text=true, caret=false, coordinated=false，吞吐字 Timed）。
#[test]
fn issue756_ime_typing_only() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, true, false, true))
        .expect("issue756 transaction must be built");
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) > 0,
        "typing=true: 必须有 InsertReveal"
    );
    assert!(
        tx.cursor_visual_track.is_none(),
        "smooth=false 且非协同: 没有 caret track"
    );
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
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, false, true, true))
        .expect("issue756 transaction must be built");
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) == 0,
        "typing=false 且非协同: 没有 InsertReveal"
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "smooth=true: 必须有 caret track"
    );
}

/// coordinated=false + typing=true + smooth=true：两个动画并行但不进入 coordinated ownership
///（text=true, caret=true, coordinated=false，吞吐字 Timed）。
#[test]
fn issue756_ime_typing_and_smooth_not_coordinated() {
    let key = VisualTransactionKey::new(1, 756);
    let tx = build_prepared_transaction(issue756_insert_spec(key, false, true, true, true))
        .expect("issue756 transaction must be built");
    assert!(
        issue756_count_kind(&tx, AnimatedSliceKind::InsertReveal) > 0,
        "typing=true: 必须有 InsertReveal"
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "smooth=true: 必须有 caret track"
    );
    assert!(
        tx.units.iter().all(|u| !u.timing.is_caret_driven()),
        "coordinated=false: 吞吐字用 Timed timing，不消费 caret frame"
    );
}

/// Issue #756: 事务创建条件不再把"两个独立开关同时开启"当协同。
///
/// - coordinated=true：文字与光标各有独立 timeline/easing/duration（不绑死），
///   要求有效 caret motion，否则整笔（含文字动画）不创建。
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
        key, false, true, true, true, 100, 300,
    ))
    .expect("issue756 transaction must be built");
    // 文字 unit 用 typing duration (100ms)
    // Issue #785: 所有 unit 都是 Timed。
    for unit in &tx.units {
        match unit.timing {
            VisualUnitTiming::Timed { duration_ms, .. } => {
                assert_eq!(
                    duration_ms, 100,
                    "非协同: Timed 文字 unit 必须用 typing duration (100ms)，不是 smooth (300ms)"
                );
            }
            // Issue #815: 非协同吞吐字保持独立 Timed；协同时才切 CaretTrack。
            VisualUnitTiming::CaretTrack { .. } => {
                panic!("非协同吞吐字不应由 caret track 驱动")
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
        key, false, true, true, true, 300, 100,
    ))
    .expect("issue756 transaction must be built");
    // 文字 unit 用 typing duration (300ms)
    // Issue #785: 所有 unit 都是 Timed。
    for unit in &tx.units {
        match unit.timing {
            VisualUnitTiming::Timed { duration_ms, .. } => {
                assert_eq!(
                    duration_ms, 300,
                    "非协同: Timed 文字 unit 必须用 typing duration (300ms)，不是 smooth (100ms)"
                );
            }
            // Issue #815: 非协同吞吐字保持独立 Timed；协同时才切 CaretTrack。
            VisualUnitTiming::CaretTrack { .. } => {
                panic!("非协同吞吐字不应由 caret track 驱动")
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

/// Issue #756 评论 5821042551 / Issue #815 评论 6042062633 修改 4: 协同时，
/// 吞吐字不再拥有自己的 duration —— 它由 cursor track 当前帧驱动；Reflow 仍然
/// 按 `text_duration_ms`（typing duration）独立播放，光标 track 仍用 smooth duration。
#[test]
fn issue756_comment5821042551_coordinated_shares_typing_duration() {
    let key = VisualTransactionKey::new(1, 756);
    // coordinated=true, typing=false, smooth=false, typing=100ms, smooth=300ms
    // Issue #785: 协同时 caret_duration_ms 始终独立（用 smooth=300ms），不再共享 typing=100ms。
    let tx = build_prepared_transaction(issue756_insert_spec_with_durations(
        key, true, false, false, true, 100, 300,
    ))
    .expect("issue756 transaction must be built");
    // Issue #815: 协同吞吐字切 CaretTrack（没有自己的 duration），Reflow 保持 Timed(typing)。
    for unit in &tx.units {
        match (&unit.slice.kind, &unit.timing) {
            (AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal, timing) => {
                assert!(
                    timing.is_caret_track(),
                    "Issue #815: 协同: 吞吐字必须是 CaretTrack（由 cursor track 当前帧驱动），                     不再有自己的 text_duration_ms 时间线"
                );
            }
            (AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade, timing) => {
                match timing {
                    VisualUnitTiming::Timed { duration_ms, .. } => assert_eq!(
                        *duration_ms, 100,
                        "Issue #815: 协同: Reflow 保持独立 Timed，用 typing duration (100ms)"
                    ),
                    VisualUnitTiming::CaretTrack { .. } => {
                        panic!("Issue #815: 协同: Reflow 不得由 caret track 驱动")
                    }
                }
            }
        }
    }
    // Issue #815: cursor_visual_track 用 smooth cursor duration (300ms)，始终独立。
    let track = tx
        .cursor_visual_track
        .as_ref()
        .expect("coordinated=true: 必须有 cursor_visual_track");
    assert_eq!(
        track.duration_ms, 300,
        "Issue #815: 协同: cursor_visual_track 用 smooth cursor duration (300ms)，始终独立，不共享 typing (100ms)"
    );
}

/// Issue #756 评论 5821042551: 非协同 smooth-only（typing=false, smooth=true）
/// 也必须有独立的 caret_duration_ms，不受 typing duration 影响。
#[test]
fn issue756_comment5821042551_smooth_only_has_independent_caret_duration() {
    let key = VisualTransactionKey::new(1, 756);
    // coordinated=false, typing=false, smooth=true, typing=100ms, smooth=300ms
    let tx = build_prepared_transaction(issue756_insert_spec_with_durations(
        key, false, false, true, true, 100, 300,
    ))
    .expect("issue756 transaction must be built");
    // smooth-only: 没有文字 unit
    assert!(tx.units.is_empty(), "smooth-only: 没有文字动画 unit");
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
        key, false, true, true, true, 100, 300,
    ))
    .expect("issue756 transaction must be built");

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
    // 旧事务有 old/new cursor rect → cursor track 可采样
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

// =============================================================================
// Issue #808 评论 5917296533 复现测试（本轮 4 个剩余问题）
// =============================================================================
//
// 每个测试展示当前代码的 bug 行为：测试通过 = bug 被成功复现。
// 修复后这些断言应反转（断言正确行为）或测试被替换为回归测试。
//
// 涉及文件：
//   - animated_slice.rs（compute_frame, insert_reveal, delete_conceal）
//   - animation/transaction_builder/slices.rs（build_*_slices, merge_adjacent_slices,
//     build_composition_commit_crossfade_slices）
//   - animation/transaction_builder.rs（create_transaction_from_prepared_handoff,
//     merge_adjacent_slices）
//   - pipeline.rs（inject_new_animation_visuals_with_diagnostics）

/// 问题 2: 前向 Delete 遮罩公式有 bug——文字可能几乎不缩。
///
/// `compute_frame()` 的 `conceal_to_left_edge=false` 分支：
/// ```text
/// left_boundary = anchor_x + (from_left - anchor_x) * visible
/// ```
/// 普通前向 Delete 时 `anchor_x == from_left`（新 caret 在被删字符左边），
/// 无论 visible 是 1 还是 0，`left_boundary` 都等于 `from_left`，
/// `frame_w = (from_right - left_boundary).max(0.0)` 基本不变，字不会真正吞掉。
///
/// Issue 指定测试用例：`from=[100,160], caret=100, visible=0` 时 frame.w 必须为 0。
/// Issue #808 评论 5917296533 修复后：is_caret_line=true 时整段遮罩向 caret 收拢，
/// 最终宽度归零。
#[test]
fn issue808_comment5917296533_problem2_forward_delete_conceal_mask_not_shrinking() {
    let key = VisualTransactionKey::new(1, 1);
    let snapshot_id = LineSnapshotId::new(1, 0, 0);
    // 被删区域 document rect [100,160]，source rect 与 document rect 一致（dpr=1, origin=0）
    let from_doc = SourceRect {
        x: 100.0,
        y: 0.0,
        w: 60.0,
        h: 20.0,
    };
    let source = SourceRect {
        x: 100.0,
        y: 0.0,
        w: 60.0,
        h: 20.0,
    };
    // 新 caret 在被删字符左边 (x=100) → Delete 键 → conceal_to_left_edge=false
    // Issue #808 评论 5917296533 问题4 修复后: 构造函数默认 is_caret_line=false，
    // 这里显式设为 true 来测试问题2的遮罩公式（caret 在删除区域一侧）。
    let mut slice = AnimatedSlice::delete_conceal(
        key,
        snapshot_id,
        source,
        from_doc,
        100.0, // cursor_x = 新 caret = 100 = from_left
        0.0,
        0,
        1,
        None,
        false, // conceal_to_left_edge = false（Delete 键，向右边缘收缩）
        Some(0),
    );
    slice.is_caret_line = true; // 模拟协同模式：caret 在删除区域一侧
                                // visible=0：文字应完全被吞掉，frame.w 应为 0
    let frame = slice.compute_frame(0.0);
    // 修复后正确行为：
    //   anchor_x = caret_anchor_x.clamp(100,160) = 100
    //   left_boundary = anchor_x = 100（固定在 caret 侧）
    //   right_boundary = 100 + (160 - 100) * 0 = 100（从 from_right 收向 anchor）
    //   frame_w = (100 - 100).max(0.0) = 0  ← 修复后正确
    assert!(
        frame.w.abs() < 0.5,
        "问题2 修复后：visible=0 时 frame.w={}（应为 0），\
         前向 Delete 遮罩正确收缩，文字被完全吞掉",
        frame.w
    );
    // 同时验证 visible=1 时是 60（完全可见），证明遮罩动画有效
    let frame_full = slice.compute_frame(1.0);
    assert!(
        (frame_full.w - 60.0).abs() < 0.5,
        "问题2 修复后：visible=1 时 frame.w={}（应为 60），\
         遮罩完全打开，文字完全可见",
        frame_full.w
    );
    // 验证 visible 0→1 有显著变化（遮罩动画有效）
    assert!(
        (frame_full.w - frame.w).abs() > 50.0,
        "问题2 修复后：visible=0 时 w={} 与 visible=1 时 w={} 差异显著，\
         遮罩动画有效",
        frame.w,
        frame_full.w
    );
}

/// 问题 3（Issue #808 评论 5918236360）: Backspace 遮罩对称性。
///
/// Backspace 场景：extent=[100,160], old caret=160, new/final caret=100。
/// `build_delete_conceal_slices` 用 old caret 算 `conceal_to_left_edge`：
/// `(160-160).abs() <= (160-100).abs()` → `0 <= 60` → `true`。
/// 旧代码 is_caret_line=true 分支用 `conceal_to_left_edge` 选 true 分支：
/// `right_boundary=anchor_x=100, left_boundary=100-(100-100)*visible=100`
/// → visible=1 时 fw=0，文字从第一帧就完全不可见。
///
/// Issue #808 评论 5918236360 问题3 修复后：is_caret_line=true 分支不再用
/// `conceal_to_left_edge`（old caret 推出来的），改用 `anchor_x`（final caret）
/// 相对 deleted extent 中点决定收拢方向。final caret=100 在 [100,160] 左半
/// → 向左收 → visible=1 时 w=60（完整），visible=0 时 w=0（归零）。
#[test]
fn issue808_comment5918236360_problem3_backspace_conceal_mask_symmetric() {
    let key = VisualTransactionKey::new(1, 1);
    let snapshot_id = LineSnapshotId::new(1, 0, 0);
    // 被删区域 document rect [100,160]，source rect 与 document rect 一致（dpr=1, origin=0）
    let from_doc = SourceRect {
        x: 100.0,
        y: 0.0,
        w: 60.0,
        h: 20.0,
    };
    let source = SourceRect {
        x: 100.0,
        y: 0.0,
        w: 60.0,
        h: 20.0,
    };
    // Backspace: old caret=160（靠右）→ conceal_to_left_edge=true
    // new/final caret=100（靠左）→ anchor_x=100
    let mut slice = AnimatedSlice::delete_conceal(
        key,
        snapshot_id,
        source,
        from_doc,
        100.0, // caret_anchor_x = final/new caret = 100 = from_left
        0.0,
        0,
        1,
        None,
        true, // conceal_to_left_edge = true（old caret=160 推出来的）
        Some(0),
    );
    slice.is_caret_line = true; // 协同模式：caret 在删除区域一侧
                                // visible=1：文字应完全可见，frame.w 应为 60
    let frame_full = slice.compute_frame(1.0);
    assert!(
        (frame_full.w - 60.0).abs() < 0.5,
        "问题3 修复后：Backspace visible=1 时 frame.w={}（应为 60），\
         遮罩完全打开，文字完全可见",
        frame_full.w
    );
    // visible=0：文字应完全被吞掉，frame.w 应为 0
    let frame = slice.compute_frame(0.0);
    assert!(
        frame.w.abs() < 0.5,
        "问题3 修复后：Backspace visible=0 时 frame.w={}（应为 0），\
         遮罩完全收拢，文字被完全吞掉",
        frame.w
    );
    // 验证 visible 0→1 有显著变化（遮罩动画有效，不是从第一帧就不可见）
    assert!(
        (frame_full.w - frame.w).abs() > 50.0,
        "问题3 修复后：Backspace visible=0 时 w={} 与 visible=1 时 w={} 差异显著，\
         遮罩动画有效，不是从第一帧就完全不可见",
        frame.w,
        frame_full.w
    );
}

/// 问题 1→2: InsertReveal 丢失链——空格输入 + caret 动画开启时保留 cursor-only 事务。
///
/// 当 inserted_range 只含空格时，`build_insert_reveal_slices` 跳过空格（whitespace 跳过）
/// 返回空 slices，`prepared_tx.units` 为空。Issue #808 评论 5918236360 问题2 修复后：
/// `create_transaction_from_prepared_handoff` 不再仅按 `units.is_empty()` 丢弃事务。
/// 空格是合法的非可见输入，本来就可以没有 InsertReveal；当 `caret_animation_enabled=true`
/// 且有合法 `cursor_visual_track` 时，必须保留 cursor-only 事务（有光标 track，units 可以为空）。
#[test]
fn issue808_comment5917296533_problem1_empty_transaction_still_created_for_whitespace() {
    use crate::sujian_editor_item::edit_motion::{EditorAnimationKind, PreparedEditMotion};
    use writer_core::editor::{EditorCursor, EditorSelection, Utf8ByteRange};

    let sid = issue756_shaping_identity();
    // new 文本为一个空格，cluster [0,1)。build_insert_reveal_slices 跳过空格 → 空 slices
    let old_snapshot = make_test_snapshot("", vec![]);
    let new_snapshot = make_test_snapshot(" ", vec![(0, 1, 0.0, 0.0, sid)]);
    let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);

    let mut coord = LinuxEditorAnimationCoordinator::new();
    let prepared = PreparedRebaseHandoff::Insert {
        rebase_frames: vec![],
        caret_handoff: None,
        range_start: 0,
        range_end: 1, // " " 一个空格
        insert_offset_map: offset_map,
        visual_affected_byte_range_old: Some((0, 0)),
        visual_affected_byte_range_new: Some((0, 1)),
    };
    let vt = PreparedEditMotion {
        kind: EditorAnimationKind::Insert,
        inserted_range: Some(Utf8ByteRange::from_ordered(0, 1)),
        deleted_range: None,
        old_text: "".to_string(),
        new_text: " ".to_string(),
        text_duration_ms: 100,
        caret_duration_ms: 100,
        old_selection: EditorSelection {
            anchor: EditorCursor::new("", 0),
            head: EditorCursor::new("", 0),
        },
        new_selection: EditorSelection {
            anchor: EditorCursor::new(" ", 1),
            head: EditorCursor::new(" ", 1),
        },
        old_cursor_rect: None,
        new_cursor_rect: None,
    };
    let old_cursor = CursorRect {
        x: 0.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    let new_cursor = CursorRect {
        x: 10.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    // coordinated=false, text=true, caret=true
    let key = coord.create_transaction_from_prepared_handoff(
        Some(prepared),
        &vt,
        true,  // text_animation_enabled
        true,  // caret_animation_enabled
        false, // coordinated_animation_enabled
        Some(old_cursor),
        Some(new_cursor),
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
    // Issue #808 评论 5918236360 问题2 修复后：空格输入 + caret_animation_enabled=true
    // 产生 cursor-only 事务（有 cursor_visual_track，units 可以为空），key 为 Some。
    assert!(
        key.is_some(),
        "问题2 修复后：inserted_range 只含空格 + caret_animation_enabled=true → \
         build_insert_reveal_slices 返回空 → prepared_tx.units 为空，但有合法 \
         cursor_visual_track，create_transaction_from_prepared_handoff 保留 \
         cursor-only 事务，返回 Some(key)"
    );
    // 确认 cursor-only 事务被 enqueue：units 为空但 cursor_visual_track 为 Some
    let active = coord.prepared_queue.active_transactions();
    assert!(
        !active.is_empty(),
        "问题2 修复后：空格 inserted_range + caret 动画应产生 cursor-only 事务，\
         prepared_queue 不应为空"
    );
    let tx = &active[0];
    assert!(
        tx.units.is_empty(),
        "问题2 修复后：空格输入不产生 InsertReveal，units 应为空（实际 {} 个）",
        tx.units.len()
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "问题2 修复后：caret_animation_enabled=true + 有 new_cursor_rect → \
         必须有 cursor_visual_track，保留 cursor-only 事务"
    );
}

/// 问题 3（Issue #808 评论 5917296533 → 评论 5919641249 修改 1）: 同一行共用一条
/// 行级吞吐边界。
///
/// 旧实现：`can_merge` 对 InsertReveal 放宽合并条件，同一 `visual_line_id` 且同 y
/// 即可合并，靠 union `source_rect` 成一个大 slice 来表达"行级共同 extent"。
/// 这会在中间夹着保留字符时把保留字符一起画进动画层。
///
/// 修复后：空格断开的两段各自保留自己的 slice（真实 source/document rect 和
/// byte range），只通过 `line_mask_left/right` 共享同一条行级 boundary；
/// `compute_frame` 用行级 boundary 与各自 rect 求交。visible 递增时只有一条
/// 边界从左向右扫过，不会出现"多个字块同时冒出来"。
#[test]
fn issue808_comment5919641249_problem1_space_separated_shared_boundary_without_union() {
    let sid = issue756_shaping_identity();
    // "abc def"：cluster 划分 abc[0,3) 空格[3,4) def[4,7)
    let new_snapshot = make_test_snapshot(
        "abc def",
        vec![
            (0, 3, 0.0, 0.0, sid.clone()),
            (3, 4, 30.0, 0.0, sid.clone()),
            (4, 7, 40.0, 0.0, sid),
        ],
    );
    let key = VisualTransactionKey::new(1, 1);
    // inserted_range = (0,7) 整行输入
    // coordinated=false → is_caret_line 全为 false（行首展开）
    let slices = build_insert_reveal_slices(
        key,
        &new_snapshot,
        (0, 7),
        None,  // old_cursor_rect
        false, // coordinated
        None,  // caret_visual_line_id
    );
    let reveal_slices: Vec<_> = slices
        .iter()
        .filter(|s| s.kind == AnimatedSliceKind::InsertReveal)
        .collect();
    // 修复后：空格 [3,4) 被跳过，abc[0,3) 和 def[4,7) 是两个 slice，
    // 不再 union 成一个大 slice。
    assert_eq!(
        reveal_slices.len(),
        2,
        "修复后：\"abc def\" 输入时空格虽断开 byte range，但两段仍各自保留自己的 \
         slice（不再 union 成一个大矩形），实际 {} 个",
        reveal_slices.len()
    );
    // 每个 slice 保留自己真实的 byte range 和 rect（中间空格 30..40 不被覆盖）
    assert!(
        reveal_slices[0].byte_start == 0 && reveal_slices[0].byte_end == 3,
        "第一段应保留自己的 byte range [0,3)，实际 [{},{})",
        reveal_slices[0].byte_start,
        reveal_slices[0].byte_end
    );
    assert!(
        (reveal_slices[0].to_document_rect.x - 0.0).abs() < 0.5
            && (reveal_slices[0].to_document_rect.w - 30.0).abs() < 0.5,
        "第一段 rect 应为 [0,30]，实际 x={} w={}",
        reveal_slices[0].to_document_rect.x,
        reveal_slices[0].to_document_rect.w
    );
    assert!(
        reveal_slices[1].byte_start == 4 && reveal_slices[1].byte_end == 7,
        "第二段应保留自己的 byte range [4,7)，实际 [{},{})",
        reveal_slices[1].byte_start,
        reveal_slices[1].byte_end
    );
    assert!(
        (reveal_slices[1].to_document_rect.x - 40.0).abs() < 0.5
            && (reveal_slices[1].to_document_rect.w - 30.0).abs() < 0.5,
        "第二段 rect 应为 [40,70]，实际 x={} w={}",
        reveal_slices[1].to_document_rect.x,
        reveal_slices[1].to_document_rect.w
    );
    // 两段共享同一条行级 boundary [0,70]
    for s in &reveal_slices {
        assert!(
            (s.line_mask_left - 0.0).abs() < 0.5 && (s.line_mask_right - 70.0).abs() < 0.5,
            "两段必须共享行级共同 boundary [0,70]，实际 [{},{}]",
            s.line_mask_left,
            s.line_mask_right
        );
    }
    // 单一 boundary 从左向右扫过：
    // visible=0.4 → boundary=28：第一段部分显示（28/30），第二段完全未出现。
    let left_at_04 = reveal_slices[0].compute_frame(0.4);
    let right_at_04 = reveal_slices[1].compute_frame(0.4);
    assert!(
        (left_at_04.w - 28.0).abs() < 0.5,
        "visible=0.4 时第一段应显示出 boundary 前的 28 宽，实际 w={}",
        left_at_04.w
    );
    assert!(
        right_at_04.w.abs() < 0.5,
        "visible=0.4 时第二段必须完全不可见（boundary 还没扫到 40），实际 w={}",
        right_at_04.w
    );
    // visible=0.6 → boundary=42：第一段完整（30），第二段只显示出前 2（boundary-40）。
    let left_at_06 = reveal_slices[0].compute_frame(0.6);
    let right_at_06 = reveal_slices[1].compute_frame(0.6);
    assert!(
        (left_at_06.w - 30.0).abs() < 0.5,
        "visible=0.6 时第一段应完整显示（w=30），实际 w={}",
        left_at_06.w
    );
    assert!(
        (right_at_06.w - 2.0).abs() < 0.5,
        "visible=0.6 时第二段只应显示出 boundary 扫过的前 2 宽，实际 w={}",
        right_at_06.w
    );
}

/// 问题 4: Composition 路径统一协同模式参数。
///
/// Issue #808 评论 5917296533 修复后：`build_composition_commit_crossfade_slices` 接收
/// `coordinated` 参数，coordinated=false 时 InsertReveal/DeleteConceal 的 is_caret_line
/// 为 false（独立文字动画语义），不再偷偷进入协同 caret mask 模式。
///
/// 本测试构造 IME commit 场景（old preedit 空 → new candidate "b"），
/// 验证 coordinated=false 时生成的 InsertReveal slice 的 is_caret_line 为 false。
#[test]
fn issue808_comment5917296533_problem4_composition_bypasses_coordinated_mode_insert() {
    let sid_new = ShapingIdentity {
        text_content_hash: 808,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 808,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    // old preedit 为空，new candidate 为 "b"（cluster [0,1)）
    let old_snapshot = make_test_snapshot("", vec![]);
    let new_snapshot = make_test_snapshot("b", vec![(0, 1, 0.0, 0.0, sid_new)]);
    let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
    let key = VisualTransactionKey::new(1, 1);
    let old_cursor = CursorRect {
        x: 0.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    let new_cursor = CursorRect {
        x: 10.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    // preedit 范围 [0,0)（空），candidate 范围 [0,1)（"b"）
    // Issue #808 评论 5917296533 问题4: coordinated=false → is_caret_line 应为 false
    let slices = build_composition_commit_crossfade_slices(
        key,
        &old_snapshot,
        &new_snapshot,
        &offset_map,
        0,
        0,
        0,
        1,
        Some(&old_cursor),
        Some(&new_cursor),
        false, // coordinated = false
        None,  // caret_visual_line_id
    );
    let insert_reveals: Vec<_> = slices
        .iter()
        .filter(|s| s.kind == AnimatedSliceKind::InsertReveal)
        .collect();
    assert!(
        !insert_reveals.is_empty(),
        "应至少产生一个 InsertReveal slice（new candidate \"b\" 在 old 中无匹配）"
    );
    // 问题 4 修复后：coordinated=false 时 Composition InsertReveal is_caret_line 为 false
    for (i, s) in insert_reveals.iter().enumerate() {
        assert!(
            !s.is_caret_line,
            "问题4 修复后：coordinated=false 时 Composition commit 生成的 InsertReveal[{}] \
             is_caret_line={}（应为 false），只走独立文字动画语义，不偷偷进入协同 caret mask 模式",
            i, s.is_caret_line
        );
    }
}

/// 问题 4（DeleteConceal 侧）：IME cancel 时 old preedit "a" → new 空，
/// coordinated=false 时生成的 DeleteConceal slice 的 is_caret_line 为 false。
#[test]
fn issue808_comment5917296533_problem4_composition_bypasses_coordinated_mode_delete() {
    let sid_old = ShapingIdentity {
        text_content_hash: 809,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 809,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    // old preedit "a"（cluster [0,1)），new candidate 为空（cancel）
    let old_snapshot = make_test_snapshot("a", vec![(0, 1, 0.0, 0.0, sid_old)]);
    let new_snapshot = make_test_snapshot("", vec![]);
    let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
    let key = VisualTransactionKey::new(1, 1);
    let old_cursor = CursorRect {
        x: 10.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    let new_cursor = CursorRect {
        x: 0.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    // preedit 范围 [0,1)（"a"），candidate 范围 [0,0)（空，cancel）
    // Issue #808 评论 5917296533 问题4: coordinated=false → is_caret_line 应为 false
    let slices = build_composition_commit_crossfade_slices(
        key,
        &old_snapshot,
        &new_snapshot,
        &offset_map,
        0,
        1,
        0,
        0,
        Some(&old_cursor),
        Some(&new_cursor),
        false, // coordinated = false
        None,  // caret_visual_line_id
    );
    let delete_conceals: Vec<_> = slices
        .iter()
        .filter(|s| s.kind == AnimatedSliceKind::DeleteConceal)
        .collect();
    assert!(
        !delete_conceals.is_empty(),
        "应至少产生一个 DeleteConceal slice（old preedit \"a\" 在 new 中无匹配）"
    );
    // 问题 4 修复后：coordinated=false 时 Composition DeleteConceal is_caret_line 为 false
    for (i, s) in delete_conceals.iter().enumerate() {
        assert!(
            !s.is_caret_line,
            "问题4 修复后：coordinated=false 时 Composition cancel 生成的 DeleteConceal[{}] \
             is_caret_line={}（应为 false），只走独立文字动画语义，不偷偷进入协同 caret mask 模式",
            i, s.is_caret_line
        );
    }
}

/// 问题 4（Issue #808 评论 5918236360 → 评论 5919641249 修改 1）: Composition
/// 多字候选按 visual_line_id 形成行级共同 mask 边界。
///
/// 中文 IME 一次上屏多 cluster（如 "abc" 三个字）时，每个 InsertReveal 都拿同一个
/// caret anchor（insert_cx）。旧实现把同一 visual_line_id 的多个 cluster union 成
/// 一个大 slice；修复后每个 cluster 保留自己的 slice/rect，只共享行级共同 boundary。
#[test]
fn issue808_comment5918236360_problem4_composition_multichar_shared_line_mask() {
    let sid = ShapingIdentity {
        text_content_hash: 810,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 810,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    // old preedit 为空，new candidate 为 "abc"（3 个 cluster [0,1) [1,2) [2,3)）
    // 同一 visual_line_id=0，同一 y=0.0
    let old_snapshot = make_test_snapshot("", vec![]);
    let new_snapshot = make_test_snapshot(
        "abc",
        vec![
            (0, 1, 0.0, 0.0, sid.clone()),
            (1, 2, 10.0, 0.0, sid.clone()),
            (2, 3, 20.0, 0.0, sid),
        ],
    );
    let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
    let key = VisualTransactionKey::new(1, 1);
    let old_cursor = CursorRect {
        x: 0.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    let new_cursor = CursorRect {
        x: 30.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    // preedit 范围 [0,0)（空），candidate 范围 [0,3)（"abc"）
    // coordinated=true → is_caret_line=true（caret 所在行）
    let slices = build_composition_commit_crossfade_slices(
        key,
        &old_snapshot,
        &new_snapshot,
        &offset_map,
        0,
        0,
        0,
        3,
        Some(&old_cursor),
        Some(&new_cursor),
        true,    // coordinated = true
        Some(0), // caret_visual_line_id = 0
    );
    let insert_reveals: Vec<_> = slices
        .iter()
        .filter(|s| s.kind == AnimatedSliceKind::InsertReveal)
        .collect();
    // 修复后：3 个 cluster 各自保留自己的 InsertReveal slice（不再 union 成 1 个）
    assert_eq!(
        insert_reveals.len(),
        3,
        "修复后：\"abc\" 3 个 cluster 同 visual_line_id 仍各自保留自己的 InsertReveal \
         slice（实际 {} 个），只共享行级 boundary，不再 union 成一个大纹理块",
        insert_reveals.len()
    );
    // 每个 slice 保留自己真实的 rect
    for (i, s) in insert_reveals.iter().enumerate() {
        let expected_x = i as f64 * 10.0;
        assert!(
            (s.to_document_rect.x - expected_x).abs() < 0.5
                && (s.to_document_rect.w - 10.0).abs() < 0.5,
            "第 {} 个 cluster 应保留自己的 rect x={} w=10，实际 x={} w={}",
            i,
            expected_x,
            s.to_document_rect.x,
            s.to_document_rect.w
        );
        assert!(
            (s.source_rect.x - expected_x).abs() < 0.5 && (s.source_rect.w - 10.0).abs() < 0.5,
            "第 {} 个 cluster 应保留自己的 source rect x={} w=10，实际 x={} w={}",
            i,
            expected_x,
            s.source_rect.x,
            s.source_rect.w
        );
    }
    // 三个 cluster 共享同一条行级共同 boundary [0,30]（caret 在行首 → 从左向右扫）
    for s in &insert_reveals {
        assert!(
            (s.line_mask_left - 0.0).abs() < 0.5 && (s.line_mask_right - 30.0).abs() < 0.5,
            "3 个 cluster 必须共享行级共同 boundary [0,30]，实际 [{},{}]",
            s.line_mask_left,
            s.line_mask_right
        );
    }
    // 单一 boundary 扫过：visible=0.5 → boundary=15
    //   cluster0 [0,10] 完整、cluster1 [10,20] 显示前 5、cluster2 [20,30] 未出现。
    let f0 = insert_reveals[0].compute_frame(0.5);
    let f1 = insert_reveals[1].compute_frame(0.5);
    let f2 = insert_reveals[2].compute_frame(0.5);
    assert!(
        (f0.w - 10.0).abs() < 0.5,
        "visible=0.5 时 cluster0 应完整显示（w=10），实际 w={}",
        f0.w
    );
    assert!(
        (f1.w - 5.0).abs() < 0.5,
        "visible=0.5 时 cluster1 应只显示 boundary 扫过的前 5，实际 w={}",
        f1.w
    );
    assert!(
        f2.w.abs() < 0.5,
        "visible=0.5 时 cluster2 必须完全不可见，实际 w={}",
        f2.w
    );
    // visible=1.0 时三个 cluster 都完整（终态不丢字）
    for (i, s) in insert_reveals.iter().enumerate() {
        let frame = s.compute_frame(1.0);
        assert!(
            (frame.w - 10.0).abs() < 0.5,
            "visible=1.0 时 cluster{} 应完整显示（w=10），实际 w={}",
            i,
            frame.w
        );
    }
}

/// Issue #808 评论 5919641249 修改 1 回归: 混合候选（中间夹保留字符）不得被
/// union 大矩形覆盖。
///
/// old = `abcde` → new = `axcye`（替换 b→x、d→y；中间 c 保留）。
/// x / y 是新的候选字符 → InsertReveal；中间 c 的 shaping/位置没变 →
/// 不生成动画 slice，继续由 canonical 静态正文画。
///
/// 修复前：x、y 两个 InsertReveal 在 `Vec` 里相邻，`can_merge()` 把它们合成一个
/// 从 x 到 y 的大 `source_rect`，union 区域把中间的 c 像素一起画进动画层
///（而 `static_hidden_document_rects` 只隐藏 x、y 自己的 rect，不隐藏 c）→
/// 动画层与 canonical 静态层都把 c 画一遍，变粗/重影。
///
/// 修复后：x / y 各自保留自己的 source rect，任何动画 frame/source rect 都不能
/// 覆盖中间保留的 c；两者只共享同一条行级 boundary。
#[test]
fn issue808_comment5919641249_problem1_mixed_candidates_do_not_cover_retained_cluster() {
    let sid_common = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_candidate_x = ShapingIdentity {
        text_content_hash: 810,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 810,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let sid_candidate_y = ShapingIdentity {
        text_content_hash: 811,
        raw_font_fingerprint: "font".into(),
        glyph_indexes_hash: 811,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    // old = abcde：a[0,1) b[1,2) c[2,3) d[3,4) e[4,5)，每 cluster 宽 10
    let old_snapshot = make_test_snapshot(
        "abcde",
        vec![
            (0, 1, 0.0, 0.0, sid_common.clone()),
            (1, 2, 10.0, 0.0, sid_common.clone()),
            (2, 3, 20.0, 0.0, sid_common.clone()),
            (3, 4, 30.0, 0.0, sid_common.clone()),
            (4, 5, 40.0, 0.0, sid_common.clone()),
        ],
    );
    // new = axcye：x[1,2) y[3,4) 是新候选；a/c/e 保留（同 shaping、同几何）
    let new_snapshot = make_test_snapshot(
        "axcye",
        vec![
            (0, 1, 0.0, 0.0, sid_common.clone()),
            (1, 2, 10.0, 0.0, sid_candidate_x),
            (2, 3, 20.0, 0.0, sid_common.clone()),
            (3, 4, 30.0, 0.0, sid_candidate_y),
            (4, 5, 40.0, 0.0, sid_common),
        ],
    );
    // 两段 inserted range [1,2) 与 [3,4)（x / y），中间 c[2,3) 不改动。
    // 整笔编辑走唯一的事务构造入口 build_prepared_transaction：
    // 两个 range 各自建出 InsertReveal，事务级 assign_shared_line_masks 把同一
    // visual_line_id 的两段并成同一条行级 boundary。
    let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
    let old_cursor = CursorRect {
        x: 10.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    let new_cursor = CursorRect {
        x: 40.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    let spec = VisualEditSpec {
        key: VisualTransactionKey::new(1, 1),
        operation_kind: TextVisualOperationKind::Insert,
        old_snapshot,
        new_snapshot,
        inserted_ranges: vec![(1, 2), (3, 4)],
        deleted_ranges: vec![],
        offset_map,
        old_cursor_rect: Some(old_cursor),
        new_cursor_rect: Some(new_cursor),
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
        visual_affected_byte_range_old: Some((0, 5)),
        visual_affected_byte_range_new: Some((0, 5)),
        text_duration_ms: 100,
        caret_duration_ms: 100,
        text_animation_enabled: true,
        caret_animation_enabled: true,
        coordinated_animation_enabled: true,
        composition_commit_crossfade: None,
    };
    let tx = build_prepared_transaction(spec).expect("issue756 transaction must be built");

    // 中间保留的 c 的 rect 区间 (20,30)：任何动画 source/frame rect 都不得覆盖它。
    let covers_retained_c = |x: f64, w: f64| -> bool {
        let right = x + w;
        x < 30.0 - 0.5 && right > 20.0 + 0.5
    };

    let insert_reveals: Vec<_> = tx
        .units
        .iter()
        .filter(|u| u.slice.kind == AnimatedSliceKind::InsertReveal)
        .map(|u| &u.slice)
        .collect();
    assert_eq!(
        insert_reveals.len(),
        2,
        "x / y 两个新候选应各产生一个 InsertReveal，实际 {} 个",
        insert_reveals.len()
    );
    // x / y 各自保留自己的 source rect，都不覆盖中间保留的 c
    for (i, s) in insert_reveals.iter().enumerate() {
        assert!(
            !covers_retained_c(s.source_rect.x, s.source_rect.w),
            "InsertReveal[{}] 的 source_rect 不得覆盖中间保留的 c：x={} w={}",
            i,
            s.source_rect.x,
            s.source_rect.w
        );
        assert!(
            !covers_retained_c(s.to_document_rect.x, s.to_document_rect.w),
            "InsertReveal[{}] 的 to_document_rect 不得覆盖中间保留的 c：x={} w={}",
            i,
            s.to_document_rect.x,
            s.to_document_rect.w
        );
        for (j, rect) in s.static_hidden_document_rects.iter().enumerate() {
            assert!(
                !covers_retained_c(rect.x, rect.w),
                "InsertReveal[{}] 的 static_hidden_document_rects[{}] 不得覆盖中间保留的 c：\
                 x={} w={}",
                i,
                j,
                rect.x,
                rect.w
            );
        }
    }
    // x / y 共享同一条行级 boundary [10,40]
    for (i, s) in insert_reveals.iter().enumerate() {
        assert!(
            (s.line_mask_left - 10.0).abs() < 0.5 && (s.line_mask_right - 40.0).abs() < 0.5,
            "InsertReveal[{}] 应与另一候选共享行级 boundary [10,40]，实际 [{},{}]",
            i,
            s.line_mask_left,
            s.line_mask_right
        );
    }
    // 任取若干 visible：每个动画帧 rect 都不得覆盖中间保留的 c
    for visible in [0.0, 0.25, 0.5, 0.75, 1.0] {
        for (i, s) in insert_reveals.iter().enumerate() {
            let frame = s.compute_frame(visible);
            assert!(
                !covers_retained_c(frame.x, frame.w),
                "visible={} 时 InsertReveal[{}] 的 frame [x={} w={}] 不得覆盖中间保留的 c",
                visible,
                i,
                frame.x,
                frame.w
            );
        }
    }
    // 共享 boundary 语义：anchor=old caret=10（行首侧）→ 从左向右扫。
    // visible=0.5 → boundary=25：x 完整（10），y 未出现（其左边缘 30 > 25）。
    // visible=0.75 → boundary=32.5：y 从自己的左边缘开始显示前 2.5。
    let x_half = insert_reveals[0].compute_frame(0.5);
    let y_half = insert_reveals[1].compute_frame(0.5);
    assert!(
        (x_half.w - 10.0).abs() < 0.5,
        "visible=0.5 时 x 应完整显示（w=10），实际 w={}",
        x_half.w
    );
    assert!(
        y_half.w.abs() < 0.5,
        "visible=0.5 时 y 必须完全不可见（boundary 未扫到），实际 w={}",
        y_half.w
    );
    let y_late = insert_reveals[1].compute_frame(0.75);
    assert!(
        (y_late.x - 30.0).abs() < 0.5 && (y_late.w - 2.5).abs() < 0.5,
        "visible=0.75 时 y 应从自己的左边缘 30 显示前 2.5，实际 x={} w={}",
        y_late.x,
        y_late.w
    );
}

/// 问题 2（Issue #808 评论 5918236360）: 非协同 smooth-only（typing=false, smooth=true）
/// 的普通可见字符 Insert 也必须保留 cursor-only 事务。
///
/// 只开平滑光标时 `text_animation_enabled=false`，`units` 天然为空（没有 InsertReveal/
/// Reflow），但 `caret_animation_enabled=true` 且 old/new caret rect 都存在，
/// `cursor_visual_track` 是合法的。旧实现按 `units.is_empty()` 直接 return None，
/// 把平滑光标一起丢掉，只能 canonical snap。
#[test]
fn issue808_comment5918236360_problem2_smooth_only_insert_keeps_cursor_only_transaction() {
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
    let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
    let prepared = PreparedRebaseHandoff::Insert {
        rebase_frames: vec![],
        caret_handoff: None,
        range_start: 1,
        range_end: 2,
        insert_offset_map: offset_map,
        visual_affected_byte_range_old: Some((0, 2)),
        visual_affected_byte_range_new: Some((0, 3)),
    };
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
    let old_cursor = CursorRect {
        x: 10.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    let new_cursor = CursorRect {
        x: 20.0,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    };
    // coordinated=false, text=false, caret=true（smooth-only）
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let key = coord.create_transaction_from_prepared_handoff(
        Some(prepared),
        &vt,
        false, // text_animation_enabled
        true,  // caret_animation_enabled
        false, // coordinated_animation_enabled
        Some(old_cursor),
        Some(new_cursor),
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
        "smooth-only 普通可见字符 Insert：units 为空但有合法 cursor_visual_track，\
         必须保留 cursor-only 事务（不能按 units.is_empty() 直接丢弃）"
    );
    let active = coord.prepared_queue.active_transactions();
    assert_eq!(active.len(), 1, "cursor-only 事务必须被 enqueue");
    let tx = &active[0];
    assert!(
        tx.units.is_empty(),
        "typing=false 且非协同：没有文字动画 unit（实际 {:?}）",
        unit_kind_labels(&tx.units)
    );
    assert!(
        tx.cursor_visual_track.is_some(),
        "smooth=true：cursor-only 事务必须带 cursor_visual_track"
    );
}

/// 问题 2（Issue #808 评论 5918236360）: 真正的空事务（既没有文字 unit 也没有
/// cursor_visual_track）仍然不 enqueue，返回 None。
#[test]
fn issue808_comment5918236360_problem2_truly_empty_transaction_still_dropped() {
    use crate::sujian_editor_item::edit_motion::{EditorAnimationKind, PreparedEditMotion};
    use writer_core::editor::{EditorCursor, EditorSelection, Utf8ByteRange};

    let sid = issue756_shaping_identity();
    // 空格插入：没有任何可见 glyph，build_insert_reveal_slices 跳过。
    let old_snapshot = make_test_snapshot("", vec![]);
    let new_snapshot = make_test_snapshot(" ", vec![(0, 1, 0.0, 0.0, sid)]);
    let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
    let prepared = PreparedRebaseHandoff::Insert {
        rebase_frames: vec![],
        caret_handoff: None,
        range_start: 0,
        range_end: 1,
        insert_offset_map: offset_map,
        visual_affected_byte_range_old: Some((0, 0)),
        visual_affected_byte_range_new: Some((0, 1)),
    };
    let vt = PreparedEditMotion {
        kind: EditorAnimationKind::Insert,
        inserted_range: Some(Utf8ByteRange::from_ordered(0, 1)),
        deleted_range: None,
        old_text: "".to_string(),
        new_text: " ".to_string(),
        text_duration_ms: 100,
        caret_duration_ms: 100,
        old_selection: EditorSelection {
            anchor: EditorCursor::new("", 0),
            head: EditorCursor::new("", 0),
        },
        new_selection: EditorSelection {
            anchor: EditorCursor::new(" ", 1),
            head: EditorCursor::new(" ", 1),
        },
        old_cursor_rect: None,
        new_cursor_rect: None,
    };
    // coordinated=false, text=true, caret=false：没有文字 unit 也没有 caret track。
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let key = coord.create_transaction_from_prepared_handoff(
        Some(prepared),
        &vt,
        true,  // text_animation_enabled
        false, // caret_animation_enabled
        false, // coordinated_animation_enabled
        None,  // 没有 caret rect → 没有 cursor_visual_track
        None,
        Some(0),
        None,
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
        key.is_none(),
        "空格插入且没有 cursor_visual_track：真正的空事务仍然 return None"
    );
    assert!(
        coord.prepared_queue.active_transactions().is_empty(),
        "空事务不得 enqueue"
    );
}

/// 问题 3（Issue #808 评论 5918236360）: Backspace / 前向 Delete 通过真实 builder
/// 走 coordinated caret mask 时必须对称——visible=1 是完整 deleted extent，
/// visible=0 严格归零，且收拢方向由 final caret 决定（不是 old caret 推出来的
/// `conceal_to_left_edge`）。
///
/// 场景：被删 cluster document extent=[100,160]。
/// - Backspace：old caret=160（右侧）→ conceal_to_left_edge=true；final caret=100（左侧）。
/// - 前向 Delete：old caret=100（左侧）→ conceal_to_left_edge=false；final caret=100。
#[test]
fn issue808_comment5918236360_problem3_delete_conceal_mask_symmetric_via_builder() {
    let sid = issue756_shaping_identity();
    // cluster (0,6) → source_rect x=100 w=60 → document rect [100,160]
    let old_snapshot = make_test_snapshot("abcdef", vec![(0, 6, 100.0, 0.0, sid)]);
    let key = VisualTransactionKey::new(1, 808);

    let build_and_check = |old_caret_x: f64, label: &str| {
        let old_cursor = CursorRect {
            x: old_caret_x,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        };
        // final/new caret = 100（删除后落在 extent 左侧）
        let new_cursor = CursorRect {
            x: 100.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        };
        let slices = build_delete_conceal_slices(
            key,
            &old_snapshot,
            (0, 6),
            Some(&old_cursor),
            Some(&new_cursor),
            true,    // coordinated
            Some(0), // caret 在这一行
        );
        let delete_slices: Vec<&AnimatedSlice> = slices
            .iter()
            .filter(|s| s.kind == AnimatedSliceKind::DeleteConceal)
            .collect();
        assert_eq!(
            delete_slices.len(),
            1,
            "{}: deleted extent [100,160] 应产生 1 个 DeleteConceal",
            label
        );
        let slice = delete_slices[0];
        assert!(
            slice.is_caret_line,
            "{}: coordinated + caret 所在行，is_caret_line 应为 true",
            label
        );
        assert!(
            (slice.from_document_rect.x - 100.0).abs() < 0.5
                && (slice.from_document_rect.w - 60.0).abs() < 0.5,
            "{}: deleted extent 应为 [100,160]，实际 x={} w={}",
            label,
            slice.from_document_rect.x,
            slice.from_document_rect.w
        );
        let full = slice.compute_frame(1.0);
        assert!(
            (full.w - 60.0).abs() < 0.5,
            "{}: visible=1 必须是完整 deleted extent（w=60），实际 {}",
            label,
            full.w
        );
        let gone = slice.compute_frame(0.0);
        assert!(
            gone.w.abs() < 0.5,
            "{}: visible=0 必须严格归零（w=0），实际 {}",
            label,
            gone.w
        );
        // final caret 在 extent 左半 → 向左收：右边界向 final caret(100) 移动，
        // 左边界固定在 100。
        assert!(
            (gone.x - 100.0).abs() < 0.5,
            "{}: final caret 在左侧，可见区域左边界应固定在 caret（100），实际 {}",
            label,
            gone.x
        );
        let half = slice.compute_frame(0.5);
        assert!(
            half.x > 99.5 && half.x < 100.5 && half.w > 29.0 && half.w < 31.0,
            "{}: visible=0.5 时可见区域应从 caret 侧向右展开一半（约 [100,130]），实际 x={} w={}",
            label,
            half.x,
            half.w
        );
    };

    // Backspace：old caret=160（右侧）→ conceal_to_left_edge=true，但 final caret=100，
    // 旧公式会从第一帧就把被删字符完全遮掉。
    build_and_check(160.0, "Backspace(old caret=160)");
    // 前向 Delete：old caret=100（左侧）→ conceal_to_left_edge=false，final caret=100。
    build_and_check(100.0, "Delete(old caret=100)");
}
