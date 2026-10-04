//! Issue #826 评论 2: Reflow 层单元测试。
//!
//! 覆盖：unchanged 匹配 / changed range 排除 / 位置插值 / 位置未变不产生 span。

use std::time::{Duration, Instant};

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::animation::reflow_motion::{ReflowSpan, ReflowState};
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, LineSnapshotId, PreparedLineSnapshot,
    ShapingIdentity, SourceRect,
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
        crate::editor::layout::LayoutSnapshot::empty_for_tests(),
        lines,
        None,
        None,
        crate::editor::layout::CaretAffinity::Downstream,
    )
}

#[test]
fn reflow_sample_interpolates_old_to_new_rect() {
    let now = Instant::now();
    let state = ReflowState {
        spans: vec![ReflowSpan {
            old_range: (0, 1),
            new_range: (0, 1),
            old_rect: SourceRect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 20.0,
            },
            new_rect: SourceRect {
                x: 100.0,
                y: 0.0,
                w: 10.0,
                h: 20.0,
            },
            snapshot_id: LineSnapshotId::new(0, 0, 0),
            source_rect: SourceRect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 20.0,
            },
        }],
        target_text: String::new(),
        started_at: now,
        duration_ms: 100,
    };
    let start = state.sample(now);
    assert!((start[0].dest_rect.x - 0.0).abs() < 1e-9);
    let end = state.sample(now + Duration::from_millis(100));
    assert!((end[0].dest_rect.x - 100.0).abs() < 1e-9);
}

#[test]
fn reflow_is_finished_after_duration() {
    let now = Instant::now();
    let state = ReflowState {
        spans: Vec::new(),
        target_text: String::new(),
        started_at: now,
        duration_ms: 160,
    };
    assert!(!state.is_finished(now));
    assert!(state.is_finished(now + Duration::from_millis(160)));
}

#[test]
fn reflow_excludes_changed_ranges() {
    // old "ab"，new "aXb"。插入的 "X" 落在 excluded_new (1..2)，必须排除；
    // 未改的 "a"/"b" 位置没变，也不该产生 span。
    let old = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
    )]);
    let new = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0), cluster(2, 3, 20.0)],
    )]);
    let offset_map = OffsetMap::from_single_edit(2, (1, 1), 1);
    let reflow = ReflowState::build(&old, &new, &offset_map, &[], &[(1, 2)], Instant::now(), 160);
    // old "ab" → new "aXb"（在位置 1 插入一个字符）。
    // - new "a"(0,1) 位置没变 → 无 span
    // - new "X"(1,2) 是 changed range → 必须被排除
    // - new "b"(2,3) 没改但右移了 → 必须产生 span
    assert_eq!(
        reflow.spans.len(),
        1,
        "只有未改且位置变化的 b 应产生 ReflowSpan，实际 {} 个",
        reflow.spans.len()
    );
    assert!(
        reflow
            .spans
            .iter()
            .all(|span| span.new_range == (2, 3) && span.old_range == (1, 2)),
        "span 必须只对应未改的 b，不能包含 changed range 的 X"
    );
}

#[test]
fn reflow_matches_unchanged_text_that_moved_to_another_line() {
    // old "ab" 单行；new "a\nb" 两行。"b" 未改（shaping 相同）但换行，应产生 span。
    let old = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
    )]);
    let new = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(1, 20.0, 2, vec![cluster(2, 3, 0.0)]),
    ]);
    // old "ab" → new "a\nb"：在 old 1 处插入 "\n"（1 byte）。
    let offset_map = OffsetMap::from_single_edit(2, (1, 1), 1);
    let reflow = ReflowState::build(
        &old,
        &new,
        &offset_map,
        &[],
        &[(1, 2)], // 插入的换行 new 坐标 1..2
        Instant::now(),
        160,
    );
    assert_eq!(reflow.spans.len(), 1, "未改但换行的 'b' 应产生 ReflowSpan");
    let span = &reflow.spans[0];
    assert_eq!(span.old_range, (1, 2));
    assert_eq!(span.new_range, (2, 3));
    // 旧位置在第一行 x=10，新位置在第二行 x=0 / y=20。
    assert!((span.old_rect.x - 10.0).abs() < 1e-9);
    assert!((span.new_rect.x - 0.0).abs() < 1e-9);
    assert!((span.new_rect.y - 20.0).abs() < 1e-9);
    // 取的是新行的纹理。
    assert_eq!(span.snapshot_id.visual_line_ordinal, 1);
}

