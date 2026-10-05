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
use crate::sujian_editor_item::animation::edit_frontier::{ConcealDirection, EditFrontierKind};
use crate::sujian_editor_item::edit_motion::EditorAnimationKind;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::LineSnapshotId;
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, PreparedLineSnapshot, ShapingIdentity, SourceRect,
};
use crate::sujian_editor_item::qt_text_node::{
    merge_static_clip_rects, AnimationClipRect, StaticClipKind,
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
        offset_map: OffsetMap::from_single_edit(1, (1, 1), 1),
        base_text: String::new(),
        target_text: String::new(),
        conceal_direction: ConcealDirection::Forward,
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
        offset_map: OffsetMap::from_single_edit(2, (0, 1), 0),
        base_text: String::new(),
        target_text: String::new(),
        conceal_direction: ConcealDirection::Forward,
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

    for (idx, _x) in [10.0f64, 20.0].into_iter().enumerate() {
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
            offset_map: OffsetMap::from_single_edit(0, (0, 0), 1),
            base_text: String::new(),
            target_text: String::new(),
            conceal_direction: ConcealDirection::Forward,
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
        offset_map: OffsetMap::from_single_edit(0, (0, 0), 1),
        base_text: String::new(),
        target_text: String::new(),
        conceal_direction: ConcealDirection::Forward,
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
        offset_map: OffsetMap::from_single_edit(1, (1, 1), 1),
        base_text: String::new(),
        target_text: String::new(),
        conceal_direction: ConcealDirection::Forward,
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
/// 构造一条静态层 exclusion clip 的测试辅助。
fn mask_clip(x: f64, y: f64, w: f64, h: f64, id: LineSnapshotId) -> AnimationClipRect {
    AnimationClipRect {
        x,
        y,
        w,
        h,
        snapshot_id: id,
        kind: StaticClipKind::FrontierMask,
    }
}

fn reflow_clip(x: f64, y: f64, w: f64, h: f64, id: LineSnapshotId) -> AnimationClipRect {
    AnimationClipRect {
        x,
        y,
        w,
        h,
        snapshot_id: id,
        kind: StaticClipKind::ReflowTarget,
    }
}

/// 同一行的两个 clip 之间有 gap 时，绝不能合成一个大区间把中间的正常正文挖掉。
///
/// 旧实现无条件做 min/max 合并，x 10..20 与 x 40..50 会变成 x 10..50。
#[test]
fn merge_static_clip_rects_keeps_gap_between_intervals() {
    let id = LineSnapshotId::new(0, 0, 0);
    let merged = merge_static_clip_rects(vec![
        mask_clip(10.0, 0.0, 10.0, 20.0, id),
        mask_clip(40.0, 0.0, 10.0, 20.0, id),
    ]);
    assert_eq!(
        merged.len(),
        2,
        "同一行两个有 gap 的 clip 必须保持两条，实际合并成 {} 条",
        merged.len()
    );
    let mut xs: Vec<f64> = merged.iter().map(|r| r.x).collect();
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
fn merge_static_clip_rects_never_crosses_snapshot_id() {
    let id_a = LineSnapshotId::new(0, 0, 1);
    let id_b = LineSnapshotId::new(0, 0, 2);
    // 刻意让两条区间重叠，如果忽略 snapshot_id 就会被合成一条。
    let merged = merge_static_clip_rects(vec![
        reflow_clip(10.0, 0.0, 20.0, 20.0, id_a),
        reflow_clip(15.0, 0.0, 20.0, 20.0, id_b),
    ]);
    assert_eq!(
        merged.len(),
        2,
        "不同 snapshot_id 的 clip 不得合并，实际 {} 条",
        merged.len()
    );
    assert!(
        merged
            .iter()
            .all(|r| r.snapshot_id == id_a || r.snapshot_id == id_b),
        "合并结果必须保留各自的 snapshot_id"
    );
}

/// Issue #826 评论 6 阻塞 1：FrontierMask 与 ReflowTarget 语义不同，不得互相合并。
///
/// FrontierMask 不依赖动画纹理，永远必须裁；ReflowTarget 纹理 miss 时必须放行。
/// 两者一旦合成一块，renderer 就再也无法区分该保留哪一部分。
#[test]
fn merge_static_clip_rects_never_crosses_clip_kind() {
    let id = LineSnapshotId::new(0, 0, 7);
    let merged = merge_static_clip_rects(vec![
        mask_clip(10.0, 0.0, 20.0, 20.0, id),
        reflow_clip(12.0, 0.0, 20.0, 20.0, id),
    ]);
    assert_eq!(
        merged.len(),
        2,
        "不同 kind 的 clip 不得合并，实际 {} 条",
        merged.len()
    );
    assert!(
        merged
            .iter()
            .any(|r| r.kind == StaticClipKind::FrontierMask)
            && merged
                .iter()
                .any(|r| r.kind == StaticClipKind::ReflowTarget),
        "两种 kind 都必须原样保留"
    );
}

/// 相交 / 相邻的同组 clip 仍然要真正合并成一条（正向断言）。
#[test]
fn merge_static_clip_rects_merges_overlapping_same_group() {
    let id = LineSnapshotId::new(0, 0, 3);
    let merged = merge_static_clip_rects(vec![
        reflow_clip(10.0, 0.0, 20.0, 20.0, id),
        reflow_clip(25.0, 0.0, 10.0, 20.0, id),
    ]);
    assert_eq!(
        merged.len(),
        1,
        "相交区间必须合并成一条，实际 {} 条",
        merged.len()
    );
    assert!((merged[0].x - 10.0).abs() < 1e-9, "合并后起点应是 10");
    assert!(
        (merged[0].w - 25.0).abs() < 1e-9,
        "合并后宽度应到 35，实际 {}",
        merged[0].w
    );
}

/// 多行 clip 每行都要各自做区间合并（不能只处理第一行）。
#[test]
fn merge_static_clip_rects_processes_every_band() {
    let id = LineSnapshotId::new(0, 0, 4);
    let merged = merge_static_clip_rects(vec![
        mask_clip(10.0, 0.0, 10.0, 20.0, id),
        mask_clip(15.0, 0.0, 10.0, 20.0, id),
        mask_clip(10.0, 40.0, 10.0, 20.0, id),
        mask_clip(15.0, 40.0, 10.0, 20.0, id),
    ]);
    assert_eq!(
        merged.len(),
        2,
        "两行各合并成一条，实际 {} 条",
        merged.len()
    );
    let mut bands: Vec<f64> = merged.iter().map(|r| r.y).collect();
    bands.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    assert!((bands[0] - 0.0).abs() < 1e-9, "第一行 y 应是 0");
    assert!((bands[1] - 40.0).abs() < 1e-9, "第二行 y 应是 40");
}

/// Issue #826 评论 6 阻塞 1：纯 Insert 的 FrontierMask 不依赖动画纹理。
///
/// 吐字遮罩的语义是「canonical 正文自己画 + mask 只把还没露出的新字裁掉」，
/// 动画层根本不画 inserted glyph，所以不需要任何动画纹理。RenderPlan 里
/// 必须仍然产出这个 clip，renderer 也不会因为 texture_cache 为空而丢掉它。
#[test]
fn insert_frontier_mask_needs_no_animation_texture() {
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
        offset_map: OffsetMap::from_single_edit(1, (1, 1), 1),
        base_text: String::from("a"),
        target_text: String::from("ab"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    let plan = build(&coord, now);
    assert!(
        plan.clip_rects
            .iter()
            .all(|cr| !cr.requires_animation_texture()),
        "纯 Insert 不该产生需要动画纹理的 clip"
    );
    assert!(
        !plan.clip_rects.is_empty(),
        "纯 Insert 必须产出 FrontierMask（不依赖动画纹理）"
    );
    assert!(
        plan.clip_rects
            .iter()
            .all(|cr| cr.kind == StaticClipKind::FrontierMask),
        "纯 Insert 的 clip 只能是 FrontierMask"
    );
    assert!(
        plan.text_animation.glyphs.is_empty(),
        "吐字只画最新 canonical 一份，动画层不该有 glyph"
    );
}

/// Issue #826 评论 6 阻塞 3：FrontierMask 只能覆盖 inserted cluster。
///
/// 旧正文 `A|B`，中间插入 X 得 `AX|B`。target 行 A(unchanged) / X(inserted) /
/// B(unchanged + Reflow)。FrontierMask 越权裁掉 B 会和 Reflow 抢同一块区域。
#[test]
fn frontier_mask_covers_only_inserted_cluster() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // 一个 inserted cluster（X）+ 一个 unchanged suffix（B）在同一行、x 更大。
    let target = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![
            cluster(0, 1, 0.0),
            cluster(1, 2, 50.0),
            cluster(2, 3, 100.0),
        ],
    )]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: target.clone(),
        target_snapshot: target,
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        // 前沿起点在行首，progress=0 时新字还没露出，FrontierMask 必须遮住它。
        offset_map: OffsetMap::from_single_edit(2, (1, 1), 1),
        base_text: String::from("ab"),
        target_text: String::from("axb"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    let plan = build(&coord, now);
    let masks: Vec<&AnimationClipRect> = plan
        .clip_rects
        .iter()
        .filter(|cr| cr.kind == StaticClipKind::FrontierMask)
        .collect();
    assert!(
        !masks.is_empty(),
        "inserted cluster 必须被 FrontierMask 遮住"
    );
    // stub_for_tests: visual_x = 首 cluster 的 x = 0，dpr = 1，
    // doc x = cluster.x + visual_x，所以 x 即 cluster.x。
    // inserted cluster（byte 1..2）doc x 0..100；unchanged suffix B（byte 2..3）
    // doc x 100..110 归 Reflow 层管，FrontierMask 不得越界盖上去。
    for cr in &masks {
        assert!(
            cr.x < 100.0 && cr.x + cr.w <= 100.0,
            "FrontierMask 只能覆盖 inserted 那个 cluster（x 0..100），不能盖到 x>=100 的 unchanged suffix，实际 x={} w={}",
            cr.x,
            cr.w
        );
    }
    // unchanged suffix 确实需要移动时，应该由 ReflowTarget clip 接管，而不是 FrontierMask。
    assert!(
        plan.clip_rects
            .iter()
            .any(|cr| cr.kind == StaticClipKind::ReflowTarget && cr.x >= 100.0),
        "unchanged suffix 的最终位置应由 ReflowTarget clip 让位"
    );
}

/// Issue #826 评论 6 阻塞 2：Reflow 的 target clip 与移动中的 glyph 同时存在。
///
/// ReflowSpan 的 snapshot_id 指向最新 target 行；clip 让静态层在 canonical
/// 最终位置让位，glyph 在动画层画正在移动的那一份。两者必须同帧共存。
#[test]
fn reflow_target_clip_and_moving_glyph_coexist() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // 段中 Enter：old "ab" 单行 → new "a\nb" 两行。换行符本身没有 glyph，
    // 所以 inserted_ranges 只有那个换行位置；未改的 b 掉到第二行，由 Reflow 负责。
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 50.0)],
    )]);
    let target = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(1, 20.0, 0, vec![cluster(2, 3, 0.0)]),
    ]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: base.clone(),
        target_snapshot: target,
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        offset_map: OffsetMap::from_single_edit(2, (1, 1), 1),
        base_text: String::from("ab"),
        target_text: String::from("a\nb"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    let plan = build(&coord, now);
    assert!(
        plan.clip_rects
            .iter()
            .any(|cr| cr.kind == StaticClipKind::ReflowTarget),
        "Reflow 接管区必须在静态层生成 ReflowTarget clip"
    );
    assert!(
        !plan.text_animation.glyphs.is_empty(),
        "Reflow 移动中的 glyph 必须在动画层绘制"
    );
}

