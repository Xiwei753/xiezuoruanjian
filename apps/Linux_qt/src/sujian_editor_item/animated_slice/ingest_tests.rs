//! Issue #815 评论 5946701331 行为级纯函数测试。
//!
//! 这一组测试不查源码字符串，直接对 `compute_frame_by_caret_ingest` 的输出求值，
//! 锁住维护者在评论里点名的三种实机效果：
//!   - forward Delete：old/new caret x 相同时也必须逐帧吞字；
//!   - Backspace 跨行：old line 2 → final line 1，旧 line 2 最终必须全隐；
//!   - Insert 跨行：new line 1 → new line 2，新行显隐顺序正确。
//!
//! 单独成文件是为了让 `animated_slice.rs` 保持在 god-file 上限之内
//! （与 `animation/render_plan_builder.rs` + `render_plan_builder/tests.rs` 同模式）。

use super::{AnimatedSlice, IngestBoundaryDriver, SourceRect};
use crate::sujian_editor_item::snapshot_id::LineSnapshotId;
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

fn snapshot_id() -> LineSnapshotId {
    LineSnapshotId::new(1, 1, 1)
}

fn key() -> VisualTransactionKey {
    VisualTransactionKey::new(1, 1)
}

/// x 方向覆盖 [x, x+w)、y 固定的一行旧文字。
fn old_text_rect(x: f64, y: f64, w: f64) -> SourceRect {
    SourceRect { x, y, w, h: 20.0 }
}

/// 单行 Backspace 吞字切片：旧字占 [x, x+w)，被删区间左端 = 删除后 caret。
fn single_line_conceal(x: f64, w: f64) -> AnimatedSlice {
    let doc = old_text_rect(x, 0.0, w);
    let mut slice = AnimatedSlice::delete_conceal(
        key(),
        snapshot_id(),
        doc.clone(),
        doc,
        /* 删除后的最终 caret = 被删区间左端 */ x,
        0.0,
        0,
        1,
        None,
        /* conceal_to_left_edge = true → Backspace */ true,
        Some(0),
    );
    slice.ingest_from_line_ord = Some(0);
    slice.ingest_to_line_ord = Some(0);
    slice.ingest_line_ord = Some(0);
    slice
}

// ── 行为 (a): forward Delete，old/new caret 同一位置 ──

