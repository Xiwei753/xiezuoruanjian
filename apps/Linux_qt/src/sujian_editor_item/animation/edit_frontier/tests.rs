//! Issue #826: 遮罩前沿单元测试。
//!
//! 覆盖：前沿采样 / extend 不产生第二个对象 / 吐字裁剪矩形 / 吞字 overlay 矩形 /
//! 连续编辑只更新同一对象。

use std::time::{Duration, Instant};

use crate::sujian_editor_item::animation::edit_frontier::{
    EditFrontierKind, EditFrontierState, FrontierRect,
};
use crate::sujian_editor_item::edit_motion::CursorRect;

fn rect(x: f64, top: f64, bottom: f64) -> CursorRect {
    CursorRect {
        x,
        top,
        bottom,
        baseline_y: bottom,
    }
}

fn instant_at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

#[test]
fn frontier_sample_progresses_from_zero_to_one() {
    let now = Instant::now();
    let state = EditFrontierState::begin_insert(
        crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot::new(
            crate::editor::layout::LayoutSnapshot::empty_for_tests(),
            Vec::new(),
            None,
            None,
            crate::editor::layout::CaretAffinity::Downstream,
        ),
        (0, 1),
        rect(0.0, 0.0, 20.0),
        rect(100.0, 0.0, 20.0),
        now,
        100,
    );
    assert_eq!(state.sample(now).progress, 0.0);
    assert_eq!(state.sample(instant_at(now, 100)).progress, 1.0);
    let mid = state.sample(instant_at(now, 50)).progress;
    assert!(mid > 0.0 && mid < 1.0, "中点进度必须在 (0,1)，实际 {mid}");
}

#[test]
fn frontier_is_finished_only_after_full_duration() {
    let now = Instant::now();
    let state = EditFrontierState::begin_insert(
        crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot::new(
            crate::editor::layout::LayoutSnapshot::empty_for_tests(),
            Vec::new(),
            None,
            None,
            crate::editor::layout::CaretAffinity::Downstream,
        ),
        (0, 1),
        rect(0.0, 0.0, 20.0),
        rect(100.0, 0.0, 20.0),
        now,
        160,
    );
    assert!(!state.is_finished(instant_at(now, 80)));
    assert!(state.is_finished(instant_at(now, 160)));
    assert!(state.is_finished(instant_at(now, 500)));
}

#[test]
fn extend_insert_keeps_same_object_and_retargets_from_sampled_position() {
    let now = Instant::now();
    let empty = crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot::new(
        crate::editor::layout::LayoutSnapshot::empty_for_tests(),
        Vec::new(),
        None,
        None,
        crate::editor::layout::CaretAffinity::Downstream,
    );
    let mut state = EditFrontierState::begin_insert(
        empty.clone(),
        (0, 1),
        rect(0.0, 0.0, 20.0),
        rect(100.0, 0.0, 20.0),
        now,
        160,
    );
    // 半程采样，作为新的起点。
    let half = instant_at(now, 80);
    let sampled = state.sample(half);
    assert!(sampled.frontier.x > 0.0 && sampled.frontier.x < 100.0);

    state.extend_insert(empty, (0, 2), rect(200.0, 0.0, 20.0), half);

    // 起点必须是被采样的前沿，不是原始起点。
    assert!((state.start_frontier.x - sampled.frontier.x).abs() < 1e-9);
    // 目标是最新的。
    assert_eq!(state.target_frontier.x, 200.0);
    // 范围已更新。
    assert_eq!(state.new_range, Some((0, 2)));
    // started_at 重置到本轮起点。
    assert!(state.started_at >= half);
}

#[test]
fn extend_delete_keeps_original_base_snapshot_and_unions_old_range() {
    let now = Instant::now();
    let base = crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot::new(
        crate::editor::layout::LayoutSnapshot::empty_for_tests(),
        Vec::new(),
        None,
        None,
        crate::editor::layout::CaretAffinity::Downstream,
    );
    let target = crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot::new(
        crate::editor::layout::LayoutSnapshot::empty_for_tests(),
        Vec::new(),
        None,
        None,
        crate::editor::layout::CaretAffinity::Downstream,
    );
    let mut state = EditFrontierState::begin_delete(
        base.clone(),
        target.clone(),
        (5, 6),
        rect(50.0, 0.0, 20.0),
        rect(40.0, 0.0, 20.0),
        now,
        160,
    );
    let half = instant_at(now, 80);
    state.extend_delete(base, target, (4, 5), rect(30.0, 0.0, 20.0), half);

    // base_snapshot 必须保持 burst 开始前的旧正文。
    assert_eq!(state.base_snapshot.line_snapshots.len(), 0);
    // old_range 在 base 坐标系里合并。
    assert_eq!(state.old_range, Some((4, 6)));
}

#[test]
fn kind_can_extend_only_with_same_kind() {
    assert!(EditFrontierKind::Insert.can_extend(EditFrontierKind::Insert));
    assert!(!EditFrontierKind::Insert.can_extend(EditFrontierKind::Delete));
    assert!(!EditFrontierKind::Delete.can_extend(EditFrontierKind::Replace));
}

#[test]
fn kind_flags_match_mask_and_overlay_needs() {
    assert!(EditFrontierKind::Insert.needs_new_mask());
    assert!(!EditFrontierKind::Insert.needs_old_overlay());
    assert!(EditFrontierKind::Delete.needs_old_overlay());
    assert!(!EditFrontierKind::Delete.needs_new_mask());
    assert!(EditFrontierKind::Replace.needs_new_mask());
    assert!(EditFrontierKind::Replace.needs_old_overlay());
}

#[test]
fn degenerate_frontier_rect_is_detected() {
    assert!(FrontierRect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 10.0
    }
    .is_degenerate());
    assert!(!FrontierRect {
        x: 0.0,
        y: 0.0,
        w: 5.0,
        h: 10.0
    }
    .is_degenerate());
}