/// Issue #826 评论 6 阻塞 1：只有 ReflowTarget 需要动画纹理。
///
/// renderer 的守卫语义：FrontierMask 纹理 miss 也必须保留，
/// ReflowTarget 纹理 miss 才撤掉（让 canonical 同帧恢复）。
#[test]
fn clip_texture_requirement_depends_on_kind() {
    assert!(
        !mask_clip(0.0, 0.0, 10.0, 20.0, LineSnapshotId::new(0, 0, 0)).requires_animation_texture(),
        "FrontierMask 不依赖动画纹理"
    );
    assert!(
        reflow_clip(0.0, 0.0, 10.0, 20.0, LineSnapshotId::new(0, 0, 0))
            .requires_animation_texture(),
        "ReflowTarget 依赖动画纹理"
    );
}

/// Issue #826 评论 7 阻塞 1：某个 Reflow span 的 target 纹理缺失时，
/// 不能把整个 coordinator（连同与它完全无关的 FrontierMask）一起收掉。
///
/// 场景 `A|B -> AX|B`：X 的 FrontierMask 不需要任何动画纹理；
/// 若 B 的 Reflow 行纹理暂时拿不到，只有 B 的 ReflowTarget clip 应该被放弃。
/// 这里从 coordinator 侧验证：只有旧 overlay（Delete/Replace）才依赖 base 行，
/// 纯 Insert 前沿不声明任何动画纹理需求，Reflow 的 id 集合独立于前沿范围。
#[test]
fn insert_frontier_declares_no_old_overlay_textures() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let target = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 50.0)],
    )]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: target.clone(),
        target_snapshot: target,
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        offset_map: OffsetMap::from_single_edit(1, (1, 1), 1),
        base_text: String::from("a"),
        target_text: String::from("ab"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    // 纯 Insert 没有旧 overlay，所以没有任何旧行纹理需求 ——
    // 这正是 `prepare_frontier_textures` 不再一刀切收口的前提。
    assert!(
        coord.active_old_overlay_snapshot_ids().is_empty(),
        "纯 Insert 前沿不应声明任何旧 overlay 纹理需求"
    );
    assert!(
        coord.has_active_edit_frontier(),
        "纹理需求为空时前沿必须保持活跃"
    );

    // 换成 Delete：此时才应该声明 base 行纹理。
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 50.0)],
    )]);
    let after = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0)],
    )]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: base,
        target_snapshot: after,
        deleted_ranges: vec![(1, 2)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(2, (1, 2), 0),
        base_text: String::from("ab"),
        target_text: String::from("a"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });
    let overlay_ids = coord.active_old_overlay_snapshot_ids();
    assert!(
        !overlay_ids.is_empty(),
        "Delete 前沿必须声明 old overlay 需要的 base 行纹理"
    );
    assert_eq!(
        overlay_ids.len(),
        1,
        "只应声明 old_range 覆盖的行，而不是整份 base snapshot 的所有可见行"
    );
}

