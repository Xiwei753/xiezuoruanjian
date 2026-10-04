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
use crate::sujian_editor_item::layout_snapshot::LineSnapshotId;
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
        base_text: String::new(),
        target_text: String::new(),
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
        base_text: String::new(),
        target_text: String::new(),
        now,
    });

    // Issue #826 评论 4 问题 3：吞字本身不裁 canonical（正文本来就该是删完的结果）。
    // clip 只可能来自 Reflow 层 —— 删除会让未改的字移动，Reflow 接管期间必须把
    // 它们在 canonical 的最终位置挖掉，否则重影。
    let frontier_hidden = coord
        .sample_edit_frontier(now)
        .map(|sample| coord.hidden_canonical_rects_for(&sample))
        .unwrap_or_default();
    assert!(
        frontier_hidden.is_empty(),
        "吞字前沿不应裁 canonical，实际 {} 个",
        frontier_hidden.len()
    );
    let expected_reflow_clips = coord.reflow_target_clip_rects().len();
    let plan = build(&coord, now);
    assert_eq!(
        plan.clip_rects.len(),
        expected_reflow_clips,
        "clip 只能来自 Reflow 目标位置"
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
            base_text: String::new(),
            target_text: String::new(),
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
        base_text: String::new(),
        target_text: String::new(),
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
        base_text: String::new(),
        target_text: String::new(),
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

// ── Issue #826 评论 5：merge_clip_rects 的两条回归 ──────────────────────────

/// 同一行的两个 clip 之间有 gap 时，绝不能合成一个大区间把中间的正常正文挖掉。
///
/// 旧实现无条件做 min/max 合并，x 10..20 与 x 40..50 会变成 x 10..50。
#[test]
fn merge_clip_rects_keeps_gap_between_intervals() {
    let id = LineSnapshotId::new(0, 0, 0);
    let merged = super::merge_clip_rects(vec![
        (10.0, 0.0, 10.0, 20.0, id),
        (40.0, 0.0, 10.0, 20.0, id),
    ]);
    assert_eq!(
        merged.len(),
        2,
        "同一行两个有 gap 的 clip 必须保持两条，实际合并成 {} 条",
        merged.len()
    );
    let mut xs: Vec<f64> = merged.iter().map(|r| r.0).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    assert!(
        (xs[0] - 10.0).abs() < 1e-9,
        "第一条起点应是 10，实际 {}",
        xs[0]
    );
    assert!(
        (xs[1] - 40.0).abs() < 1e-9,
        "第二条起点应是 40，实际 {}",
        xs[1]
    );
}

/// 同一 y / h 但 snapshot_id 不同的 clip 不得合并。
///
/// 合并并挂到某个 snapshot_id 上会破坏 renderer 的纹理缺失回退规则：
/// snapshot A 纹理存在、snapshot B 纹理缺失时，B 那块静态正文会被误裁，
/// 而 B 的动画 glyph 又画不出来，直接出现空洞。
#[test]
fn merge_clip_rects_never_crosses_snapshot_id() {
    let id_a = LineSnapshotId::new(0, 0, 1);
    let id_b = LineSnapshotId::new(0, 0, 2);
    // 刻意让两条区间重叠，如果忽略 snapshot_id 就会被合成一条。
    let merged = super::merge_clip_rects(vec![
        (10.0, 0.0, 20.0, 20.0, id_a),
        (15.0, 0.0, 20.0, 20.0, id_b),
    ]);
    assert_eq!(
        merged.len(),
        2,
        "不同 snapshot_id 的 clip 不得合并，实际 {} 条",
        merged.len()
    );
    assert!(
        merged.iter().all(|r| r.4 == id_a || r.4 == id_b),
        "合并结果必须保留各自的 snapshot_id"
    );
}

/// 相交 / 相邻的同组 clip 仍然要真正合并成一条（正向断言）。
#[test]
fn merge_clip_rects_merges_overlapping_same_group() {
    let id = LineSnapshotId::new(0, 0, 3);
    let merged = super::merge_clip_rects(vec![
        (10.0, 0.0, 20.0, 20.0, id),
        (25.0, 0.0, 10.0, 20.0, id),
    ]);
    assert_eq!(
        merged.len(),
        1,
        "相交区间必须合并成一条，实际 {} 条",
        merged.len()
    );
    assert!((merged[0].0 - 10.0).abs() < 1e-9, "合并后起点应是 10");
    assert!(
        (merged[0].2 - 25.0).abs() < 1e-9,
        "合并后宽度应到 35，实际 {}",
        merged[0].2
    );
}

/// 多行 clip 每行都要各自做区间合并（不能只处理第一行）。
#[test]
fn merge_clip_rects_processes_every_band() {
    let id = LineSnapshotId::new(0, 0, 4);
    let merged = super::merge_clip_rects(vec![
        (10.0, 0.0, 10.0, 20.0, id),
        (15.0, 0.0, 10.0, 20.0, id),
        (10.0, 40.0, 10.0, 20.0, id),
        (15.0, 40.0, 10.0, 20.0, id),
    ]);
    assert_eq!(
        merged.len(),
        2,
        "两行各合并成一条，实际 {} 条",
        merged.len()
    );
    let mut bands: Vec<f64> = merged.iter().map(|r| r.1).collect();
    bands.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    assert!((bands[0] - 0.0).abs() < 1e-9, "第一行 y 应是 0");
    assert!((bands[1] - 40.0).abs() < 1e-9, "第二行 y 应是 40");
}
