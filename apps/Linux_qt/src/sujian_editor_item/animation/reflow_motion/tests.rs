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
