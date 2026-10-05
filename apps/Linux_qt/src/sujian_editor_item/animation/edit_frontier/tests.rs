//! Issue #826: 遮罩前沿单元测试。
//!
//! 覆盖：前沿采样 / extend 不产生第二个对象 / 跨 revision 范围累计 /
//! 视觉路径（跨自动换行 + 方向）/ 多条不相邻 patch / 吐字裁剪矩形 /
//! 吞字 overlay 首帧可见。

use std::time::{Duration, Instant};

use writer_core::editor::OffsetMap;

use crate::editor::layout::{CaretAffinity, LayoutSnapshot};
use crate::sujian_editor_item::animation::coordinator::EditFrontierRequest;
use crate::sujian_editor_item::animation::edit_frontier::{
    ConcealDirection, EditFrontierKind, EditFrontierState, FrontierPath, FrontierRect,
    PathDirection,
};
use crate::sujian_editor_item::edit_motion::EditorAnimationKind;
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, PreparedLineSnapshot, ShapingIdentity, SourceRect,
};

fn instant_at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

fn empty_snapshot() -> EditorLayoutSnapshot {
    EditorLayoutSnapshot::new(
        LayoutSnapshot::empty_for_tests(),
        Vec::new(),
        None,
        None,
        CaretAffinity::Downstream,
    )
}

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

/// `ab` 两字符的单行快照（cluster byte range 与文本一致）。
fn ab_snapshot() -> EditorLayoutSnapshot {
    snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
    )])
}

/// `abc` 三字符的单行快照。
fn abc_snapshot() -> EditorLayoutSnapshot {
    snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0), cluster(2, 3, 20.0)],
    )])
}

/// 单行、200 个 cluster 的宽正文，用于「连续输入/删除」这类长序列测试。
fn wide_snapshot() -> EditorLayoutSnapshot {
    let clusters: Vec<LineClusterSnapshot> = (0..200)
        .map(|i| cluster(i, i + 1, (i as f64) * 10.0))
        .collect();
    snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0, 0.0, 0, clusters,
    )])
}

#[test]
fn frontier_sample_progresses_from_zero_to_one() {
    let now = Instant::now();
    let state = EditFrontierState::begin_insert(
        String::new(),
        empty_snapshot(),
        String::from("a"),
        vec![(0, 1)],
        OffsetMap::from_single_edit(0, (0, 0), 0),
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
        String::new(),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0)],
        )]),
        String::from("a"),
        vec![(0, 1)],
        OffsetMap::from_single_edit(0, (0, 0), 0),
        now,
        160,
    );
    assert!(!state.is_finished(instant_at(now, 80)));
    assert!(state.is_finished(instant_at(now, 160)));
    assert!(state.is_finished(instant_at(now, 500)));
}

/// Issue #826 评论 3 问题 4：吞字第一帧旧字必须完整可见。
///
/// `needs_old_overlay` 之前写的是 `progress > 0.0`，导致 progress=0 时不画旧字，
/// 下一帧 progress>0 又画出来 —— 存在首帧闪烁窗口。正确语义是
/// `progress < 1.0`：从第一帧到结束前一帧都画，完全收掉后不再画。
#[test]
fn delete_overlay_is_visible_on_the_first_frame() {
    let now = Instant::now();
    let state = EditFrontierState::begin_delete(
        empty_snapshot(),
        String::from("ABCDEF"),
        empty_snapshot(),
        String::from("ABCEF"),
        vec![(3, 4)],
        OffsetMap::from_single_edit(0, (0, 0), 0),
        &[],
        ConcealDirection::Backward,
        now,
        160,
    );
    assert!(
        state.sample(now).needs_old_overlay(),
        "吞字第一帧旧字必须完整可见，不能是空白"
    );
    assert!(
        state.sample(instant_at(now, 80)).needs_old_overlay(),
        "吞字中途旧字仍可见"
    );
    assert!(
        !state.sample(instant_at(now, 160)).needs_old_overlay(),
        "吞字结束旧字必须完全消失"
    );
}

/// Issue #826 评论 3 问题 1：连续吐字必须把先前还没吐完的字一起留在遮罩范围里。
#[test]
fn extend_insert_accumulates_new_range_across_revisions() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_insert(
        String::new(),
        ab_snapshot(),
        String::from("a"),
        vec![(1, 2)],
        OffsetMap::from_single_edit(0, (0, 0), 0),
        now,
        160,
    );
    assert_eq!(state.new_ranges(), vec![(1, 2)]);

    // 第一笔：先在半程采样，再扩展。正文 "a" -> "ab"（在位置 1 插入 b）。
    let half = instant_at(now, 80);
    let prev_target_to_new = OffsetMap::build("a", "ab");
    state.extend_insert(
        ab_snapshot(),
        String::from("ab"),
        vec![(1, 2)],
        &prev_target_to_new,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );
    assert_eq!(state.new_ranges(), vec![(1, 2)]);

    // 第二笔：正文 "ab" -> "abc"（在位置 2 插入 c）。
    // 第一次的 [1,2) 映射到新坐标仍是 [1,2)，本次新增 [2,3)，
    // 累计后必须是 [1,3) —— 不能只剩 [2,3)。
    let prev_target_to_new = OffsetMap::build("ab", "abc");
    state.extend_insert(
        abc_snapshot(),
        String::from("abc"),
        vec![(2, 3)],
        &prev_target_to_new,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );
    // Issue #826 评论 9 阻塞 3：相邻但来源不同的 track **不合并** ——
    // 动画状态按编辑身份保存，静态 clip 层渲染时本来就会合并相邻矩形。
    // Issue #826 评论 17：相邻 range 合并成一段，连续按键不按次数累积 region。
    assert_eq!(
        state.new_ranges(),
        vec![(1, 3)],
        "连续吐字必须把整轮 burst 的新字都留在遮罩里；相邻 track 保持各自身份"
    );
    assert!(
        state
            .reveal
            .regions
            .iter()
            .all(|track| track.range.1 > track.range.0),
        "每条 track 的 range 都必须非空"
    );
}

/// Issue #826 评论 3 问题 2：连续 Delete 的 old_range 必须映射回 burst 最初坐标。
///
/// 例子 `ABC|DEF` 连续 Delete：第一次删 D，本次 old range = [3,4)；
/// 第二次删 E，本次 old range 仍是 [3,4)（因为 E 往前挪了一位），
/// 但在 burst 最初的 `ABCDEF` 坐标里应该累计成 [3,5)。
///
/// Issue #826 评论 19：两笔都必须带**真实 glyph 的快照** —— 吞字 region 只从
/// 仍有可见 glyph 的 owner 生成（评论 19 阻塞 3），空快照根本不产生 region，
/// 那时断言 `old_ranges()` 就没有意义了。
#[test]
fn extend_delete_maps_old_range_back_to_base_coordinates() {
    let now = Instant::now();
    // burst base：`ABCDEF`（D 在 byte 3，E 在 byte 4）。
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        (0..6)
            .map(|i| cluster(i, i + 1, (i as f64) * 10.0))
            .collect(),
    )]);
    let mut state = EditFrontierState::begin_delete(
        base.clone(),
        String::from("ABCDEF"),
        empty_snapshot(),
        String::from("ABCEF"),
        vec![(3, 4)],
        OffsetMap::from_single_edit(0, (0, 0), 0),
        &[],
        ConcealDirection::Forward,
        now,
        160,
    );
    assert_eq!(state.old_ranges(), vec![(3, 4)]);

    // 第二次删 E：base_text = "ABCDEF"，本次编辑前正文 = "ABCEF"，
    // 本次删除区间在 "ABCEF" 坐标里是 [3,4)（删掉 E）。
    // 映回 base 坐标是 [4,5)，与第一次的 [3,4) 合并成 [3,5)。
    let base_to_current = OffsetMap::build("ABCDEF", "ABCEF");
    // 本次编辑前的旧正文快照：新的 line id（代表又过了一个 revision）。
    let current = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        (0..5)
            .map(|i| cluster(i, i + 1, (i as f64) * 10.0))
            .collect(),
    )]);
    state.extend_delete(
        empty_snapshot(),
        String::from("ABCE"),
        vec![(3, 4)],
        &current,
        &base_to_current,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        &[],
        ConcealDirection::Forward,
        instant_at(now, 80),
    );
    assert_eq!(
        state.old_ranges(),
        vec![(3, 5)],
        "第二次删除必须映射回 base 坐标；相邻 range 合并成同一段（评论 17）"
    );
    // base_snapshot 必须保持 burst 开始前的旧正文。
    assert_eq!(state.base_text, "ABCDEF");
}