/// Issue #826 评论 11 阻塞 2（要求补的第二个测试）：当前编辑改掉了上一轮
/// 正在 Reveal 的那段文字时，coordinator 必须**开新 burst**，而不是让旧 track
/// 静默消失。
///
/// 反例（评论原文的两次 Undo `c -> b -> a`）：
/// - 第一笔 Undo `c -> b`：Replace，burst base = `c`，b 正在 Reveal。
/// - 动画未结束第二笔 Undo `b -> a`：b 既是「上一笔刚插出来的字」（映不回
///   burst base `c`），又是「这次要改掉的 reveal text」（映不到最新 target a）。
///   两处映射都失败。
/// - 旧代码：old 侧 `filter_map` 丢掉 → b 没有 ConcealTrack；new 侧
///   `continue` 丢掉 → b 的 RevealTrack 消失。**b 直接闪没。**
/// - 现在：`can_extend_identity` 判身份断裂 → 结束当前 burst、用本次
///   base/target 开新 burst，b 正常进 Conceal、a 正常进 Reveal。
#[test]
fn replace_opens_new_burst_when_old_reveal_text_is_edited_away() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let line = |vlid: usize, x: f64| {
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            vlid,
            0.0,
            0,
            vec![cluster(0, 1, x)],
        )])
    };

    // 第一笔 Undo：c -> b。base 里是 c，target 里是 b。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Replace,
        base_snapshot: line(0, 0.0),
        target_snapshot: line(1, 0.0),
        deleted_ranges: vec![(0, 1)],
        inserted_ranges: vec![(0, 1)],
        offset_map: OffsetMap::from_single_edit(1, (0, 1), 1),
        base_text: String::from("c"),
        target_text: String::from("b"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });
    assert!(
        coord.has_active_edit_frontier(),
        "第一笔 Undo 后必须有前沿（b 正在 Reveal、c 正在 Conceal）"
    );

    // 第二笔 Undo：b -> a。b 是上一笔刚插出来的，映不回 burst base `c`。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Replace,
        base_snapshot: line(1, 0.0),
        target_snapshot: line(2, 0.0),
        deleted_ranges: vec![(0, 1)],
        inserted_ranges: vec![(0, 1)],
        offset_map: OffsetMap::from_single_edit(1, (0, 1), 1),
        base_text: String::from("b"),
        target_text: String::from("a"),
        conceal_direction: ConcealDirection::Forward,
        now: now + Duration::from_millis(80),
    });

    let frontier = coord.sample_edit_frontier(now + Duration::from_millis(80));
    assert!(
        frontier.is_some(),
        "身份断裂后 coordinator 必须仍然持有一个活跃前沿（开新 burst），不能把动画对象丢掉"
    );
    // 新 burst 的 base 必须是 b（这样 b 能正常 Conceal），而不是原来的 c。
    let overlay = coord.old_overlay_glyphs_for(frontier.as_ref().expect("前沿存在"));
    assert!(
        !overlay.is_empty(),
        "b 必须作为旧正文 overlay 画出来（正常吞字），不能闪没"
    );
    let clips = coord.hidden_canonical_rects_for(frontier.as_ref().expect("前沿存在"));
    assert!(
        !clips.is_empty(),
        "a 必须作为新字被遮罩逐步打开（正常吐字）"
    );
}

