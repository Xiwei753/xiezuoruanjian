//! Issue #826: 每帧 RenderPlan 构建的单元测试。
//!
//! 覆盖 RenderPlan 的三层输出语义：
//! - 无前沿时静态正文层完整显示（`clip_rects` 为空）；
//! - 吐字前沿只通过 `clip_rects` 裁掉 canonical 里还没露出的新字，不额外画 overlay；
//! - 吞字前沿只通过 `text_animation.glyphs` 画旧正文 overlay；
//! - 光标完全独立，只透传 `CursorRenderState`。

use std::time::{Duration, Instant};

use writer_core::editor::OffsetMap;

use crate::editor::layout::{CaretAffinity, LayoutSnapshot};
use crate::sujian_editor_item::animation::coordinator::{
    EditFrontierRequest, LinuxEditorAnimationCoordinator,
};
use crate::sujian_editor_item::edit_motion::{CursorRect, EditorAnimationKind};
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, PreparedLineSnapshot, ShapingIdentity, SourceRect,
};
use crate::sujian_editor_item::render_plan::{
    CursorRenderState, CursorStyle, RenderPlan, SelectionPreeditPlan, SelectionPreeditStyle,
};

fn shaping() -> ShapingIdentity {
    ShapingIdentity {
        text_content_hash: 1,
        raw_font_fingerprint: String::from("test-font"),
        glyph_indexes_hash: 1,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 1,
    }
}

fn cluster(byte_start: usize, byte_end: usize, x: f64) -> LineClusterSnapshot {
    LineClusterSnapshot {
        byte_start,
        byte_end,
        source_rect: SourceRect {
            x,
            y: 0.0,
            w: 10.0,
            h: 20.0,
        },
        shaping_identity: shaping(),
    }
}

fn snapshot(lines: Vec<PreparedLineSnapshot>) -> EditorLayoutSnapshot {
    EditorLayoutSnapshot::new(
        LayoutSnapshot::empty_for_tests(),
        lines,
        None,
        None,
        CaretAffinity::Downstream,
    )
}

fn caret_rect(x: f64) -> CursorRect {
    CursorRect {
        x,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    }
}

fn cursor_state() -> CursorRenderState {
    CursorRenderState {
        visible: true,
        x: 12.5,
        y: 3.5,
        h: 19.0,
        opacity: 1.0,
    }
}

fn build(coord: &LinuxEditorAnimationCoordinator, frame_now: Instant) -> RenderPlan {
    coord.build_render_plan_full(
        cursor_state(),
        SelectionPreeditPlan::default(),
        CursorStyle::default(),
        SelectionPreeditStyle::default(),
        frame_now,
    )
}

/// 无前沿时静态正文层完整显示：没有任何 clip，也没有任何 overlay glyph。
#[test]
fn without_frontier_static_body_is_untouched() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let plan = build(&coord, now);

    assert!(
        plan.clip_rects.is_empty(),
        "无前沿时不应裁掉 canonical 正文，实际 {} 个 clip",
        plan.clip_rects.len()
    );
    assert!(
        plan.text_animation.glyphs.is_empty(),
        "无前沿时不应画 overlay/reflow glyph，实际 {} 个",
        plan.text_animation.glyphs.len()
    );
}

/// `drawn_caret_rect` 纯透传 `CursorRenderState` —— 光标与文字动画解耦。
#[test]
fn drawn_caret_rect_mirrors_cursor_render_state() {
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let plan = build(&coord, Instant::now());
    let (x, y, h) = plan.drawn_caret_rect.expect("drawn_caret_rect 已设");
    assert!((x - 12.5).abs() < 1e-9, "x 应透传，实际 {x}");
    assert!((y - 3.5).abs() < 1e-9, "y 应透传，实际 {y}");
    assert!((h - 19.0).abs() < 1e-9, "h 应透传，实际 {h}");
}

/// 吐字前沿只裁静态层，不画旧正文 overlay。
#[test]
fn insert_frontier_clips_new_text_without_overlay() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let target = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
    )]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: target.clone(),
        target_snapshot: target,
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        start_frontier: caret_rect(10.0),
        target_frontier: caret_rect(20.0),
        offset_map: OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
    });

    // 动画刚开始：新字还没露出来，必须裁掉。
    let plan = build(&coord, now);
    assert!(
        !plan.clip_rects.is_empty(),
        "吐字第一帧必须裁掉还没露出的新字"
    );
    assert!(
        plan.text_animation.glyphs.is_empty(),
        "吐字只画最新 canonical 一份，不该有 overlay glyph，实际 {} 个",
        plan.text_animation.glyphs.len()
    );

    // 动画结束：遮罩全开，静态层完整显示。
    let done = build(&coord, now + Duration::from_millis(200));
    assert!(
        done.clip_rects.is_empty(),
        "吐字结束后不应再裁静态层，实际 {} 个",
        done.clip_rects.len()
    );
}