/// Issue #826 评论 8 阻塞 2：连续 Replace 必须**双侧**累计。
///
/// 之前 Replace 走 `extend_delete`，只更新 old side，新插入的字根本不进 reveal
/// mask，canonical 会把这次新字直接完整显示。
///
/// Issue #826 评论 19：吞字侧改用「仍有可见 glyph 的 owner」建 region，所以
/// begin 用的旧快照、extend 用的 current 快照都必须带真实 glyph。
#[test]
fn extend_replace_accumulates_both_sides() {
    let now = Instant::now();
    // burst base：`ABCDEF`。
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        (0..6)
            .map(|i| cluster(i, i + 1, (i as f64) * 10.0))
            .collect(),
    )]);
    let mut state = EditFrontierState::begin_replace(
        base.clone(),
        String::from("ABCDEF"),
        empty_snapshot(),
        String::from("AXBCDEF"),
        vec![(3, 4)],
        vec![(1, 2)],
        OffsetMap::from_single_edit(0, (0, 0), 0),
        &[],
        ConcealDirection::Backward,
        now,
        160,
    );
    assert_eq!(state.old_ranges(), vec![(3, 4)]);
    assert_eq!(state.new_ranges(), vec![(1, 2)]);

    let half = instant_at(now, 80);
    let base_to_current = OffsetMap::build("ABCDEF", "AXBCDEF");
    let prev_target_to_new = OffsetMap::build("AXBCDEF", "AXBCDEZ");
    // 本次编辑前的旧正文：`AXBCDEF`（`AX` 插在 B 前，E 在 byte 5）。
    let current = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        (0..7)
            .map(|i| cluster(i, i + 1, (i as f64) * 10.0))
            .collect(),
    )]);
    state.extend_replace(
        empty_snapshot(),
        String::from("AXBCDEZ"),
        vec![(5, 6)],
        vec![(6, 7)],
        &current,
        &base_to_current,
        &prev_target_to_new,
        &[],
        ConcealDirection::Backward,
        half,
    );
    // 旧侧：本次删 E（"AXBCDEF" 的 [5,6)）映回 base 是 [4,5)，与 [3,4) 相邻
    // 合并成 [3,5)（评论 17）。
    assert_eq!(
        state.old_ranges(),
        vec![(3, 5)],
        "Replace 的旧侧必须累计；相邻 range 合并成同一段（评论 17）"
    );
    assert_eq!(
        state.new_ranges(),
        vec![(1, 2), (6, 7)],
        "Replace 的新侧也必须累计，否则新插入的字没有任何遮罩"
    );
    // Issue #826 评论 19 阻塞 1：吞字侧 prune 之后 distance 必须从 0 重起
    // （与 `extend_delete` 同一契约）。
    assert_eq!(
        state.conceal.travelled, 0.0,
        "prune 之后吞字前沿必须从 0 重起，不能继承旧 path 的已消费距离"
    );
}

/// Issue #826 评论 8 阻塞 3：多条不相邻的 display_patch 不能被 union 成一个大 range。
#[test]
fn disjoint_patches_stay_separate_ranges_and_paths() {
    let now = Instant::now();
    // target 里放两段相距很远的字：第一行 [0,1) 和第二行 [100,101)。
    let target = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(1, 20.0, 0, vec![cluster(100, 101, 0.0)]),
    ]);
    let state = EditFrontierState::begin_insert(
        String::new(),
        target,
        String::from("x"),
        vec![(0, 1), (100, 101)],
        OffsetMap::from_single_edit(0, (0, 0), 0),
        now,
        160,
    );
    assert_eq!(
        state.new_ranges().len(),
        2,
        "两段不相邻的 patch 必须保持两条，不能 union"
    );
    assert_eq!(
        state.reveal.regions.len(),
        2,
        "每条不相邻 patch 各有自己的视觉路径"
    );
    // progress=0：两段的新字都必须被遮住（共享同一个 progress 同时接管）。
    let rects = state.hidden_new_text_rects(&state.sample(now));
    assert_eq!(
        rects.len(),
        2,
        "同一 progress 下两条 patch 都要被接管，实际 {} 块",
        rects.len()
    );
    assert!(
        state
            .hidden_new_text_rects(&state.sample(instant_at(now, 160)))
            .is_empty(),
        "结束时两段遮罩都必须完全打开"
    );
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

/// Issue #826 评论 7 阻塞 2：跨自动换行输入时，遮罩必须从下一行左侧开始打开。
#[test]
fn wrap_around_insert_reveals_from_the_next_line_left_edge() {
    let now = Instant::now();
    // target：换行后第一行已有 A，新字 X 在第二行行首。
    let target = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(1, 20.0, 0, vec![cluster(1, 2, 40.0)]),
    ]);
    let state = EditFrontierState::begin_insert(
        String::new(),
        target,
        String::from("a\nb"),
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
        160,
    );

    // progress = 0：遮罩在路径起点，第二行的 X 尚未打开，必须整块被遮。
    let start = state.sample(now);
    let rects = state.hidden_new_text_rects(&start);
    assert_eq!(
        rects.len(),
        1,
        "progress=0 时第二行的新字必须整块被遮住，实际 {} 块",
        rects.len()
    );
    assert!(
        rects[0].x >= 40.0,
        "遮罩必须从第二行新字自己的位置开始（x >= 40），实际 {}",
        rects[0].x
    );

    // progress 接近 1：X 必须被完整打开（不再遮）。
    let end = state.sample(instant_at(now, 160));
    assert!(
        state.hidden_new_text_rects(&end).is_empty(),
        "动画结束时遮罩必须完全打开"
    );
}

/// Issue #826 评论 7：视觉路径按视觉顺序分段，跨行时每行各占一段。
#[test]
fn frontier_path_segments_follow_visual_order() {
    let snap = snapshot(vec![
        // 第一行：新字在行尾很靠右的位置（doc x = 200 + visual_x 0）。
        PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 200.0)],
        ),
        // 第二行：新字在行首（doc x = 0）。
        PreparedLineSnapshot::stub_for_tests(1, 20.0, 0, vec![cluster(2, 3, 0.0)]),
    ]);
    let path = FrontierPath::build(&snap, (1, 3), PathDirection::Forward);
    assert_eq!(path.segments.len(), 2, "跨两行必须两段");
    assert!((path.segments[0].y - 0.0).abs() < 1e-9);
    assert!((path.segments[1].y - 20.0).abs() < 1e-9);
    assert!(
        path.segments[1].x_from < path.segments[0].x_from,
        "下一行的路径必须从自己的左侧开始，而不是延续上一行的 x"
    );

    // distance=0 时 reveal 边界都在各段 x_from（什么都没打开）。
    let b0 = path.reveal_bounds(0.0);
    assert!((b0[0].0 - path.segments[0].x_from).abs() < 1e-9);
    assert!((b0[1].0 - path.segments[1].x_from).abs() < 1e-9);

    // distance 超过第一段长度时，第一段全开、第二段还没开始。
    let b1 = path.reveal_bounds(path.segments[0].visual_length);
    assert!((b1[0].0 - path.segments[0].x_to).abs() < 1e-9);
    assert!((b1[1].0 - path.segments[1].x_from).abs() < 1e-9);

    // conceal 反向（Backward）：distance=0 时保留整段旧字。
    let back = FrontierPath::build(&snap, (1, 3), PathDirection::Backward);
    assert_eq!(back.segments.len(), 2);
    assert!(
        (back.segments[0].y - 20.0).abs() < 1e-9,
        "Backward 必须按视觉逆序，第一段是第二行"
    );
    let c0 = back.conceal_bounds(0.0);
    assert!((c0[0].0 - back.segments[0].x_left).abs() < 1e-9);
    assert!((c0[0].1 - back.segments[0].x_right).abs() < 1e-9);
}

/// Issue #826 评论 7：换行符没有 glyph，路径里不产生段（后半段交给 Reflow）。
#[test]
fn newline_only_insert_produces_no_frontier_segment() {
    let now = Instant::now();
    // target "a\nb"：换行符本身没有 cluster，只留两行的可见字。
    let target = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(1, 20.0, 0, vec![cluster(2, 3, 0.0)]),
    ]);
    // inserted range 只覆盖换行符所在字节 [1,2)，那一行没有 cluster。
    let state = EditFrontierState::begin_insert(
        String::new(),
        target,
        String::from("a\nb"),
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
        160,
    );
    assert!(
        state
            .reveal
            .regions
            .iter()
            .all(|t| t.path.segments.is_empty()),
        "只有换行符被插入时不应产生 FrontierMask，改由 Reflow 承担位置变化"
    );
    assert!(
        state.hidden_new_text_rects(&state.sample(now)).is_empty(),
        "没有可见新字就不该有遮罩矩形"
    );
}

/// Issue #826 评论 8 阻塞 1：连续 Backspace 跨自动换行时，
/// 下一行已吞掉的状态不能在 old range 扩到上一行后复活。
///
/// burst base 是两行 `ABC` / `DEF`，caret 在 `DEF|`。
/// 先 Backspace 把 DEF 吞一半，随后继续删到上一行的 C。
/// Backward 方向的路径按视觉逆序（先第二行），所以扩到上一行后新增的段
/// 排在**尾部**，已经走过的第二行不会被重绑到 C 上。
#[test]
fn consecutive_backspace_does_not_revive_previously_concealed_line() {
    let now = Instant::now();
    // base 两行：ABC（doc x 0/10/20），DEF（doc x 0/10/20）。
    let base = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0), cluster(2, 3, 20.0)],
        ),
        PreparedLineSnapshot::stub_for_tests(
            1,
            20.0,
            0,
            vec![cluster(3, 4, 0.0), cluster(4, 5, 10.0), cluster(5, 6, 20.0)],
        ),
    ]);
    // 第一笔：只删第二行的 DEF（old range [3,6)），Backward。
    let mut state = EditFrontierState::begin_delete(
        base.clone(),
        String::from("ABCDEF"),
        base.clone(),
        String::from("ABC"),
        vec![(3, 6)],
        OffsetMap::from_single_edit(0, (0, 0), 0),
        &[],
        ConcealDirection::Backward,
        now,
        160,
    );
    assert_eq!(state.conceal.regions.len(), 1);
    assert!(
        (state.conceal.regions[0].path.segments[0].y - 20.0).abs() < 1e-9,
        "Backward 第一笔必须从第二行开始"
    );

    // 走到半程：第二行已被吞掉一半。
    let half = instant_at(now, 80);
    let mid = state.sample(half);
    let before = state.old_overlay_rects(&mid);
    assert!(
        before.iter().any(|r| (r.y - 20.0).abs() < 1e-9),
        "半程时第二行仍要在 overlay 列表里（正在被收）"
    );

    // 第二笔：继续删到上一行的 C（base 坐标 [2,3)），Backward 仍是视觉逆序。
    let base_to_current = OffsetMap::build("ABCDEF", "ABC");
    state.extend_delete(
        base.clone(),
        String::from("AB"),
        vec![(2, 3)],
        &base.clone(),
        &base_to_current,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        &[],
        ConcealDirection::Backward,
        half,
    );
    // Issue #826 评论 9 阻塞 3：新增的 C 与已吞掉的 DEF **不相邻**（DEF 已删，
    // C 在它左边），所以是两条独立 track。第一条 track 的路径不受影响 ——
    // 这正是「按编辑身份保存」要保证的：扩到上一行不会重绑已走过的那条。
    assert_eq!(
        state.old_ranges(),
        vec![(2, 6)],
        "扩到上一行后合并成一段（评论 17：相邻就合并，不按按键累积 region）"
    );
    let paths: Vec<&FrontierPath> = state.conceal.regions.iter().map(|r| &r.path).collect();
    assert_eq!(paths.len(), 1, "合并后只有一条吞字路径");
    assert_eq!(paths[0].segments.len(), 2, "跨两行仍然有两段");
    assert!(
        (paths[0].segments[0].y - 20.0).abs() < 1e-9,
        "视觉逆序：第一段仍是第二行"
    );
    assert!(
        (paths[0].segments[1].y - 0.0).abs() < 1e-9,
        "视觉逆序：第二段才是第一行（新增的 C 追加在尾部）"
    );
}

