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
        empty_snapshot(),
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
        empty_snapshot(),
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
        empty_snapshot(),
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
        empty_snapshot(),
        String::from("abc"),
        vec![(2, 3)],
        &prev_target_to_new,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );
    // Issue #826 评论 9 阻塞 3：相邻但来源不同的 track **不合并** ——
    // 动画状态按编辑身份保存，静态 clip 层渲染时本来就会合并相邻矩形。
    // 两段 track 合起来仍然覆盖整轮 burst 的新字范围，不会漏遮。
    assert_eq!(
        state.new_ranges(),
        vec![(1, 2), (2, 3)],
        "连续吐字必须把整轮 burst 的新字都留在遮罩里；相邻 track 保持各自身份"
    );
    assert!(
        state
            .reveal_tracks
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
        &base_to_current,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        ConcealDirection::Forward,
        instant_at(now, 80),
    );
    assert_eq!(
        state.old_ranges(),
        vec![(3, 4), (4, 5)],
        "第二次删除必须映射回 base 坐标；相邻 track 按编辑身份分开保存"
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
        &base_to_current,
        &prev_target_to_new,
        ConcealDirection::Backward,
        half,
    );
    assert_eq!(
        state.old_ranges(),
        vec![(3, 4), (4, 5)],
        "Replace 的旧侧必须累计；相邻 track 保持各自身份"
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
        state.reveal_tracks.len(),
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
            .reveal_tracks
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
        ConcealDirection::Backward,
        now,
        160,
    );
    assert_eq!(state.conceal_tracks.len(), 1);
    assert!(
        (state.conceal_tracks[0].path.segments[0].y - 20.0).abs() < 1e-9,
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
        &base_to_current,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        ConcealDirection::Backward,
        half,
    );
    // Issue #826 评论 9 阻塞 3：新增的 C 与已吞掉的 DEF **不相邻**（DEF 已删，
    // C 在它左边），所以是两条独立 track。第一条 track 的路径不受影响 ——
    // 这正是「按编辑身份保存」要保证的：扩到上一行不会重绑已走过的那条。
    assert_eq!(
        state.old_ranges(),
        vec![(3, 6), (2, 3)],
        "扩到上一行是新增 track，已吞的那段保持自己的 range"
    );
    let paths: Vec<&FrontierPath> = state.conceal_tracks.iter().map(|t| &t.path).collect();
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0].segments.len(), 1);
    assert!(
        (paths[0].segments[0].y - 20.0).abs() < 1e-9,
        "第一段 track 仍是第二行，视觉逆序"
    );
    assert!(
        (paths[1].segments[0].y - 0.0).abs() < 1e-9,
        "新增 track 是第一行"
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
        ConcealDirection::Forward,
        now,
        160,
    );
    assert!(
        (state.conceal_tracks[0].path.segments[0].y - 0.0).abs() < 1e-9,
        "Forward 第一笔必须从第一行开始"
    );

    let half = instant_at(now, 80);
    // 第二笔：扩到下一行（base 坐标 [3,6)）。old range 变成 [0,6)。
    let base_to_current = OffsetMap::build("ABCDEF", "DEF");
    state.extend_delete(
        base.clone(),
        String::from(""),
        vec![(0, 3)],
        &base_to_current,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        ConcealDirection::Forward,
        half,
    );
    // 相邻（[0,3) 与 [3,6)）但来源不同 → 两条 track；第一条完全不受影响。
    assert_eq!(state.old_ranges(), vec![(0, 3), (3, 6)]);
    let paths: Vec<&FrontierPath> = state.conceal_tracks.iter().map(|t| &t.path).collect();
    assert_eq!(paths.len(), 2);
    assert!(
        (paths[0].segments[0].y - 0.0).abs() < 1e-9,
        "第一条 track 仍是第一行，已走过的部分不会被重绑"
    );
    assert!((paths[1].segments[0].y - 20.0).abs() < 1e-9);
}