/// Issue #826 评论 12：Frontier 换 burst 时**不能**把独立的 Reflow 一起清掉。
///
/// 评论原文的反例：旧正文 `A|B` 输入 X 得 `AX|B` ——
/// - X 走 Insert Frontier；
/// - B 走 Reflow，正在从旧位置往右移动。
///
/// 动画跑到一半立刻 Backspace 删掉 X，Frontier 因 kind 不同（Insert -> Delete）
/// 换 burst 完全正常。但此时 `previous.target_text == request.base_text ==
/// "AXB"`，**Reflow 的 revision 链仍然连续**，应该「B 当前屏幕半路位置 →
/// retarget → 删掉 X 后的最新位置」。
///
/// 旧代码在 Frontier 开新 burst 的分支调全局 `finish_edit_frontier_to_canonical()`，
/// 它会先 `active_reflow = None`，导致 B 从屏幕半路**瞬移**回 `AXB` 的
/// canonical 位置，再从那里往回走 —— 肉眼可见的瞬移。
///
/// 断言：新一轮 Reflow 的起点 x **等于**上一轮 80ms 时 B 的屏幕 `dest_rect.x`，
/// **不等于** `AXB` 的 canonical B.x。
#[test]
fn reflow_survives_frontier_burst_boundary_and_retargets_from_current_screen_position() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // 旧正文 `A|B`：`B` 在 doc x = 20。
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 20.0)],
    )]);
    // 输入 X 后 `AX|B`：`B` 右移到 doc x = 60（Reflow 的 canonical 目标）。
    let target = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 30.0), cluster(2, 3, 60.0)],
    )]);

    // 第一笔：Insert X。B 进入 Reflow，20 -> 60。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: base,
        target_snapshot: target.clone(),
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        offset_map: OffsetMap::from_single_edit(2, (1, 1), 1),
        base_text: String::from("AB"),
        target_text: String::from("AXB"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });
    assert!(
        !coord.reflow_glyphs(now).is_empty(),
        "输入 X 之后 B 必须已经在 Reflow 里"
    );

    // 动画跑到 80ms，记下 B 当前的屏幕位置。
    let mid = now + Duration::from_millis(80);
    let mid_x = coord
        .reflow_glyphs(mid)
        .first()
        .expect("B 的 Reflow span 必须在跑")
        .dest_rect
        .x;

    // 第二笔：Backspace 删掉 X，回到 `A|B`。Frontier kind 从 Insert 变 Delete，
    // 必然换 burst；但 `previous.target_text("AXB") == request.base_text("AXB")`，
    // Reflow 的 revision 链连续。
    let restored = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        2,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 20.0)],
    )]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: target,
        target_snapshot: restored.clone(),
        deleted_ranges: vec![(1, 2)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(3, (1, 2), 0),
        base_text: String::from("AXB"),
        target_text: String::from("AB"),
        conceal_direction: ConcealDirection::Forward,
        now: mid,
    });

    // Frontier 确实换了新 burst（kind 变成 Delete）。
    assert_eq!(
        coord.active_edit_frontier_kind(),
        Some(EditFrontierKind::Delete),
        "Frontier 必须已经换成 Delete 的新 burst"
    );

    let after = coord.reflow_glyphs(mid);
    assert!(
        !after.is_empty(),
        "Frontier 换 burst 不得清掉 revision 链仍连续的 Reflow"
    );
    let start_x = after
        .first()
        .expect("B 的 Reflow span 必须还在")
        .dest_rect
        .x;
    assert!(
        (start_x - mid_x).abs() < 1e-6,
        "新一轮 Reflow 必须从上一帧的屏幕位置继续（mid_x = {mid_x}），实际 start_x = {start_x}"
    );
    assert!(
        (start_x - 60.0).abs() > 1e-6,
        "不能退回 AXB 的 canonical B.x = 60（那就是瞬移），实际 start_x = {start_x}"
    );
}