/// Issue #826 评论 8 阻塞 1：连续 Delete 键跨自动换行时，
/// 第一行已吞掉的状态不能在 old range 扩到下一行后复活。
#[test]
fn consecutive_forward_delete_does_not_revive_previously_concealed_line() {
    let now = Instant::now();
    let base = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0), cluster(2, 3, 20.0)],
        ),
        PreparedLineSnapshot::stub_for_tests(
            1,
            20.0,
            0,
            vec![cluster(3, 4, 0.0), cluster(4, 5, 10.0), cluster(5, 6, 20.0)],
        ),
    ]);
    // caret 在第一行 `ABC|`。Delete 键向右扩，Forward = 视觉正序。
    let mut state = EditFrontierState::begin_delete(
        base.clone(),
        String::from("ABCDEF"),
        base.clone(),
        String::from("DEF"),
        vec![(0, 3)],
        OffsetMap::from_single_edit(0, (0, 0), 0),
        &[],
        ConcealDirection::Forward,
        now,
        160,
    );
    assert!(
        (state.conceal.regions[0].path.segments[0].y - 0.0).abs() < 1e-9,
        "Forward 第一笔必须从第一行开始"
    );

    let half = instant_at(now, 80);
    // 第二笔：扩到下一行（base 坐标 [3,6)）。old range 变成 [0,6)。
    let base_to_current = OffsetMap::build("ABCDEF", "DEF");
    // Issue #826 评论 17：相邻 range 合并成**一段**（[2,6)）后，这条 region 跨两行，
    // 新增的 C（y=0）与已吞的 DEF（y=20）必须画在**同一条**路径上。因此
    // current old layout 要同时包含两行的 glyph —— 已吞的那一行虽然已从正文
    // 消失，但它仍要被吞、仍要画、仍要占纹理，这些几何由上一笔收集的
    // `conceal_glyphs` 保留（见 `extend_delete` 里只收「本次新删」的 glyph）。
    let current_two_rows = base.clone();
    state.extend_delete(
        current_two_rows.clone(),
        String::from(""),
        vec![(0, 3)],
        &current_two_rows,
        &base_to_current,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        &[],
        ConcealDirection::Forward,
        half,
    );
    // 第二笔的 current old layout 用**新的 line id**：它代表「第一笔已经删掉第 1 行」
    // 之后的那个 revision，两行的字虽然 byte range 相同，但不是同一块 glyph。
    let current_two_rows = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(
            10,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0), cluster(2, 3, 20.0)],
        ),
        PreparedLineSnapshot::stub_for_tests(
            11,
            20.0,
            0,
            vec![cluster(3, 4, 0.0), cluster(4, 5, 10.0), cluster(5, 6, 20.0)],
        ),
    ]);
    state.extend_delete(
        current_two_rows.clone(),
        String::from(""),
        vec![(3, 6)],
        &current_two_rows,
        &base_to_current,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        &[],
        ConcealDirection::Forward,
        half,
    );
    // 相邻（[0,3) 与 [3,6)）→ 合并成一段（评论 17）。
    assert_eq!(state.old_ranges(), vec![(0, 6)]);
    let paths: Vec<&FrontierPath> = state.conceal.regions.iter().map(|r| &r.path).collect();
    assert_eq!(paths.len(), 1, "合并后只有一条吞字路径");
    assert_eq!(paths[0].segments.len(), 2, "跨两行仍然有两段");
    assert!(
        (paths[0].segments[0].y - 0.0).abs() < 1e-9,
        "视觉正序：第一段仍是第一行"
    );
    assert!((paths[0].segments[1].y - 20.0).abs() < 1e-9);
}

/// Issue #826 评论 17：连续输入 100 次，状态**不随按键次数线性增长**。
///
/// 议题正文明确禁止「按了多少次键就积多少个动画单元」。旧实现是
/// `Vec<RevealTrack>`，第 N 个键产生 N 条 track，每条各存 path / travelled /
/// owner —— 结构上就是历史动画单元。
///
/// 现在相邻 range 会合并成同一个 region，整个 state 只有一个前沿时钟
/// （`reveal.travelled`），所以连打 100 次仍然只有 1 个 region。
#[test]
fn continuous_insert_does_not_accumulate_per_keystroke_animation_units() {
    let now = Instant::now();
    let mut text = String::from("a");
    let mut state = EditFrontierState::begin_insert(
        String::from("a"),
        wide_snapshot(),
        text.clone(),
        Vec::new(),
        OffsetMap::from_single_edit(0, (0, 0), 0),
        now,
        160,
    );

    for i in 0..100usize {
        let prev = text.clone();
        text.push('a');
        let offset_map = OffsetMap::from_single_edit(prev.len(), (prev.len(), prev.len()), 1);
        state.extend_insert(
            wide_snapshot(),
            text.clone(),
            vec![(prev.len(), prev.len() + 1)],
            &offset_map,
            &OffsetMap::from_single_edit(0, (0, 0), 0),
            instant_at(now, (i as u64) * 2),
        );
        // 每一次都必须仍是「一个前沿 + 至多一个 region」。
        assert!(
            state.reveal.regions.len() <= 1,
            "第 {} 次输入后 region 数必须是 0 或 1（相邻合并），实际 {}",
            i + 1,
            state.reveal.regions.len()
        );
    }

    assert_eq!(
        state.reveal.regions.len(),
        1,
        "连续 append 100 次后只应有 1 个 reveal region，而不是 100 个历史动画单元"
    );
    assert_eq!(
        state.new_ranges().len(),
        1,
        "累计的新文字范围是**一段** [0, 100)，不是 100 段"
    );
    assert!(
        state.reveal.travelled.is_finite(),
        "单一前沿时钟必须是有限值"
    );
}

/// Issue #826 评论 17：相邻的 reveal range **必须合并成同一个 region**。
///
/// 评论 10 时期要求「相邻但来源不同的 track 不合并」，那是 per-track owner
/// 模型的产物。评论 17 取消了这套规则：相邻就合并，所以连续按键不会按次数
/// 累积 region（见 `continuous_insert_does_not_accumulate_per_keystroke_animation_units`）。
#[test]
fn adjacent_insert_ranges_merge_into_one_region() {
    let now = Instant::now();
    let target = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
    )]);

    let state = EditFrontierState::begin_insert(
        String::new(),
        target,
        String::from("ab"),
        // 两条相邻但不相交的 inserted range。
        vec![(0, 1), (1, 2)],
        OffsetMap::from_single_edit(0, (0, 0), 2),
        now,
        160,
    );

    assert_eq!(
        state.new_ranges(),
        vec![(0, 2)],
        "相邻 range 必须合并成一个 region，不能按按键次数累积 owner"
    );
    assert_eq!(state.reveal.regions.len(), 1, "合并后只有一条前沿路径");
}