#[test]
fn reflow_never_emits_span_inside_changed_range() {
    // new 坐标 1..2 是 changed（excluded_new），无论它几何是否变化都不该有 span。
    let old = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0)],
    )]);
    let new = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 999.0)],
    )]);
    let offset_map = OffsetMap::from_single_edit(1, (1, 1), 1);
    let reflow = ReflowState::build(&old, &new, &offset_map, &[], &[(1, 2)], Instant::now(), 160);
    assert!(reflow.is_empty(), "changed range 内的 glyph 不得进 Reflow");
}

/// Issue #826 评论 3 问题 3：连续输入时 Reflow 不能跳位置。
///
/// 之前每笔编辑都重新 `ReflowState::build(...)`，上一笔还在 A -> B 半路时
/// 新一笔会直接从 canonical 的 B 开始 B -> C，屏幕上先跳一下再动。
/// `retarget` 必须先 `sample(now)` 拿到当前屏幕真实位置当新起点。
#[test]
fn retarget_continues_from_current_screen_position() {
    let now = Instant::now();
    // 第一次：A(x=0) -> B(x=100)，时长 100ms。
    let old_snapshot = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0)],
    )]);
    // stub 的 doc x = cluster.x + visual_x(= cluster.x)，所以这里用 50 得到 doc x=100。
    let mid_snapshot = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 50.0)],
    )]);
    let mut state = ReflowState::build(
        &old_snapshot,
        &mid_snapshot,
        &OffsetMap::from_single_edit(1, (1, 1), 1),
        &[],
        &[(1, 2)],
        now,
        100,
    );
    assert_eq!(state.spans.len(), 1);
    state.set_target_text(String::from("Xa"));

    // 半程采样：屏幕上这个字应该在中间（不在 canonical 的 100）。
    let half = now + Duration::from_millis(50);
    let mid_x = state.sample(half)[0].dest_rect.x;
    assert!(mid_x > 1.0 && mid_x < 99.0, "半程应该在中间，实际 {mid_x}");

    // 第二次编辑：正文继续右移到 C(x=200)。
    // prev_target_to_new 把上一份 Reflow 的 new 坐标（"Xa" 坐标）映到最新
    // new 坐标（"XYa" 坐标）。
    let next_snapshot = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 100.0)],
    )]);
    let prev_target_to_new = OffsetMap::build("Xa", "XYa");
    // 本次编辑的 old_snapshot 是上一帧的 "Xa" 版（doc x=100），old_to_new 是
    // "Xa" -> "XYa"。
    let cur_old_snapshot = mid_snapshot.clone();
    let old_to_new = OffsetMap::build("Xa", "XYa");
    let retargeted = state.retarget(
        half,
        &cur_old_snapshot,
        &old_to_new,
        &next_snapshot,
        &prev_target_to_new,
        &[],
        &[(2, 3)],
        100,
    );

    assert_eq!(retargeted.spans.len(), 1, "retarget 后必须保留这一段");
    // 起点必须就是刚才屏幕上的位置，而不是 canonical 的 100。
    assert!(
        (retargeted.spans[0].old_rect.x - mid_x).abs() < 1e-9,
        "retarget 起点必须是当前屏幕位置 {}，实际 {}",
        mid_x,
        retargeted.spans[0].old_rect.x
    );
    // 目标是最新的 canonical。
    assert!(
        (retargeted.spans[0].new_rect.x - 200.0).abs() < 1e-9,
        "retarget 目标必须是最新 canonical 位置"
    );
    // retarget 后第一帧就停在屏幕位置，不会跳。
    let first = retargeted.sample(half);
    assert!(
        (first[0].dest_rect.x - mid_x).abs() < 1e-9,
        "retarget 后第一帧必须原地不动（不能跳到 canonical）"
    );
}