/// Issue #826 评论 13：正在 Reflow 的字这一笔被删除时，Conceal 必须从
/// **当前屏幕位置**开始吞，而不是瞬移回 canonical 位置。
///
/// 评论原文的反例：
/// ```text
/// A|B  ->  输入 X  ->  AX|B
/// B old x = 20，B target canonical x = 60，正在 Reflow 20 -> 60
/// 80ms 时 B 当前屏幕位置 x = 55
/// 此时按 Delete 把 B 删掉  ->  AX|
/// ```
///
/// B 属于本次 `deleted_ranges`，所以最新 Reflow retarget 会正确排除它、
/// 新 Delete Frontier 会给它建 ConcealTrack。但 ConcealTrack 如果只从
/// `base_snapshot`（= `AXB`，B 在 x=60）取几何，屏幕上就会出现
/// `55 -> 60 瞬移一下 -> 再开始吞字`。自动换行时这跳变可能跨整行。
#[test]
fn reflowing_glyph_deleted_mid_motion_conceals_from_current_screen_position() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 20.0)],
    )]);
    let after_x = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 30.0), cluster(2, 3, 60.0)],
    )]);

    // 第一笔：输入 X。B 进入 Reflow，20 -> 60。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: base,
        target_snapshot: after_x.clone(),
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        offset_map: OffsetMap::from_single_edit(2, (1, 1), 1),
        base_text: String::from("AB"),
        target_text: String::from("AXB"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    let mid = now + Duration::from_millis(80);
    let mid_x = coord
        .reflow_glyphs(mid)
        .first()
        .expect("B must be reflowing")
        .dest_rect
        .x;

    // 第二笔：Delete 把 B 删掉。B 从 Reflow 所有权切到 Conceal 所有权。
    let mut request = EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: after_x,
        target_snapshot: snapshot(Vec::new()),
        deleted_ranges: vec![(2, 3)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(3, (2, 3), 0),
        base_text: String::from("AXB"),
        target_text: String::from("AX"),
        conceal_direction: ConcealDirection::Forward,
        now: mid,
    };
    coord.begin_or_extend_edit_frontier(request);

    let sample = coord.sample_edit_frontier(mid).expect("frontier alive");

    // 1. Reflow 不再包含 B（B 已 changed）。
    assert!(
        coord.reflow_glyphs(mid).is_empty(),
        "B 已经是 changed text，必须从 Reflow 移除"
    );
    // 2. Delete Frontier 仍给 B 建了 overlay。
    let overlay = coord.old_overlay_glyphs_for(&sample);
    assert!(
        !overlay.is_empty(),
        "B 必须作为旧正文 overlay 画出来（正常吞字），不能闪没"
    );
    // 3. overlay 第一帧的 dest 必须等于上一帧 Reflow 的屏幕位置。
    let overlay_x = overlay.first().expect("overlay glyph").dest_rect.x;
    assert!(
        (overlay_x - mid_x).abs() < 1e-6,
        "Conceal 必须从 mid_x = {mid_x} 开始，实际 overlay_x = {overlay_x}"
    );
    // 4. 不能等于 canonical 的 60。
    assert!(
        (overlay_x - 60.0).abs() > 1e-6,
        "不能退回 AXB 的 canonical B.x = 60（那就是瞬移），实际 {overlay_x}"
    );
}