/// Issue #826 评论 11 阻塞 2（要求补的第一个测试）：连续两次 Undo 时，
/// 第二笔不能 extend 第一笔的 burst。
///
/// 稳定反例（评论原文）：正文历史 `a -> b -> c`，当前正文 `c`。
/// - 第一次 Undo `c -> b`：Replace，burst base = `c`。
/// - 动画未结束立刻第二次 Undo `b -> a`：kind 相同、cursor 通常没动、
///   conceal direction 也相同，`can_extend` 本来是 true。
///   但第二笔要删的 `b` 是第一次 Undo **刚插出来的字**，它在 burst base `c`
///   里根本不存在 → `base_to_target_map.map_new_range_to_old(b_range)` 必然 None。
///   旧代码 `filter_map` 静默丢掉 → `b` 没有 ConcealTrack；同时它还在 Reveal 的
///   track 映射也失败 → 静默 `continue` → **`b` 直接闪没**。
#[test]
fn replace_cannot_extend_when_deleted_text_was_created_inside_current_burst() {
    let now = Instant::now();
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0)],
    )]);

    // 第一笔 Undo：c -> b，burst base = c。
    let first_map = OffsetMap::from_single_edit(1, (0, 1), 1);
    let state = EditFrontierState::begin_replace(
        base,
        String::from("c"),
        snapshot(Vec::new()),
        String::from("b"),
        vec![(0, 1)],
        vec![(0, 1)],
        first_map,
        &[],
        ConcealDirection::Forward,
        now,
        160,
    );
    // burst base 是 c，它只有 [0,1)；因此 base_to_target_map 对任何
    // `abc`/`b` 坐标的区间都映不回 base。
    assert_eq!(state.base_text, "c");

    // 第二笔 Undo：b -> a。deleted range 是当前正文 `b` 的坐标 [0,1)。
    let second = EditFrontierRequest {
        kind: EditorAnimationKind::Replace,
        base_snapshot: snapshot(Vec::new()),
        target_snapshot: snapshot(Vec::new()),
        deleted_ranges: vec![(0, 1)],
        inserted_ranges: vec![(0, 1)],
        offset_map: OffsetMap::from_single_edit(1, (0, 1), 1),
        base_text: String::from("b"),
        target_text: String::from("a"),
        conceal_direction: ConcealDirection::Forward,
        now: instant_at(now, 80),
    };

    // `b` 映不回 burst base `c` → 身份断裂 → 不能 extend。
    assert!(
        !state.can_extend_identity(EditFrontierKind::Replace, &second),
        "本次要删的 b 是第一笔 Undo 刚插出来的，不在 burst base c 里，必须判身份断裂"
    );
    // 换成能映回 base 的场景（例如同一 burst 内继续删 c 本体）就应该能 extend，
    // 证明 preflight 不是一刀切拒绝。
    let same_burst = EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: snapshot(Vec::new()),
        target_snapshot: snapshot(Vec::new()),
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        offset_map: OffsetMap::from_single_edit(1, (1, 1), 1),
        base_text: String::from("b"),
        target_text: String::from("bX"),
        conceal_direction: ConcealDirection::Forward,
        now: instant_at(now, 80),
    };
    assert!(
        state.can_extend_identity(EditFrontierKind::Insert, &same_burst),
        "Insert 侧必须能正常 extend（没有要映回 base 的 deleted range）"
    );
}

/// Issue #826 评论 18 阻塞 1：path 只是**延长**时，前沿绝不能按总长比例往前推。
///
/// 最小反例：第一笔输入 1 个 10px 字，80ms/160ms -> 前沿在 8.75px（ease 0.875）。
/// 立刻再 append 一个相邻 10px 字 -> new_total = 20。
/// 单前沿的正确语义是「前沿仍在 8.75px」：第一个字还差 1.25px 吐完，
/// 第二个刚输入的字**仍然完全藏住**。
#[test]
fn extending_reveal_target_does_not_advance_frontier_into_new_text() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_insert(
        String::new(),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0)],
        )]),
        String::from("a"),
        vec![(0, 1)],
        OffsetMap::from_single_edit(0, (0, 0), 1),
        now,
        160,
    );

    // 半程。真实 Instant 会有调度抖动，所以不写死 8.75，而是记下 extend 之前
    // 本帧实际推进到的位置 —— 关键断言是「extend 之后前沿不动」。
    // 不要手动写 travelled —— `inherited()` 已经会按本帧 progress 推进，
    // 手动写一次等于把 ease 算两遍（extend 时又会推进一次）。
    let half = instant_at(now, 80);
    let before = state.reveal.inherited(state.sample(half).progress);
    assert!(before > 0.0, "半程前沿必须已经前进");
    assert!(
        before > 0.0 && before < 10.0,
        "半程前沿必须在路径内，实际 {before}"
    );

    // 第二笔：再 append 一个 10px 字。
    let target = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
    )]);
    let prev_target_to_new = OffsetMap::from_single_edit(1, (1, 1), 1);
    state.extend_insert(
        target.clone(),
        String::from("ab"),
        vec![(1, 2)],
        &prev_target_to_new,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    // 关键：前沿**原地不动**，不是 before * 20/10。
    assert!(
        (state.reveal.travelled - before).abs() < 1e-6,
        "path 只是延长时前沿必须保持在 {before}，不能按比例推到 {}",
        before * 2.0
    );

    // 从 mask 侧更硬地断言：第二个 glyph（10..20）在这一帧必须 100% 隐藏。
    let sample = state.sample(half);
    let hidden = state.hidden_new_text_rects(&sample);
    let covers_new_glyph = hidden
        .iter()
        .any(|rect| rect.x <= 10.0 + 1e-9 && rect.x + rect.w >= 20.0 - 1e-9);
    assert!(
        covers_new_glyph,
        "刚输入的第二个 glyph（10..20）必须 100% 仍被遮罩，实际 hidden = {:?}",
        hidden
    );
}

/// Issue #826 评论 19 阻塞 2：刚删的字第一帧必须完整显示。
///
/// Issue #826 评论 18 阻塞 1 修的是「extend 时不要按总长比例 rescale」，那个修法
/// 对 **Reveal** 成立（Reveal 没裁掉旧几何，延长 path 可以继承绝对距离）。
/// 但 Conceal 在评论 18 已经改成「prune 成当前可见几何」，此时新 path 的原点
/// 天然是「distance 0 == 当前这一帧的屏幕状态」，再继承旧绝对距离就是重复消费。
///
/// 场景（等宽 10px）：`AB|` 连续 Backspace。
/// - 第一笔删 B，80ms/160ms -> 旧 distance 8.75，B 屏幕真正还剩 1.25px；
/// - 第二笔删 A。prune 后可见几何 = B 的 1.25 + fresh A 的 10 = 11.25px。
///
/// 正确的第二笔第一帧：完整 A（10px）+ B 剩下的 1.25px = 11.25px。
/// 若又 inherited 8.75，第一帧就被预吞 8.75/11.25 ≈ 75%。
///
/// 断言**屏幕上真正画出来的 overlay**（按 source line 区分 A / B），而不是内部
/// `travelled` 数值 —— 上一版正是断言内部数值，把 bug 固化成了测试。
#[test]
fn extending_conceal_target_does_not_preconsume_newly_deleted_text() {
    let now = Instant::now();
    // base：`ab`，a 在 line 7（x 0..10），b 在 line 8（x 10..20）。
    let base = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(7, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(8, 0.0, 1, vec![cluster(1, 2, 10.0)]),
    ]);
    let mut state = EditFrontierState::begin_delete(
        base.clone(),
        String::from("ab"),
        snapshot(Vec::new()),
        String::from("a"),
        vec![(1, 2)],
        OffsetMap::from_single_edit(2, (1, 2), 0),
        &[],
        ConcealDirection::Backward,
        now,
        160,
    );

    // 第一笔半程：b 被吞到只剩 1.25px。
    let half = instant_at(now, 80);
    let mid_sample = state.sample(half);
    let mid_overlay = state.old_overlay_glyphs(&mid_sample);
    let mid_b_width: f64 = mid_overlay
        .iter()
        .filter(|glyph| glyph.snapshot_id.visual_line_ordinal == 8)
        .map(|glyph| glyph.dest_rect.w)
        .sum();
    assert!(
        (mid_b_width - 1.25).abs() < 0.2,
        "半程时 b 屏幕应只剩约 1.25px，实际 {mid_b_width}"
    );

    // 第二笔：把 a 也删掉（同一 burst）。
    let after_first = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        7,
        0.0,
        0,
        vec![cluster(0, 1, 0.0)],
    )]);
    state.extend_delete(
        after_first.clone(),
        String::from(""),
        vec![(0, 1)],
        &after_first,
        // 本次编辑前的正文是 burst base 去掉 b 之后的 `a`，所以 base_to_current
        // 就是 `"ab" -> "a"` 这条映射；用它把本次删的 `a` 映回 base 坐标 [0,1)。
        &OffsetMap::build("ab", "a"),
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        &[],
        ConcealDirection::Backward,
        half,
    );

    // extend 后 progress = 0，这一帧必须完全等于「prune 后的当前屏幕状态」。
    let after = state.sample(half);
    let overlay = state.old_overlay_glyphs(&after);
    let a_width: f64 = overlay
        .iter()
        .filter(|glyph| glyph.snapshot_id.visual_line_ordinal == 7)
        .map(|glyph| glyph.dest_rect.w)
        .sum();
    let b_width: f64 = overlay
        .iter()
        .filter(|glyph| glyph.snapshot_id.visual_line_ordinal == 8)
        .map(|glyph| glyph.dest_rect.w)
        .sum();
    let total: f64 = overlay.iter().map(|glyph| glyph.dest_rect.w).sum();

    assert!(
        (a_width - 10.0).abs() < 1e-6,
        "刚删的 a 第一帧必须完整显示 10px，实际 {a_width}（被预吞了）"
    );
    assert!(
        (b_width - mid_b_width).abs() < 1e-6,
        "b 必须保持上一帧剩下的宽度（{mid_b_width}px），实际 {b_width}"
    );
    assert!(
        (total - (10.0 + mid_b_width)).abs() < 1e-6,
        "第一帧 overlay 总宽必须等于当前屏幕状态（约 {}px），实际 {total}",
        10.0 + mid_b_width
    );
}