/// Issue #826 评论 4 问题 2：这次新进入 Reflow 的字不能瞬移。
///
/// 场景：第一笔输入行还没满，`b` 没动 -> 不在 previous.spans。
/// 第二笔刚好把行撑满，`b` 第一次掉到下一行。
/// 它在上一份 Reflow 里根本不存在，retarget 必须退到「本次 base_snapshot 的
/// old rect -> 最新 new rect」建立新 span，而不是 `continue` 跳过。
#[test]
fn retarget_creates_span_for_text_that_newly_enters_reflow() {
    let now = Instant::now();
    // 正文从 "ab" 变成 "aXYb"（在 byte 1 插入 XY），b 被挤到第二行。
    // 编辑前：b 在第一行末尾，doc x = 300。
    let old_snapshot = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 150.0)],
    )]);
    // 编辑后：b 在第二行行首，doc y = 20。
    let new_snapshot = snapshot(vec![
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        // "aXYb" 里 b 在 byte 3（byte 0=a, 1=X, 2=Y, 3=b）。
        PreparedLineSnapshot::stub_for_tests(1, 20.0, 0, vec![cluster(3, 4, 0.0)]),
    ]);
    // 上一份 Reflow：一个 span 都没有（b 上一笔没动）。
    let previous = ReflowState {
        spans: Vec::new(),
        started_at: now,
        duration_ms: 100,
        target_text: String::from("ab"),
    };
    let old_to_new = OffsetMap::build("ab", "aXYb");
    let prev_target_to_new = OffsetMap::build("ab", "aXYb");
    // excluded：插入的 XY 在 old 侧是零长度插入点 (1,1)，在 new 侧是 (1,3)。
    // b 的 (1,2) / (2,3) 不该被排除 —— 它正是这次要重排的字。
    let retargeted = previous.retarget(
        now,
        &old_snapshot,
        &old_to_new,
        &new_snapshot,
        &prev_target_to_new,
        &[(1, 1)],
        &[(1, 3)],
        100,
    );

    assert_eq!(
        retargeted.spans.len(),
        1,
        "本次新掉行的 b 必须建立 span，不能瞬移"
    );
    // 起点 = 本次 base_snapshot 里的 old rect（第一行末尾，doc y=0）。
    assert!(
        (retargeted.spans[0].old_rect.y - 0.0).abs() < 1e-9,
        "起点应取本次 base_snapshot 的 old rect，实际 y={}",
        retargeted.spans[0].old_rect.y
    );
    // 目标 = 最新 canonical（第二行，doc y=20）。
    assert!(
        (retargeted.spans[0].new_rect.y - 20.0).abs() < 1e-9,
        "目标应取最新 canonical 位置，实际 y={}",
        retargeted.spans[0].new_rect.y
    );
}

/// Issue #826 评论 4 问题 3：Reflow 接管期间要输出 canonical 目标位置的 exclusion clip。
#[test]
fn target_clip_rects_cover_canonical_destination() {
    let now = Instant::now();
    let old_snapshot = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0)],
    )]);
    let new_snapshot = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 50.0)],
    )]);
    let state = ReflowState::build(
        &old_snapshot,
        &new_snapshot,
        &OffsetMap::from_single_edit(1, (1, 1), 1),
        &[],
        &[(1, 2)],
        now,
        100,
    );
    let clips = state.target_clip_rects();
    assert_eq!(clips.len(), 1, "每个 active span 要有一条 exclusion clip");
    // clip 必须覆盖 canonical 的最终位置（doc x=100），不是起点。
    assert!(
        (clips[0].0 - 100.0).abs() < 1e-9,
        "exclusion clip 必须覆盖 canonical 目标位置，实际 x={}",
        clips[0].0
    );
    assert!((clips[0].1 - 0.0).abs() < 1e-9);
    assert!(clips[0].2 > 0.0 && clips[0].3 > 0.0);
}