/// Issue #826 评论 13：同一条 burst 内（`can_extend == true`，不换 Frontier burst）
/// 连续 Delete 撞上正在 Reflow 的字，同样要从当前屏幕几何接管。
///
/// 评论原文指出：连续 Delete 时 `can_extend == true` 不会换 burst，但
/// `extend_delete()` 新建 B 的 ConcealTrack 时仍只从 `self.base_snapshot` 取几何，
/// 所以即使评论 12 的 `finish_frontier_burst_only` 完全正确，这个问题依然存在。
#[test]
fn same_burst_delete_handoff_from_reflow_uses_current_geometry() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // `abc` 删掉中间的 b 得 `ac`：b 被 Conceal、c 走 Reflow（20 -> 60）。
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 20.0), cluster(2, 3, 60.0)],
    )]);
    let after = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 30.0)],
    )]);

    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: base,
        target_snapshot: after.clone(),
        deleted_ranges: vec![(1, 2)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(3, (1, 2), 0),
        base_text: String::from("abc"),
        target_text: String::from("ac"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });
    assert!(!coord.reflow_glyphs(now).is_empty(), "c must be reflowing");

    let mid = now + Duration::from_millis(80);
    let mid_x = coord
        .reflow_glyphs(mid)
        .first()
        .expect("c reflow span")
        .dest_rect
        .x;

    // 同一 burst 内继续 Delete 删掉 c：c 从 Reflow 切到 Conceal。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: after,
        target_snapshot: snapshot(Vec::new()),
        deleted_ranges: vec![(1, 2)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(2, (1, 2), 0),
        base_text: String::from("ac"),
        target_text: String::from("a"),
        conceal_direction: ConcealDirection::Forward,
        now: mid,
    });

    let sample = coord.sample_edit_frontier(mid).expect("frontier alive");
    let overlay = coord.old_overlay_glyphs_for(&sample);
    assert!(!overlay.is_empty(), "c 必须有 Conceal overlay");
    // overlay 里同时还有上一笔就在吞的 b，它的位置是「已被 clip 过的剩余部分」，
    // 与本次交接无关。断言的是**存在一个** overlay glyph 落在 mid_x —— 那就是
    // 本次从 Reflow 接管过来的 c。
    assert!(
        overlay
            .iter()
            .any(|glyph| (glyph.dest_rect.x - mid_x).abs() < 1e-6),
        "同一 burst 内也必须从当前屏幕几何接管：mid_x = {mid_x}, overlays = {:?}",
        overlay
            .iter()
            .map(|glyph| glyph.dest_rect.x)
            .collect::<Vec<_>>()
    );
}