/// Issue #826 评论 18 阻塞 3：`pending_reveal_ranges` 必须**逐视觉行**用自己那条
/// segment 的 reveal boundary。
///
/// 反例：
/// ```text
/// 第 1 行 changed segment: x = 80..100，长度 20
/// 第 2 行 changed segment: x = 0..100，长度 100
/// distance = 30
/// ```
/// 实际是「第 1 行 20px 全露完、第 2 行只露了 10px」。只取第一条 boundary(=100)
/// 去判断第 2 行，会把第 2 行大量甚至全部 glyph 误判成已完全露出、从
/// `excluded_new` 提前移除 —— 屏幕上还没吐出来的字可能被 Reflow 画出来。
#[test]
fn pending_reveal_ranges_use_each_visual_line_own_boundary() {
    let now = Instant::now();
    let target = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 80.0), cluster(1, 2, 90.0)],
        ),
        PreparedLineSnapshot::stub_for_tests(
            1,
            20.0,
            0,
            vec![cluster(2, 3, 0.0), cluster(3, 4, 10.0), cluster(4, 5, 20.0)],
        ),
    ]);
    let mut state = EditFrontierState::begin_insert(
        String::new(),
        target,
        String::from("a\nb"),
        vec![(0, 5)],
        OffsetMap::from_single_edit(0, (0, 0), 5),
        now,
        160,
    );

    // 第 1 行 segment 长度 20、第 2 行 10+10+10 = 30，总计 50。
    // distance = 30 -> 第 1 行全露完，第 2 行只露了第一个 10px。
    state.reveal.travelled = 30.0;
    let pending = state.pending_reveal_ranges(0.0);

    // 第 1 行的 cluster（0..2）不应再 pending。
    assert!(
        !pending.iter().any(|&(s, e)| s < 2 && 2 <= e),
        "第 1 行已全露完，不应再 pending；实际 pending = {pending:?}"
    );
    // 第 2 行只露出第一个 cluster（2..3），后面两个必须仍 pending。
    assert!(
        pending.iter().any(|&(s, e)| s <= 3 && 4 <= e),
        "第 2 行第二个 cluster（3..4）必须仍 pending（它在自己那行的 boundary 之后）"
    );
    assert!(
        pending.iter().any(|&(s, e)| s <= 4 && 5 <= e),
        "第 2 行第三个 cluster（4..5）必须仍 pending"
    );
}

/// Issue #826 评论 20 阻塞：上一笔还只吐了一半，下一键触发自动换行。
///
/// 绝对距离在这条路上根本没有意义：
/// ```text
/// 第一笔插 X，X 的 glyph 在 y=0、x=90..100
/// 80ms 时 X 已露 8.75/10px，还剩 1.25px pending
/// 马上输入 Y，Qt 重排把整个词换到下一行：X 变成 y=20、x=0..10
/// ```
/// 旧实现把 `travelled = 8.75` 塞进新 path，于是 X 那 8.75px 从
/// `(y=0, x=90..98.75)` 跳到 `(y=20, x=0..8.75)`，没有任何过渡。
///
/// Reflow 也救不了：X 还没吐完 -> 仍在 `pending_reveal_ranges` -> 仍归 Reveal
/// -> Reflow 明确排除 X，而 Reveal 又已经把 path 重建到新行。
///
/// 契约：第二笔刚进入的**同一帧**，X 那 8.75px 必须仍在 old screen position
/// （由 carry overlay 从旧位置补间），刚输入的 Y 必须完整 hidden。
#[test]
fn partially_revealed_text_rewraps_without_jumping_to_new_line() {
    let now = Instant::now();
    // 第一笔：正文 `aX`，X 的文档矩形是 (90, 0, 10, 20)。
    // `stub_for_tests` 用 cluster 的最小 x 当 `visual_x`，所以要用 x=0 的
    // 正文 cluster 把 `visual_x` 钉在 0，X 的文档 x 才是 90。
    let mut state = EditFrontierState::begin_insert(
        String::from("a"),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 90.0)],
        )]),
        String::from("aX"),
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
        160,
    );

    // 半程遮罩只盖住 boundary 右边那 1.25px，所以「已露出宽度」=
    // 遮罩左边界 - glyph 左边界。
    let half = instant_at(now, 80);
    let mid = state.sample(half);
    let mid_visible: f64 = state
        .hidden_new_text_rects(&mid)
        .iter()
        .filter(|rect| (rect.x + rect.w - 100.0).abs() < 1e-9)
        .map(|rect| rect.x - 90.0)
        .sum();
    assert!(
        (8.75 - mid_visible).abs() < 0.2,
        "半程时 X 应已露出约 8.75px（80ms / ease_out_cubic(0.5)），实际 {mid_visible}"
    );

    // 第二笔：输入 Y，Qt 重排把 X 挤到第二行 —— X 变成 (0, 20, 10, 20)，
    // Y 是 (10, 20, 10, 20)。
    let rewrapped = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(
            1,
            20.0,
            0,
            vec![cluster(1, 2, 0.0), cluster(2, 3, 10.0)],
        ),
    ]);
    let prev_target_to_new = OffsetMap::from_single_edit(2, (2, 2), 1);
    state.extend_insert(
        rewrapped,
        String::from("aXY"),
        vec![(2, 3)],
        &prev_target_to_new,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    // 就在第二笔刚进入的**同一帧**采样。
    let sample = state.sample(half);
    let carried = state.reveal_carried_glyphs(&sample);
    assert_eq!(
        carried.len(),
        1,
        "换行后已可见的那一段必须由 carry overlay 接管，实际 carry 数 = {}",
        carried.len()
    );
    let glyph = carried[0].dest_rect.clone();
    assert!(
        (glyph.x - 90.0).abs() < 1e-6 && (glyph.y - 0.0).abs() < 1e-6,
        "X 已露出的那一段第一帧必须仍在 old screen position (90, 0)，实际 ({}, {})",
        glyph.x,
        glyph.y
    );
    assert!(
        (glyph.w - 8.75).abs() < 0.2,
        "carry 只画上一帧已经看见的宽度（约 8.75px），实际 {}",
        glyph.w
    );
    assert!(
        (glyph.x - 0.0).abs() > 1e-6 || (glyph.y - 20.0).abs() > 1e-6,
        "绝不能第一帧就已经落在新的 (0, 20) 位置"
    );

    // canonical 那边必须给 carry 让位：X 的新位置被 clip 掉，由 overlay 画。
    let target_rects = state.reveal_carried_target_rects(&sample);
    assert_eq!(target_rects.len(), 1, "carry 必须登记 canonical 目标位置");
    assert!(
        (target_rects[0].0.x - 0.0).abs() < 1e-6 && (target_rects[0].0.y - 20.0).abs() < 1e-6,
        "canonical 要让位的必须是 X 的新位置 (0, 20)，实际 {:?}",
        target_rects[0].0
    );

    // 刚输入的 Y 第一帧必须完整 hidden。
    let hidden = state.hidden_new_text_rects(&sample);
    assert!(
        hidden.iter().any(|rect| {
            (rect.x - 10.0).abs() < 1e-9
                && (rect.x + rect.w - 20.0).abs() < 1e-9
                && (rect.y - 20.0).abs() < 1e-9
        }),
        "刚输入的 Y（x 10..20, y 20）必须完整 hidden，实际 hidden = {hidden:?}"
    );
    // carry 期间这段字仍归 Reveal：Reflow 不能同时搬它，否则两个动画抢同一块像素。
    assert!(
        state
            .pending_reveal_ranges(sample.progress)
            .iter()
            .any(|&(s, e)| s <= 1 && 2 <= e),
        "被 carry 接管的 X 仍必须算 pending（Reflow 不能碰）"
    );
}

/// Issue #826 评论 20 阻塞：不换行、只是新 patch 插在旧前沿之前。
///
/// 这条反例直接咬住根因 —— `travelled = 8.75` 没有任何字符身份：
/// ```text
/// 旧 reveal path 只有 X = x 100..110，80ms 时屏幕上真正露出 X 的前 8.75px
/// 同一 burst 又来一条新 patch Y，位置在 X 之前（new target：Y = 0..10, X = 100..110）
/// 视觉顺序变成 Y(10px) -> X(10px)，total = 20
/// ```
/// 旧实现继承 `travelled = 8.75`，于是第二笔第一帧是「Y 露 8.75px、X 完全隐藏」
/// —— 上一帧已经看见的 X 突然消失，而刚插入的 Y 一进来就露了 87.5%。
///
/// 契约：第一帧 X 那 8.75px 仍在，Y 完整 hidden。
#[test]
fn inserting_patch_before_existing_reveal_preserves_visible_owner() {
    let now = Instant::now();
    // 第一笔：正文 `aX`，X 的文档矩形是 (100, 0, 10, 20)。
    let mut state = EditFrontierState::begin_insert(
        String::from("a"),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 100.0)],
        )]),
        String::from("aX"),
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
        160,
    );

    let half = instant_at(now, 80);
    let mid = state.sample(half);
    let mid_visible: f64 = state
        .hidden_new_text_rects(&mid)
        .iter()
        .filter(|rect| (rect.x + rect.w - 110.0).abs() < 1e-9)
        .map(|rect| rect.x - 100.0)
        .sum();
    assert!(
        (8.75 - mid_visible).abs() < 0.2,
        "半程时 X 应已露出约 8.75px，实际 {mid_visible}"
    );

    // 第二笔：新 patch Y 插在 X **之前** —— Y = x 0..10，X 仍在 x 100..110。
    let reordered = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 0.0), cluster(2, 3, 100.0)],
    )]);
    // 在 byte 1 插入 Y -> 旧的 (1,2) 变成新的 (2,3)。
    let prev_target_to_new = OffsetMap::from_single_edit(2, (1, 1), 1);
    state.extend_insert(
        reordered,
        String::from("aYX"),
        vec![(1, 2)],
        &prev_target_to_new,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    let sample = state.sample(half);
    // 视觉顺序变了 -> 不能继承绝对距离。前沿时钟必须从 0 起。
    assert_eq!(
        state.reveal.travelled, 0.0,
        "新 patch 插到旧前沿之前时，scalar distance 不再代表「谁已露出」，必须从 0 起"
    );

    // 上一帧已经看见的 X 那 8.75px 不能消失。
    let carried = state.reveal_carried_glyphs(&sample);
    assert_eq!(
        carried.len(),
        1,
        "已可见的那一段 X 必须由 carry overlay 接管，实际 carry 数 = {}",
        carried.len()
    );
    assert!(
        (carried[0].dest_rect.x - 100.0).abs() < 1e-6,
        "X 已露出的那一段必须仍在 x = 100，实际 {}",
        carried[0].dest_rect.x
    );
    assert!(
        (carried[0].dest_rect.w - 8.75).abs() < 0.2,
        "carry 只画上一帧已经看见的宽度（约 8.75px），实际 {}",
        carried[0].dest_rect.w
    );

    // 刚插入的 Y 不能凭空露出 8.75px —— 它第一帧必须完整 hidden。
    let hidden = state.hidden_new_text_rects(&sample);
    assert!(
        hidden
            .iter()
            .any(|rect| { (rect.x - 0.0).abs() < 1e-9 && (rect.x + rect.w - 10.0).abs() < 1e-9 }),
        "刚插入的 Y（x 0..10）必须完整 hidden，实际 hidden = {hidden:?}"
    );
    // X 已经被 carry 接管，前沿不能再遮罩它的 canonical 位置。
    assert!(
        !hidden.iter().any(|rect| (rect.x - 100.0).abs() < 1e-9),
        "X 归 carry overlay 所有，前沿不得再遮罩它，实际 hidden = {hidden:?}"
    );
}