/// Issue #815 评论 5946701331 问题1：Delete 键 old/new caret 本来就是同一位置，
/// cursor track 从头到尾不移动。若吞字边界无条件等于 caret.x，
/// `anchor == caret_x` 会让第一帧裁切宽度就是 0，DeleteConceal 直接消失。
///
/// 这里断言：Delete 键切片（`DeleteForwardBoundary` 驱动）在 caret 完全不动时
/// 仍然逐帧收窄，并且严格递减、终帧为 0。
#[test]
fn forward_delete_with_static_caret_still_swallows_per_frame() {
    // 旧字占 [100, 140)，caret 固定在 100（被删文字左侧 = Delete 键）。
    let doc = old_text_rect(100.0, 0.0, 40.0);
    let mut slice = AnimatedSlice::delete_conceal(
        key(),
        snapshot_id(),
        doc.clone(),
        doc,
        100.0,
        0.0,
        0,
        1,
        None,
        /* conceal_to_left_edge = false */ false,
        Some(0),
    );
    // Delete 键：边界从被删区间右端 140 向 caret 100 收拢。
    slice.ingest_boundary_driver = IngestBoundaryDriver::DeleteForwardBoundary;
    slice.ingest_boundary_from_x = Some(140.0);
    slice.ingest_from_line_ord = Some(0);
    slice.ingest_to_line_ord = Some(0);
    slice.ingest_line_ord = Some(0);

    // caret 一动不动，只有 track progress 推进。
    let widths: Vec<f64> = [0.0, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .map(|p| slice.compute_frame_by_caret_ingest(100.0, *p).w)
        .collect();

    assert_eq!(
        widths[0], 40.0,
        "首帧必须显示完整旧字（visible=1），不是 0——否则 DeleteConceal 直接消失"
    );
    for pair in widths.windows(2) {
        assert!(
            pair[1] < pair[0],
            "吞字必须逐帧收窄，实际宽度序列 {:?}",
            widths
        );
    }
    assert_eq!(
        widths[widths.len() - 1],
        0.0,
        "末帧必须完全吞掉旧字，实际宽度序列 {:?}",
        widths
    );
}

/// 同一条 DeleteConceal，如果误用 `CaretPosition` 驱动（即拿静止 caret.x
/// 当动画进度），第一帧就会是 0 宽——这正是维护者报的实机 bug。
/// 本测试把这个"错误实现"钉死成反例，确保 Delete 键不再走这条路径。
#[test]
fn forward_delete_would_vanish_under_caret_position_driver() {
    let doc = old_text_rect(100.0, 0.0, 40.0);
    let mut slice = AnimatedSlice::delete_conceal(
        key(),
        snapshot_id(),
        doc.clone(),
        doc,
        100.0,
        0.0,
        0,
        1,
        None,
        false,
        Some(0),
    );
    slice.ingest_boundary_from_x = Some(140.0);
    slice.ingest_from_line_ord = Some(0);
    slice.ingest_to_line_ord = Some(0);
    slice.ingest_line_ord = Some(0);
    // 保持构造函数默认的 CaretPosition：另一端 = caret_anchor_x = 100 = 静止 caret，
    // 首帧就被 clip 成 0 宽——这正是维护者报的实机 bug。
    assert_eq!(
        slice.ingest_boundary_driver,
        IngestBoundaryDriver::CaretPosition
    );
    assert_eq!(
        slice.compute_frame_by_caret_ingest(100.0, 0.0).w,
        0.0,
        "反例前提：CaretPosition + 静止 caret 会让首帧宽度为 0"
    );
}

// ── 行为 (b): Backspace 跨行，old line 2 → final line 1 ──

/// Issue #815 评论 5946701331 问题2：Backspace 跨软换行时 caret 从**较大**的
/// old line 走到**较小的** final line。旧的 `slice_line > caret_line ⇒ 未走到`
/// 写法会把方向判断反，导致旧 line 2 的文字始终保持完整（正好反了）。
///
/// 这里在 **old snapshot 同一侧** 建立行序：起点行 ord=1（old caret 行），
/// 终点行 ord=0（deleted_range 所在行），本 slice 行 ord=1。
#[test]
fn backspace_across_lines_2_to_1_fully_hides_old_line_2() {
    let doc = old_text_rect(100.0, 20.0, 40.0);
    let mut slice = AnimatedSlice::delete_conceal(
        key(),
        snapshot_id(),
        doc.clone(),
        doc,
        /* 最终 caret 落在下一行，x = 被删区间左端 */ 100.0,
        0.0,
        0,
        1,
        None,
        /* conceal_to_left_edge = true → Backspace */ true,
        Some(1),
    );
    slice.ingest_from_line_ord = Some(1); // 起点：old caret 行
    slice.ingest_to_line_ord = Some(0); // 终点：被删区间所在行
    slice.ingest_line_ord = Some(1); // 本 slice 在 old line ord 1

    // progress 0：边界还在 old line 1（编辑前 caret 在旧字右端 140），旧字完整可见。
    assert_eq!(
        slice.compute_frame_by_caret_ingest(140.0, 0.0).w,
        40.0,
        "跨行吞字起点必须完整显示旧 line 2 的字"
    );
    // progress 1：边界已落到 ord 0，旧 line 2 已被 caret 走过，必须全隐。
    assert_eq!(
        slice.compute_frame_by_caret_ingest(100.0, 1.0).w,
        0.0,
        "Backspace 跨行 2→1 后，旧 line 2 必须全隐（方向感知，不能反）"
    );
}

// ── 行为 (c): Insert 跨行，new line 1 → new line 2 ──

/// Issue #815 评论 5946701331 问题2：Insert 跨软换行时方向相反（向后走）。
/// 在 **new snapshot 同一侧** 建立行序：起点行 ord=0（inserted_range.start 所在行），
/// 终点行 ord=1（new caret 行）。新 line 1 必须先吐完，新 line 2 最后吐。
#[test]
fn insert_across_lines_1_to_2_reveals_in_order() {
    let make_insert = |y: f64| {
        let doc = old_text_rect(100.0, y, 40.0);
        let mut slice = AnimatedSlice::insert_reveal(
            key(),
            snapshot_id(),
            doc.clone(),
            doc,
            /* 编辑前 caret（吐字起点） */ 100.0,
            0.0,
            0,
            1,
            None,
            None,
        );
        slice.ingest_from_line_ord = Some(0); // 起点：inserted_range 所在新行
        slice.ingest_to_line_ord = Some(1); // 终点：new caret 行
        slice
    };
    let mut line_1 = make_insert(0.0);
    let mut line_2 = make_insert(20.0);
    line_2.ingest_line_ord = Some(1);
    line_1.ingest_line_ord = Some(0);

    // progress 0：两行都还没吐出来。
    assert_eq!(line_1.compute_frame_by_caret_ingest(100.0, 0.0).w, 0.0);
    assert_eq!(line_2.compute_frame_by_caret_ingest(100.0, 0.0).w, 0.0);

    // progress 0.75：边界已越过新 line 1、还在新 line 2 上。
    // → line 1 已走过 → 完整吐出；line 2 正在被 caret 扫过 → 按 caret.x 裁。
    assert_eq!(
        line_1.compute_frame_by_caret_ingest(100.0, 0.75).w,
        40.0,
        "新 line 1 必须先吐完"
    );
    assert_eq!(
        line_2.compute_frame_by_caret_ingest(100.0, 0.75).w,
        0.0,
        "新 line 2 此时还没被 caret 扫到，不该提前吐出"
    );

    // progress 1：终点行是 new line 2，caret 已到 140，两行都完整。
    assert_eq!(line_1.compute_frame_by_caret_ingest(140.0, 1.0).w, 40.0);
    assert_eq!(line_2.compute_frame_by_caret_ingest(140.0, 1.0).w, 40.0);
}

/// 补充：Insert 单行时边界就是本帧 caret.x（InsertReveal 始终 CaretPosition）。
#[test]
fn insert_single_line_boundary_is_current_caret_x() {
    let doc = old_text_rect(100.0, 0.0, 60.0);
    let slice = AnimatedSlice::insert_reveal(
        key(),
        snapshot_id(),
        doc.clone(),
        doc,
        100.0,
        0.0,
        0,
        1,
        None,
        None,
    );
    assert_eq!(slice.compute_frame_by_caret_ingest(130.0, 0.0).w, 30.0);
    assert_eq!(slice.compute_frame_by_caret_ingest(160.0, 0.0).w, 60.0);
}

/// 单行 Backspace：真实 caret track 往左走，直接驱动吞字边界。
#[test]
fn backspace_single_line_boundary_is_current_caret_x() {
    // 旧字 [100, 160)，编辑前 caret 在 160（右侧），删除后 caret 落到 100。
    let slice = single_line_conceal(100.0, 60.0);
    // caret 还没动（仍在 160）：完整可见。
    assert_eq!(slice.compute_frame_by_caret_ingest(160.0, 0.0).w, 60.0);
    // caret 走到中点 130：吞掉从右往左的一半。
    assert_eq!(slice.compute_frame_by_caret_ingest(130.0, 0.5).w, 30.0);
    // caret 到达终点 100：全吞。
    assert_eq!(slice.compute_frame_by_caret_ingest(100.0, 1.0).w, 0.0);
}