/// Issue #826 评论 14 阻塞 1：begin_delete 的每条 track 只能拥有**自己那条
/// deleted range** 覆盖的 Reflow glyph。
///
/// 当前有 B / C / D 三个字都在 Reflow，这一笔只删 C —— 新 Delete track
/// 不能把 B/C/D 全塞进自己的 `glyphs` 和 path，否则 B / D 明明没删却被
/// overlay 再画一份（双影），更糟时它们会跟着 C 的 conceal 前沿一起消失。
/// 本次有多个 disjoint deleted_ranges 时，每条 track 也要各自只拿自己那份
/// —— 不能在 coordinator 先 filter 一次（那样两条 track 会各自拿到 A+B）。
#[test]
fn begin_delete_handoff_only_owns_glyphs_inside_each_deleted_range() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // `BCD` 三行，B / C / D 各自在不同 y：0 / 20 / 40。
    let base = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(1, 20.0, 1, vec![cluster(1, 2, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(2, 40.0, 2, vec![cluster(2, 3, 0.0)]),
    ]);

    // 只删 C（base 坐标 [2,3)）。B / D 不在 deleted_ranges 里。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: base.clone(),
        target_snapshot: snapshot(Vec::new()),
        deleted_ranges: vec![(2, 3)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(3, (2, 3), 0),
        base_text: String::from("BCD"),
        target_text: String::from("BD"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    let sample = coord.sample_edit_frontier(now).expect("frontier alive");
    let overlay = coord.old_overlay_glyphs_for(&sample);
    assert_eq!(
        overlay.len(),
        1,
        "只有被删的 C 能有 overlay，B / D 不得被塞进来；实际 y = {:?}",
        overlay
            .iter()
            .map(|glyph| glyph.dest_rect.y)
            .collect::<Vec<_>>()
    );
    assert!(
        (overlay[0].dest_rect.y - 40.0).abs() < 1e-6,
        "被删的 C 在第三行（y=40），实际 y = {}",
        overlay[0].dest_rect.y
    );
}

/// Issue #826 评论 14 阻塞 2：一个 deleted range 里**一部分在 Reflow、
/// 一部分没在 Reflow** 时，没在 Reflow 的字不能丢。
///
/// 一次删两个字 `BC`：B 上一帧正在 Reflow，C 本来就在稳定位置。
/// 之前的双轨逻辑（`glyphs.is_empty()` 就回 snapshot、非空就只用 handed_off）
/// 会让整条 track 只剩 B，C 第一帧直接消失、没有吞字。
#[test]
fn mixed_reflow_and_static_deleted_range_keeps_all_deleted_glyphs() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // `AXBC`：B 在移动中（doc x 40），C 稳定（doc x 60）。
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![
            cluster(0, 1, 0.0),
            cluster(1, 2, 20.0),
            cluster(2, 3, 40.0),
            cluster(3, 4, 60.0),
        ],
    )]);
    // 输入 X 让 B 右移：old B 在 x=20，target B 在 x=40。
    let after_x = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        vec![
            cluster(0, 1, 0.0),
            cluster(1, 2, 20.0),
            cluster(2, 3, 40.0),
            cluster(3, 4, 60.0),
            cluster(4, 5, 80.0),
        ],
    )]);

    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: base.clone(),
        target_snapshot: after_x.clone(),
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(2, 2)],
        offset_map: OffsetMap::from_single_edit(3, (2, 2), 1),
        base_text: String::from("ABC"),
        target_text: String::from("AXBC"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    let mid = now + Duration::from_millis(80);
    // B 的 Reflow span 一定存在（它确实移动了）。
    assert!(!coord.reflow_glyphs(mid).is_empty(), "B 必须在 Reflow 里");

    // 一次删掉 BC：B（正在 Reflow）+ C（稳定）。
    let mut request = EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: after_x.clone(),
        target_snapshot: snapshot(Vec::new()),
        deleted_ranges: vec![(2, 4)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(4, (2, 4), 0),
        base_text: String::from("AXBC"),
        target_text: String::from("AX"),
        conceal_direction: ConcealDirection::Forward,
        now: mid,
    };
    coord.begin_or_extend_edit_frontier(request);

    let sample = coord.sample_edit_frontier(mid).expect("frontier alive");
    let overlay = coord.old_overlay_glyphs_for(&sample);
    assert_eq!(
        overlay.len(),
        2,
        "被删的 B 和 C 都必须有 overlay（一个 Reflow、一个静态），实际 {} 个",
        overlay.len()
    );
}

/// Issue #826 评论 14 阻塞 3：handoff（以及所有）吞字 track 的 overlay 必须
/// **真的逐步收缩**，不能 progress 0~0.9 一直完整、progress=1 突然消失。
///
/// 之前 `old_overlay_rects()` 拿 base_snapshot 的 line id 去
/// `path.segment_index_for_line` 反查，而 handoff 建的 path 的 `line_id` 是
/// 占位值，真实 id 几乎不可能匹配 —— 每次都落进「整行保留」分支。
#[test]
fn handed_off_conceal_progress_actually_shrinks_before_terminal_frame() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 20.0)],
    )]);
    let after_x = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 20.0), cluster(2, 3, 40.0)],
    )]);

    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: base,
        target_snapshot: after_x.clone(),
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 1)],
        offset_map: OffsetMap::from_single_edit(2, (1, 1), 1),
        base_text: String::from("AB"),
        target_text: String::from("AXB"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });
    assert!(!coord.reflow_glyphs(now).is_empty(), "B must be reflowing");

    let mid = now + Duration::from_millis(80);
    let mut request = EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: after_x,
        target_snapshot: snapshot(Vec::new()),
        deleted_ranges: vec![(2, 3)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(3, (2, 3), 0),
        base_text: String::from("AXB"),
        target_text: String::from("AX"),
        conceal_direction: ConcealDirection::Forward,
        now: mid,
    };
    coord.begin_or_extend_edit_frontier(request);

    let width_at = |offset_ms: u64| -> f64 {
        let at = mid + Duration::from_millis(offset_ms);
        let sample = coord.sample_edit_frontier(at).expect("frontier alive");
        coord
            .old_overlay_glyphs_for(&sample)
            .iter()
            .map(|glyph| glyph.dest_rect.w)
            .sum::<f64>()
    };

    let full = width_at(0);
    let half = width_at(60);
    let late = width_at(140);
    assert!(full > 0.0, "第一帧必须有完整 overlay 宽度");
    assert!(
        half < full,
        "半程 overlay 必须已经在收缩：full = {full}, half = {half}"
    );
    assert!(
        late < half,
        "接近结束前 overlay 必须继续收缩：half = {half}, late = {late}"
    );
}