/// Issue #826 评论 21 BLOCKER 2：已完整露出的字不能被下一笔又遮回去。
///
/// 慢路径 retarget 之后 `reveal.travelled` 从 0 起。如果「已经完整露出、已经
/// 交还给 canonical 的字」还留在 `reveal.regions` 里，第三笔快速输入到来时
/// path 还没走到它 —— `visible_width = 0`，它既不进可见采样，也没有别的入口，
/// 于是下一轮重建时它又被 `FrontierMask` 从头遮回去。视觉债回生。
///
/// 三笔：
/// 1. `a` + `XY`（x 10..30）→ 80ms 时 X 全露、Y 露 7.5px；
/// 2. 输入 Z 触发 rewrap → X 挪到 y=20、Y 挪到 y=20、Z 新增在 y=20；
///    慢路径把 X 判为 settled（彻底退出 reveal.regions）、Y 判为 carried；
/// 3. 立刻再输入 W —— X 绝不能重新进 mask。
#[test]
fn settled_reveal_does_not_get_masked_again_on_immediate_third_retarget() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_insert(
        String::from("a"),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            1,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0), cluster(2, 3, 20.0)],
        )]),
        String::from("aXY"),
        vec![(1, 2), (2, 3)],
        OffsetMap::from_single_edit(1, (1, 1), 2),
        now,
        160,
    );

    // 第二笔：插入 Z，同时 Qt 重排把 XY 挪到第二行。
    let half = instant_at(now, 80);
    let after_second = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(
            2,
            20.0,
            0,
            vec![cluster(1, 2, 0.0), cluster(2, 3, 10.0), cluster(3, 4, 20.0)],
        ),
    ]);
    state.extend_insert(
        after_second.clone(),
        String::from("aXYZ"),
        vec![(3, 4)],
        &OffsetMap::from_single_edit(3, (3, 3), 1),
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    // 机制断言：已完整露出的 X 必须**彻底退出** scalar reveal path，
    // 否则它会重新吃 distance 并在第三笔被遮回去。
    assert_eq!(
        state.new_ranges(),
        vec![(3, 4)],
        "settled 的 X 与 carried 的 Y 都必须退出 reveal.regions，只剩真正还要遮罩的 Z"
    );
    assert_eq!(state.reveal_settled, vec![(1, 2)]);
    assert_eq!(
        state
            .reveal_carried
            .iter()
            .map(|carried| carried.range)
            .collect::<Vec<_>>(),
        vec![(2, 3)]
    );

    // 第三笔：紧接同一个时刻再输入 W。
    let after_third = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(
            2,
            20.0,
            0,
            vec![
                cluster(1, 2, 0.0),
                cluster(2, 3, 10.0),
                cluster(3, 4, 20.0),
                cluster(4, 5, 30.0),
            ],
        ),
    ]);
    state.extend_insert(
        after_third,
        String::from("aXYZW"),
        vec![(4, 5)],
        &OffsetMap::from_single_edit(4, (4, 4), 1),
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    // 第三笔之后，scalar path 里不允许再出现 X（byte 1..2）或 Y（byte 2..3）。
    assert!(
        state
            .reveal
            .regions
            .iter()
            .all(|region| region.range.0 >= 3),
        "X / Y 已退出前沿，第三笔的 scalar path 只能覆盖真正还要打开的 Z/W，实际 {:?}",
        state
            .reveal
            .regions
            .iter()
            .map(|region| region.range)
            .collect::<Vec<_>>()
    );

    // 屏幕上真正画的遮罩里也不能有 X 的新位置（y=20, x 0..10）与 Y 的新位置
    // （y=20, x 10..20）—— 上一帧它们已经完整可见，不能被 FrontierMask 挖回去。
    let hidden = state.hidden_new_text_rects(&state.sample(half));
    for (x, label) in [(0.0, "X"), (10.0, "Y")] {
        assert!(
            !hidden
                .iter()
                .any(|rect| (rect.y - 20.0).abs() < 1e-9 && (rect.x - x).abs() < 1e-9),
            "{label} 已经在屏幕上完整可见，第三笔不得用 FrontierMask 把它遮回去（x={x}），实际 hidden = {hidden:?}"
        );
    }
}

/// Issue #826 评论 21 BLOCKER 3：settled / carried 不能继续吃 scalar path 的 distance。
///
/// `hidden_new_text_rects` 虽然跳过它们，但 `FrontierLayer::advanced()` 仍按
/// `total_length()` 推进。若 settled + carried 还留在 path 里，前沿要先空跑过
/// 它们的长度，新输入的字在 160ms 动画的前三四十毫秒里完全不动 —— 与评论 19
/// 删掉 Conceal 幽灵 path 是同一类问题，只是这次出现在 Reveal。
///
/// 同一构造下 scalar path 只能等于「还需要 FrontierMask 从 0 打开」的内容。
#[test]
fn settled_and_carried_ranges_do_not_consume_scalar_reveal_distance() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_insert(
        String::from("a"),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            1,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0), cluster(2, 3, 20.0)],
        )]),
        String::from("aXY"),
        vec![(1, 2), (2, 3)],
        OffsetMap::from_single_edit(1, (1, 1), 2),
        now,
        160,
    );

    let half = instant_at(now, 80);
    let after_second = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(
            2,
            20.0,
            0,
            vec![cluster(1, 2, 0.0), cluster(2, 3, 10.0), cluster(3, 4, 20.0)],
        ),
    ]);
    state.extend_insert(
        after_second,
        String::from("aXYZ"),
        vec![(3, 4)],
        &OffsetMap::from_single_edit(3, (3, 3), 1),
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    // 只有 Z（10px）真正需要前沿打开。X 10px 已交还 canonical、Y 10px 由 carry
    // 自己补间，两者都不该出现在 scalar path 的长度里。
    assert!(
        (state.reveal.total_length() - 10.0).abs() < 1e-9,
        "scalar reveal path 只能覆盖还需要 FrontierMask 打开的 Z（10px），实际 {}px（若含 X+Y 应是 30px）",
        state.reveal.total_length()
    );

    // 并且前沿必须真的从那段内容的最左端开始 —— 不是先空跑一段再开始。
    let mid = state.sample(half);
    let hidden = state.hidden_new_text_rects(&mid);
    assert!(
        hidden
            .iter()
            .any(|rect| (rect.y - 20.0).abs() < 1e-9 && (rect.x - 20.0).abs() < 1e-9),
        "刚插入的 Z 第一帧必须完整 hidden（y=20, x 20..30），实际 hidden = {hidden:?}"
    );
}