/// Issue #826 评论 9 阻塞 3：多 patch 时 `travelled` 必须按 track 自己的身份继承，
/// 不能靠平行数组的下标。
///
/// 场景：先有一笔 Insert 在同一行开两个不相邻的 track `[10,12)` / `[100,102)`，
/// 两段都走完（`travelled == total_length`）。又来一笔 Insert 在正文**最前面**：
/// - 已有两段被 OffsetMap 映射成 `[12,14)` / `[102,104)`，`travelled` 跟着自己走，仍是走完；
/// - 新 patch `[0,2)` 开一条**新 track**，`travelled == 0`，**不能**继承任何旧进度。
///
/// 旧实现按下标搬数字：`normalize_ranges` 排序后是 `[0,2)` / `[12,14)` / `[102,104)`，
/// 新 patch 落到下标 0 → 凭空拿到 `[10,12)` 的进度；而 `[102,104)` 落到下标 2 → 旧数组
/// 只有 2 个元素，补 0，已经吐完的字重新被遮住。
#[test]
fn extend_insert_keeps_travelled_with_its_own_track() {
    let now = Instant::now();
    // 单行，三个 cluster 分别落在 10..12 / 100..102（未改动的部分）与最前面。
    let wide = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![
            cluster(0, 2, 0.0),
            cluster(2, 10, 100.0),
            cluster(10, 12, 200.0),
            cluster(12, 100, 300.0),
            cluster(100, 102, 400.0),
            cluster(102, 110, 500.0),
        ],
    )]);

    let mut state = EditFrontierState::begin_insert(
        String::new(),
        wide.clone(),
        String::new(),
        vec![(10, 12), (100, 102)],
        OffsetMap::from_single_edit(0, (0, 0), 0),
        now,
        160,
    );
    assert_eq!(state.reveal_tracks.len(), 2);
    assert!(state
        .reveal_tracks
        .iter()
        .all(|track| track.travelled == 0.0));
    // 路径必须真的非空，否则下面的 travelled 断言没有意义。
    assert!(
        state
            .reveal_tracks
            .iter()
            .all(|track| track.path.total_length > 0.0),
        "每条 track 的视觉路径长度必须为正"
    );

    // 模拟两段都已经走完。
    for track in &mut state.reveal_tracks {
        track.travelled = track.path.total_length;
    }

    // 又来一笔 Insert 在正文最前面（`[0,2)`），在动画未结束的半程 extend。
    let half = instant_at(now, 80);
    // old 正文长 110 byte，在最前面插入 2 byte：后续 range 整体右移 2。
    let prev_target_to_new = OffsetMap::from_single_edit(110, (0, 0), 2);
    state.extend_insert(
        wide.clone(),
        String::new(),
        vec![(0, 2)],
        &prev_target_to_new,
        &OffsetMap::from_single_edit(0, (0, 0), 0),
        half,
    );

    let ranges: Vec<(usize, usize)> = state.reveal_tracks.iter().map(|t| t.range).collect();
    assert_eq!(
        state.reveal_tracks.len(),
        3,
        "新增 patch 必须开一条独立 track，而不是把旧 track 拆开；实际 ranges = {ranges:?}"
    );
    let travelled_of = |range: (usize, usize)| -> f64 {
        state
            .reveal_tracks
            .iter()
            .find(|track| track.range == range)
            .map(|track| track.travelled)
            .expect("track 必须存在")
    };
    assert!(
        (travelled_of((0, 2)) - 0.0).abs() < 1e-9,
        "新 patch 必须从 0 开始，不能继承前面任何 track 的进度"
    );
    assert!(
        (travelled_of((12, 14))
            - state
                .reveal_tracks
                .iter()
                .find(|track| track.range == (12, 14))
                .expect("track 必须存在")
                .path
                .total_length)
            .abs()
            < 1e-9,
        "旧 track 的 travelled 必须跟着自己走完，不因新 patch 插到前面而回退"
    );
    assert!(
        (travelled_of((102, 104))
            - state
                .reveal_tracks
                .iter()
                .find(|track| track.range == (102, 104))
                .expect("track 必须存在")
                .path
                .total_length)
            .abs()
            < 1e-9,
        "旧 track 的 travelled 必须跟着自己走完，不因数组下标移位而回退"
    );
}