/// Issue #826 评论 14 阻塞 4：同 burst handoff 的 source texture 必须留在
/// active 集合里，否则 `retain_active_snapshot_ids` 会先把它删掉。
///
/// `base_snapshot` 是 burst 第一笔之前的快照（`abc`，line id 7），而 handoff
/// glyph 的贴图来自「上一轮 Reflow target」快照（`ac`，line id 9），两者不同。
/// 之前 active ids 只含 burst base 的 id，于是
/// `texture_cache.retain_active_snapshot_ids()`（实现就是 `line_store.retain`）
/// 先删掉 handoff 那张图；新 Reflow 因该 glyph 已 changed 不再声明它，old
/// overlay 纹理准备也只看 burst base 补不回来 —— renderer 找不到
/// `ConcealGlyphGeometry.snapshot_id`，这个 glyph 直接 skip，真机画不出来。
///
/// 用 Delete -> Delete 保证 `can_extend == true`（同 burst），否则 Insert -> Delete
/// 会换新 burst，base_snapshot 恰好等于 current snapshot，就测不到这个差异。
#[test]
fn same_burst_handoff_snapshot_id_is_retained_as_active_overlay_texture() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // burst base = `abc`（line id 7）。删中间的 b 得 `ac`：c 走 Reflow。
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        7,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 20.0), cluster(2, 3, 60.0)],
    )]);
    // 第一次 Delete 之后的新正文 = `ac`（line id 9），c 在 doc x = 30。
    let after_b = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        9,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 30.0)],
    )]);

    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: base,
        target_snapshot: after_b.clone(),
        deleted_ranges: vec![(1, 2)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(3, (1, 2), 0),
        base_text: String::from("abc"),
        target_text: String::from("ac"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });
    assert!(!coord.reflow_glyphs(now).is_empty(), "c must be reflowing");

    // 同一 burst 继续 Delete 删掉 c：c 从 Reflow 切到 Conceal，贴图来自
    // `after_b`（line id 9），而 burst base 是 `abc`（line id 7）。
    let mid = now + Duration::from_millis(80);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: after_b,
        target_snapshot: snapshot(Vec::new()),
        deleted_ranges: vec![(1, 2)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(2, (1, 2), 0),
        base_text: String::from("ac"),
        target_text: String::from("a"),
        conceal_direction: ConcealDirection::Forward,
        now: mid,
    });

    let sample = coord.sample_edit_frontier(mid).expect("frontier alive");
    let overlay = coord.old_overlay_glyphs_for(&sample);
    assert!(!overlay.is_empty(), "handoff glyph 必须有 overlay");

    // handoff glyph 的贴图必须来自 current snapshot（id 9），不是 burst base（id 7）。
    let handoff_id = LineSnapshotId::new(0, 0, 9);
    let base_id = LineSnapshotId::new(0, 0, 7);
    assert!(
        overlay.iter().any(|glyph| glyph.snapshot_id == handoff_id),
        "handoff glyph 的 snapshot_id 应来自 current old layout（line 9），实际 {:?}",
        overlay
            .iter()
            .map(|glyph| glyph.snapshot_id)
            .collect::<Vec<_>>()
    );

    // active overlay ids 必须包含它 —— 否则 retain 会先把纹理清掉。
    let active = coord.active_old_overlay_snapshot_ids();
    assert!(
        active.contains(&handoff_id),
        "handoff 的 source texture 必须进 active overlay ids：active = {active:?}"
    );
    assert!(
        !active.contains(&base_id) || active.len() > 1,
        "burst base 的 line id 不该是唯一的 active overlay texture"
    );
    // 也必须出现在 retain 真正依据的总集合里。
    assert!(
        coord.collect_active_snapshot_ids().contains(&handoff_id),
        "handoff 的 source texture 必须进 collect_active_snapshot_ids"
    );
}