/// Issue #826 评论 21 结构偏差：**完整露出**的字进 settled，**部分露出**的才 carry。
///
/// 一次 rewrap 会把这一轮已经吐完的所有旧插入字全部挪位置。如果判据里带上
/// 「位置有没有变」，就会给每一个已完整露出的字都造一个 `RevealCarriedPrefix` ——
/// 一次 rewrap 50 个字就长出 50 个 carry，状态量又跟「历史已露出的字数」一起
/// 膨胀，也就真的不满足「每条 region 最多一个边界 cluster carry」。
///
/// 完整露出的字位置真变了就交给 Reflow（`request.base_snapshot` 里就是它当前的
/// 屏幕位置），只有前沿边界上那一个部分露出的 cluster 需要 carry。
#[test]
fn fully_visible_moved_clusters_are_released_to_reflow_not_carried() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_insert(
        String::from("a"),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            1,
            0.0,
            0,
            vec![
                cluster(0, 1, 0.0),
                cluster(1, 2, 10.0),
                cluster(2, 3, 20.0),
                cluster(3, 4, 30.0),
                cluster(4, 5, 40.0),
            ],
        )]),
        String::from("aWXYZ"),
        vec![(1, 2), (2, 3), (3, 4), (4, 5)],
        OffsetMap::from_single_edit(1, (1, 1), 4),
        now,
        160,
    );

    // 80ms/160ms -> advance 35px，boundary 45：W/X/Y 全露，Z 只露 5px。
    let half = instant_at(now, 80);
    // 第二笔：插入 W2 并触发 rewrap —— W/X/Y/Z 全部挪到第二行且 x 前移。
    let after_second = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(
            2,
            20.0,
            0,
            vec![
                cluster(1, 2, 0.0),
                cluster(2, 3, 10.0),
                cluster(3, 4, 20.0),
                cluster(4, 5, 30.0),
                cluster(5, 6, 40.0),
            ],
        ),
    ]);
    state.extend_insert(
        after_second,
        String::from("aXYZW2"),
        vec![(5, 6)],
        &OffsetMap::from_single_edit(5, (5, 5), 1),
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    assert_eq!(
        state.reveal_settled,
        vec![(1, 2), (2, 3), (3, 4)],
        "W/X/Y 虽然被 rewrap 挪了位置，但已完整露出 —— 应释放给 Reflow，而不是 carry"
    );
    assert_eq!(
        state.reveal_carried.len(),
        1,
        "只有前沿边界上那个部分露出的 cluster 能 carry，实际 carry 了 {} 个（范围 {:?}）",
        state.reveal_carried.len(),
        state
            .reveal_carried
            .iter()
            .map(|carried| carried.range)
            .collect::<Vec<_>>()
    );
    assert_eq!(state.reveal_carried[0].range, (4, 5));
    assert_eq!(state.new_ranges(), vec![(5, 6)]);
    assert!(
        (state.reveal.total_length() - 10.0).abs() < 1e-9,
        "scalar path 只剩新插入的 W2，实际 {}px",
        state.reveal.total_length()
    );
}

/// Issue #826 评论 21：`reveal.regions` 整条为空但 carry 还在补间时，
/// 前沿**不能**被判为已结束。
///
/// settled / carried 退出 scalar path 之后，可能出现 `reveal.regions` 为空
/// 而 `reveal_carried` 非空的状态。此时 `reveal.is_advanced_done()` 恒为 true。
/// 若 `is_finished` 只看它，`coordinator::tick` 会在 carry 走完之前把整轮
/// 前沿丢掉 —— carry overlay 中途消失、X 从半吐直接变成完整（评论 20 修掉的
/// 瞬移在真实渲染链里复活）。
#[test]
fn active_reveal_carry_keeps_burst_alive_when_scalar_path_is_empty() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_insert(
        String::from("a"),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            1,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 90.0)],
        )]),
        String::from("aX"),
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
        160,
    );

    // 80ms：X 露 8.75px / 10px，仍在吐。
    let half = instant_at(now, 80);
    assert!(
        (state.reveal.advanced(state.sample(half).progress) - 8.75).abs() < 1e-6,
        "半程时前沿应推进到 8.75px，实际 {}",
        state.reveal.advanced(state.sample(half).progress)
    );

    // 触发行内重排：X 挪到 (0, 20)。本笔没有新插入文字，所以重排之后
    // 「还需要 FrontierMask 打开」的内容为空 —— scalar path 整条为空，
    // 唯一还归 Reveal 的就是那条 carry。
    let rewrapped = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(2, 20.0, 0, vec![cluster(1, 2, 0.0)]),
    ]);
    state.extend_insert(
        rewrapped,
        String::from("aX"),
        Vec::new(),
        &OffsetMap::build("aX", "aX"),
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    assert!(
        state.reveal.regions.is_empty(),
        "本笔没有新字要遮罩，scalar path 应为空，实际 {:?}",
        state
            .reveal
            .regions
            .iter()
            .map(|region| region.range)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        state.reveal_carried.len(),
        1,
        "X 半吐且被挪走，必须由 carry 接管"
    );

    assert!(
        !state.is_finished(half),
        "carry 还在补间时整轮前沿不能结束（scalar path 已空，reveal.is_advanced_done 恒为 true）"
    );
    // 注意 retarget 把 `started_at` 重置成了 `half`，所以「走完」是 half + duration。
    assert!(
        state.is_finished(instant_at(half, 160)),
        "动画走完后 carry 交回 canonical，整轮才结束"
    );
}

/// Issue #826 评论 22 BLOCKER：单前沿已经越过整条 A region、但后面 B region
/// 还没走完时，A **必须**继续被采成「当前屏幕事实」。
///
/// 稳定反例（一轮两条不相邻 Insert patch，#826 从评论 8 起就要求支持 Core 多
/// DisplayPatch）：A = 10px、B = 10px 共用单前沿总长 20px，前沿走到 15px 时
/// 屏幕真实状态是「A 完整显示、B 只显示一半」，整轮仍未 finished。
///
/// 旧代码在 `sample_visible_reveal` 里对「走完的 region」直接 `continue`
/// （那句注释「屏幕上没有它的像素了」对吞字成立、对吐字是反的），于是采不到
/// A 已全露这个事实：A 既不进 `settled` 也不进 `carried`，却因为仍在
/// `reveal.regions` 里而留在 `merged` -> `mask_ranges`，配合 `travelled = 0.0`
/// 让第二笔第一帧把 A 完整遮回去。
///
/// 而且 `begin_or_extend_reflow` 是在 retarget 之后才算 pending，A 重新进入
/// scalar region 就又出现在 `pending_reveal_ranges()` 里，Reflow 也不接它 ——
/// `canonical 被重新 mask + 没有 carry + 被排除出 Reflow => A 真消失`。
#[test]
fn fully_completed_scalar_region_stays_visible_when_later_region_is_still_pending() {
    let now = Instant::now();
    // `a` + A(1,2) + gap(2,3) + B(3,4)。A 与 B 之间夹着一个 unchanged 的 gap，
    // 所以 `normalize_ranges` 不会把它们并成一段，正好两条 region。
    let mut state = EditFrontierState::begin_insert(
        String::from("a"),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            1,
            0.0,
            0,
            vec![
                cluster(0, 1, 0.0),
                cluster(1, 2, 10.0),
                cluster(2, 3, 20.0),
                cluster(3, 4, 30.0),
            ],
        )]),
        String::from("aAZB"),
        vec![(1, 2), (3, 4)],
        OffsetMap::from_single_edit(1, (1, 1), 3),
        now,
        160,
    );
    assert_eq!(
        state.reveal.regions.len(),
        2,
        "两条不相邻 patch 必须各有一条 region"
    );

    // 把单前沿直接摆到 15px：A(0..10) 走完、B(10..20) 只走 5px。
    let half = instant_at(now, 80);
    state.reveal.travelled = 15.0;
    state.started_at = half;
    assert!(
        !state.is_finished(half),
        "B 还没走完，整轮必须仍在进行中 —— 这正是 A '已完整显示但仍属 scalar region' 的场景"
    );

    // 第二笔：在文末插入 Y，同时 Qt 重排把整段挪到第二行（几何变化 -> 慢路径）。
    let after_second = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(
            2,
            20.0,
            0,
            vec![
                cluster(1, 2, 0.0),
                cluster(2, 3, 10.0),
                cluster(3, 4, 20.0),
                cluster(4, 5, 30.0),
            ],
        ),
    ]);
    state.extend_insert(
        after_second.clone(),
        String::from("aAZBY"),
        vec![(4, 5)],
        &OffsetMap::from_single_edit(4, (4, 4), 1),
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    // A 已经完整露出 —— 它必须被采到并判为 settled，然后从 scalar path 扣掉。
    assert_eq!(
        state.reveal_settled,
        vec![(1, 2)],
        "A 早已在屏幕上完整显示，必须进 settled（位置变了则由 Reflow 从旧位置补过去）"
    );
    assert_eq!(
        state.new_ranges(),
        vec![(4, 5)],
        "A(settled) 与 B(carried) 都必须退出 scalar path，只剩真正还要遮罩的 Y"
    );
    // B 只露了一半且被挪走 -> carry 接管，从旧屏幕位置 (30,0) 补间到 (20,20)。
    assert_eq!(state.reveal_carried.len(), 1);
    assert_eq!(state.reveal_carried[0].range, (3, 4));
    let carried = &state.reveal_carried[0];
    assert!(
        (carried.from_rect.x - 30.0).abs() < 1e-9 && (carried.from_rect.y - 0.0).abs() < 1e-9,
        "carry 必须从 B 上一帧的屏幕位置 (30, 0) 出发，实际 ({}, {})",
        carried.from_rect.x,
        carried.from_rect.y
    );
    assert!(
        (carried.to_rect.x - 20.0).abs() < 1e-9 && (carried.to_rect.y - 20.0).abs() < 1e-9,
        "carry 终点是 B 在最新 target 的位置 (20, 20)，实际 ({}, {})",
        carried.to_rect.x,
        carried.to_rect.y
    );
    assert!(
        (carried.visible_width - 5.0).abs() < 1e-9,
        "B 上一帧只露了 5px，实际 {}",
        carried.visible_width
    );

    // 机制断言：屏幕上真正画的遮罩里不能有 A 的新位置 (y=20, x 0..10)；
    // 新插入的 Y (y=20, x 30..40) 必须完整 hidden。
    let sample = state.sample(half);
    let hidden = state.hidden_new_text_rects(&sample);
    assert!(
        !hidden
            .iter()
            .any(|rect| (rect.y - 20.0).abs() < 1e-9 && (rect.x - 0.0).abs() < 1e-9),
        "A 在上一帧已经完整显示，第二笔不得用 FrontierMask 把它遮回去，实际 hidden = {hidden:?}"
    );
    assert!(
        hidden.iter().any(|rect| {
            (rect.y - 20.0).abs() < 1e-9
                && (rect.x - 30.0).abs() < 1e-9
                && (rect.w - 10.0).abs() < 1e-9
        }),
        "新插入的 Y 第一帧必须完整 hidden，实际 hidden = {hidden:?}"
    );

    // Reflow 必须能接 A：A 不该再出现在 pending reveal 里。
    let pending = state.pending_reveal_ranges(sample.progress);
    assert!(
        !pending
            .iter()
            .any(|&(start, end)| start < 2 && 2 <= end),
        "A 已完整露出，不能再被算成 pending（否则 Reflow 也接不了它，它就真消失了），实际 pending = {pending:?}"
    );
}