/// Issue #826 评论 10 阻塞 1：连续 Delete 必须沿 Core 的精确字符身份映回
/// burst base，不能每笔拿两份全文重新 `OffsetMap::build`。
///
/// 反例（评论原文）：
/// ```text
/// burst base = aXbXc
/// 第一笔（多 patch Delete）：aXbXc -> abc
/// 第二笔（继续 Delete b）：abc -> ac，deleted range 是当前 old 坐标 [1,2]
/// ```
/// 第二笔如果用 `OffsetMap::build("aXbXc", "abc")` 映回 base，只有最长公共
/// 前缀 a + 后缀 c，中间 b 没映射 → `map_new_range_to_old(1,2)` 返回 `None`
/// → `b` 被 filter 掉、ConcealTrack 没建出来 → `b` 视觉上直接从 canonical 消失，
/// 没有吞字。
///
/// 现在前沿保存累计的 `base_to_target_map`，用 `compose` 沿 Core 的精确 map 累计。
#[test]
fn consecutive_delete_uses_composed_base_mapping() {
    let now = Instant::now();
    // base 正文 `aXbXc`，被删掉的两处 X 分别是 old [1,2) 与 old [3,4)。
    let base = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![
            cluster(0, 1, 0.0),
            cluster(1, 2, 10.0),
            cluster(2, 3, 20.0),
            cluster(3, 4, 30.0),
            cluster(4, 5, 40.0),
        ],
    )]);

    // 第一笔：多 patch delete，两处 X。Core 的精确 map 保留 a/b/c 三个 island。
    let first_map = OffsetMap::from_edits(5, &[(1, 2, 1, 1), (3, 4, 2, 2)]);
    let mut state = EditFrontierState::begin_delete(
        base.clone(),
        String::from("aXbXc"),
        snapshot(Vec::new()),
        String::from("abc"),
        vec![(1, 2), (3, 4)],
        first_map.clone(),
        ConcealDirection::Forward,
        now,
        160,
    );
    assert_eq!(state.old_ranges(), vec![(1, 2), (3, 4)]);

    // 第二笔：删掉 `abc` 里的 b，本次 deleted range 是 `abc` 坐标 [1,2)。
    let second_map = OffsetMap::from_single_edit(3, (1, 2), 0);
    let base_to_current = state.base_to_target_map.clone();
    assert_eq!(
        base_to_current.map_new_range_to_old(1, 2),
        Some((2, 3)),
        "累计映射必须能把 abc 的 b 映回 aXbXc 的 b"
    );

    state.extend_delete(
        snapshot(Vec::new()),
        String::from("ac"),
        vec![(1, 2)],
        &base_to_current,
        &second_map,
        ConcealDirection::Forward,
        instant_at(now, 80),
    );

    // track 的顺序是**创建顺序**而不是排序 —— 动画状态按编辑身份保存，
    // 渲染阶段才合并几何。前两条是第一笔的两处 X，第三条是第二笔新建的 b。
    assert_eq!(
        state.old_ranges(),
        vec![(1, 2), (3, 4), (2, 3)],
        "b 必须建出 ConcealTrack（映回 base 坐标是 [2,3)），不能被 filter 掉"
    );
    assert_eq!(state.base_text, "aXbXc", "burst base 必须保持不变");
    // compose 之后再问一次：base 的 b 现在已经被删掉，不该再有映射。
    assert_eq!(
        state.base_to_target_map.map_old_to_new(2),
        None,
        "compose 之后 base 的 b（已被第二笔真正删除）不应再有映射"
    );
}

/// Issue #826 评论 10 阻塞 3：track 层归一化**只合并真正 overlap**，
/// 相邻 range 必须保持两个 owner。
///
/// 危害：Undo 一个 delete-surrounding 会一次恢复光标两侧的相邻文字，
/// 两条 final-new patch `[0,1]` / `[1,2]` 本该是两条 RevealTrack；
/// 合成成 `[0,2]` 后，动画未结束立刻在 byte 1 继续输入时，
/// `map_old_range_to_new(0, 2)` 跨过本次插入点返回 `None`，
/// 整条旧 track 被丢弃，上一轮还没吐完的恢复文字瞬间回 canonical。
#[test]
fn adjacent_insert_ranges_stay_separate_tracks() {
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
        vec![(0, 1), (1, 2)],
        "相邻 range 必须保持两个 owner，不能合成 [0,2)"
    );
    assert_eq!(
        state.reveal_tracks.len(),
        2,
        "相邻 patch 必须是两条独立 RevealTrack"
    );
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
