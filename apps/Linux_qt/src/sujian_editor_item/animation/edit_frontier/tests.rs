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
#[test]
fn extend_delete_maps_old_range_back_to_base_coordinates() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_delete(
        empty_snapshot(),
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
    state.extend_delete(
        empty_snapshot(),
        String::from("ABCE"),
        vec![(3, 4)],
        &empty_snapshot(),
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
#[test]
fn extend_replace_accumulates_both_sides() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_replace(
        empty_snapshot(),
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
    state.extend_replace(
        empty_snapshot(),
        String::from("AXBCDEZ"),
        vec![(5, 6)],
        vec![(6, 7)],
        &empty_snapshot(),
        &base_to_current,
        &prev_target_to_new,
        &[],
        ConcealDirection::Backward,
        half,
    );
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