/// 吞字前沿画旧正文 overlay 把删掉的字逐步收掉，静态层不裁。
#[test]
fn delete_frontier_draws_old_overlay_only() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
    )]);
    let target = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0)],
    )]);
    // Backspace 场景：old caret 在 "ab" 的位置 1（x=10），删掉的是它左边的
    // cluster "a"（byte 0..1，占 x 0..10）。所以遮罩从被删文字的右边缘
    // （x=10）往左收到左边缘（x=0），期间用旧正文 overlay 顶着。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: base,
        target_snapshot: target,
        deleted_ranges: vec![(0, 1)],
        inserted_ranges: Vec::new(),
        start_frontier: caret_rect(10.0),
        target_frontier: caret_rect(0.0),
        offset_map: OffsetMap::from_single_edit(2, (0, 1), 0),
        now,
    });

    let plan = build(&coord, now);
    assert!(
        plan.clip_rects.is_empty(),
        "吞字不裁 canonical（正文本来就该是删完的结果），实际 {} 个",
        plan.clip_rects.len()
    );
    assert!(
        !plan.text_animation.glyphs.is_empty(),
        "吞字第一帧必须还画着旧正文 overlay"
    );

    // 遮罩收完之后旧正文 overlay 消失。
    //
    // 注意此时 Reflow 层可能还在播（未改的字确实因为删除而移动了），所以
    // 只能断言 overlay 那一路为空，不能断言整层 glyph 为空。
    let done_at = now + Duration::from_millis(200);
    let done_sample = coord
        .sample_edit_frontier(done_at)
        .expect("前沿尚未被 finish 前仍可采样");
    assert!(
        done_sample.progress >= 1.0,
        "200ms 后前沿必须走完，实际 progress={}",
        done_sample.progress
    );
    assert!(
        coord.old_overlay_glyphs_for(&done_sample).is_empty(),
        "遮罩收完之后不该残留旧正文 overlay"
    );
}

/// 连续输入只更新同一个前沿对象，不生成第二个历史动画。
#[test]
fn consecutive_insert_keeps_one_frontier_object() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let empty = snapshot(Vec::new());

    for (idx, x) in [10.0f64, 20.0].into_iter().enumerate() {
        let _ = idx;
        let target = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0)],
        )]);
        coord.begin_or_extend_edit_frontier(EditFrontierRequest {
            kind: EditorAnimationKind::Insert,
            base_snapshot: empty.clone(),
            target_snapshot: target,
            deleted_ranges: Vec::new(),
            inserted_ranges: vec![(0, 1)],
            start_frontier: caret_rect(x - 10.0),
            target_frontier: caret_rect(x),
            offset_map: OffsetMap::from_single_edit(0, (0, 0), 1),
            now: now + Duration::from_millis(u64::try_from(idx).unwrap_or(0)),
        });
    }

    assert!(
        coord.has_active_edit_frontier(),
        "连续输入后应仍有且只有一个前沿对象"
    );
    assert_eq!(
        coord.active_edit_frontier_kind(),
        Some(crate::sujian_editor_item::animation::EditFrontierKind::Insert),
        "连续输入前沿种类保持 Insert"
    );
}

/// 前沿结束后 `has_active_text_animation` 归零，静态正文层接管。
#[test]
fn finished_frontier_releases_text_animation() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let target = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0)],
    )]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: target.clone(),
        target_snapshot: target,
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(0, 1)],
        start_frontier: caret_rect(0.0),
        target_frontier: caret_rect(10.0),
        offset_map: OffsetMap::from_single_edit(0, (0, 0), 1),
        now,
    });
    assert!(
        coord.has_active_text_animation(now),
        "刚开始时应有正文动画在跑"
    );
    assert!(
        !coord.has_active_text_animation(now + Duration::from_millis(500)),
        "时长过去后正文动画必须结束"
    );
}

/// `finish_edit_frontier_to_canonical` 让遮罩立刻落到 canonical 终态。
#[test]
fn finish_edit_frontier_snaps_to_canonical() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let target = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
    )]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: target.clone(),
        target_snapshot: target,
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        start_frontier: caret_rect(10.0),
        target_frontier: caret_rect(20.0),
        offset_map: OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
    });
    coord.finish_edit_frontier_to_canonical();

    let plan = build(&coord, now);
    assert!(!coord.has_active_edit_frontier(), "收口后不应还有前沿");
    assert!(
        plan.clip_rects.is_empty(),
        "收口后静态层必须完整显示，实际 {} 个 clip",
        plan.clip_rects.len()
    );
}

#[test]
fn layout_snapshot_test_helper_is_reachable() {
    // 防止 `LayoutRevision` import 在本文件变成未使用。
    let _ = LayoutRevision::next();
}