/// Issue #826 评论 22 同类漏改：identity preflight 必须和 retarget 用**同一份**
/// owner 集合。
///
/// `can_extend_identity` 之前只看 `reveal.regions`，而 `extend_insert` /
/// `extend_replace` 用的是 `active_reveal_owned_ranges()`（scalar regions +
/// carry ranges）。评论 21 之后 carry 已退出 `reveal.regions`，于是出现
/// 「scalar path 已空 + carry 非空」的状态：预检检查的是**空集合**，必然 true；
/// 而真正 retarget 里的 `map_ranges_forward` 是 filter_map —— 某个 carry identity
/// 在本次 OffsetMap 里映不出来时，它会被**静默丢掉**，屏幕上那部分已可见像素凭空消失。
#[test]
fn can_extend_identity_checks_carried_reveal_ranges_when_scalar_path_is_empty() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_insert(
        String::from("a"),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            1,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 90.0)],
        )]),
        String::from("aX"),
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
        160,
    );
    let half = instant_at(now, 80);
    let rewrapped = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(2, 20.0, 0, vec![cluster(1, 2, 0.0)]),
    ]);
    state.extend_insert(
        rewrapped.clone(),
        String::from("aX"),
        Vec::new(),
        &OffsetMap::build("aX", "aX"),
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );
    assert!(
        state.reveal.regions.is_empty() && state.reveal_carried.len() == 1,
        "前提：scalar path 为空，唯一还归 Reveal 的是那条 carry"
    );
    assert_eq!(state.active_reveal_owned_ranges(), vec![(1, 2)]);

    let make_request = |offset_map: OffsetMap| EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: snapshot(Vec::new()),
        target_snapshot: snapshot(Vec::new()),
        deleted_ranges: Vec::new(),
        inserted_ranges: Vec::new(),
        offset_map,
        base_text: String::from("aX"),
        target_text: String::from("aX"),
        conceal_direction: ConcealDirection::Forward,
        now: half,
    };

    // carry 的 range (1,2) 正好是本次编辑区间 -> OffsetMap 对它没有映射。
    let unmapable = make_request(OffsetMap::from_single_edit(2, (1, 2), 0));
    assert!(
        !state.can_extend_identity(EditFrontierKind::Insert, &unmapable),
        "carry 的 range 在本次 OffsetMap 里映不出来，必须换 burst；否则 retarget 会静默丢掉它"
    );

    // 反过来，能完整映射时仍然允许 extend（别把这条路径判死）。
    let mappable = make_request(OffsetMap::from_single_edit(2, (2, 2), 1));
    assert!(
        state.can_extend_identity(EditFrontierKind::Insert, &mappable),
        "carry 的 range 能完整映射时应当继续并入当前 burst"
    );
}

/// Issue #826 评论 23 BLOCKER：fast path 不能把「同一个屏幕位置」当成
/// 「同一批字符」。
///
/// `shares_geometry_prefix()` 只比较 `(x, y, h)`，它**不知道**某个像素原来属于
/// 哪个 byte range。等宽 10px 下「新字恰好占了旧字原来的位置」就会漏过：
/// ```text
/// 第一笔：aX    X range=1..2  X rect = x 10..20   old reveal path = 10..20
/// 80ms/160ms -> inherited = 8.75px，屏幕上真正已看见 X 的 x 10..18.75
/// 马上在 X 前插 Y -> aYX
/// 新排版：Y range=1..2 rect = x 10..20 ；X 映成 range=2..3 rect = x 20..30
/// merged = Y(1..2) + mappedX(2..3) -> normalize -> 1..3
/// 新 probe path = x 10..30
/// 旧 path distance 8.75 -> x 18.75 ；新 probe path distance 8.75 -> 也 x 18.75
/// => shares_geometry_prefix(probe) == true
/// ```
/// 于是走 fast path、`travelled = 8.75` —— 但这 8.75px 在新 path 上已经是
/// **Y** 的 x 10..18.75。第二笔第一帧从「X 已露 8.75px、Y 不存在」变成
/// 「Y 已露 8.75px、X 完全 hidden」：旧字已露出的像素瞬间转移给刚输入的新字。
///
/// 现有 `inserting_patch_before_existing_reveal_preserves_visible_owner` 故意把
/// 旧/新几何做得差别明显（旧 X = x100..110、新 Y = x0..10），所以第一个判据就能
/// 发现；这条反例专门咬「几何一样、身份换人」。
///
/// 契约：fast path 必须**同时**满足两个判据 ——
/// 1. 完整新 path 的前 N 像素没换屏幕位置；
/// 2. 旧 owner 单独映到新 revision 后，前 N 像素仍然是旧 owner 自己。
#[test]
fn same_geometry_prefix_with_different_character_owner_must_use_slow_retarget() {
    let now = Instant::now();
    // 第一笔：正文 `aX`，X 的文档矩形是 (10, 0, 10, 20)。
    // 行内要有 x=0 的 cluster 把 `visual_x` 钉在 0，X 的文档 x 才是 10。
    let mut state = EditFrontierState::begin_insert(
        String::from("a"),
        snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
        )]),
        String::from("aX"),
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
        160,
    );

    let half = instant_at(now, 80);
    let mid = state.sample(half);
    // `hidden_new_text_rects` 返回 boundary **右边**被遮的部分，
    // 所以「已露出宽度」= 遮罩左边界 - glyph 左边界。
    let mid_visible: f64 = state
        .hidden_new_text_rects(&mid)
        .iter()
        .filter(|rect| (rect.x + rect.w - 20.0).abs() < 1e-9)
        .map(|rect| rect.x - 10.0)
        .sum();
    assert!(
        (8.75 - mid_visible).abs() < 0.2,
        "半程时 X 应已露出约 8.75px，实际 {mid_visible}"
    );

    // 第二笔：在 X **之前**插入 Y。新排版 Y 占 x 10..20、X 挪到 x 20..30 ——
    // Y 恰好占住 X 原来的屏幕位置，probe 的前 8.75px 几何与旧 path 完全一致。
    let shifted = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0), cluster(2, 3, 20.0)],
    )]);
    // 在 byte 1 插入 Y -> 旧的 (1,2) 变成新的 (2,3)。
    let prev_target_to_new = OffsetMap::from_single_edit(2, (1, 1), 1);
    state.extend_insert(
        shifted,
        String::from("aYX"),
        vec![(1, 2)],
        &prev_target_to_new,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    let sample = state.sample(half);
    assert_eq!(
        state.reveal.travelled, 0.0,
        "旧 owner 映到新 revision 后位置变了，scalar distance 已被换人，必须从 0 起"
    );

    // X 以**新**身份 (2,3) 进入 carry，补间起点仍是它上一帧的真实屏幕位置。
    assert_eq!(state.reveal_carried.len(), 1);
    let carried = &state.reveal_carried[0];
    assert_eq!(
        carried.range,
        (2, 3),
        "carry 记录的是 X 在最新 target 里的身份"
    );
    assert!(
        (carried.from_rect.x - 10.0).abs() < 1e-6 && carried.from_rect.w > 0.0,
        "carry 起点必须仍是 X 上一帧的 x = 10，实际 {:?}",
        carried.from_rect
    );
    assert!(
        (carried.to_rect.x - 20.0).abs() < 1e-6,
        "carry 终点是 X 的新位置 x = 20，实际 {:?}",
        carried.to_rect
    );
    assert!(
        (8.75 - carried.visible_width).abs() < 0.2,
        "carry 只拥有上一帧已经看见的宽度（约 8.75px），实际 {}",
        carried.visible_width
    );

    // 第一帧 X 那 8.75px 必须仍画在旧位置。
    let glyphs = state.reveal_carried_glyphs(&sample);
    assert_eq!(glyphs.len(), 1);
    assert!(
        (glyphs[0].dest_rect.x - 10.0).abs() < 1e-6 && (glyphs[0].dest_rect.w - 8.75).abs() < 0.2,
        "X 已露出的 8.75px 第一帧必须仍在 x = 10 宽 8.75，实际 {:?}",
        glyphs[0].dest_rect
    );

    // 刚插入的 Y 必须完整 hidden —— 它绝不能继承 X 那 8.75px。
    let hidden = state.hidden_new_text_rects(&sample);
    assert!(
        hidden
            .iter()
            .any(|rect| (rect.x - 10.0).abs() < 1e-9 && (rect.x + rect.w - 20.0).abs() < 1e-9),
        "刚插入的 Y（x 10..20）必须完整 hidden，实际 hidden = {hidden:?}"
    );
    // X 归 carry 所有，前沿不得再遮罩它在 canonical 位置（x 20..30）。
    assert!(
        !hidden.iter().any(|rect| (rect.x - 20.0).abs() < 1e-9),
        "X 已由 carry overlay 所有，前沿不得遮罩它的 canonical 位置，实际 hidden = {hidden:?}"
    );
}
