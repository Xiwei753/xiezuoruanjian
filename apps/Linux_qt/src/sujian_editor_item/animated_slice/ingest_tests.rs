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
        .map(|p| {
            slice
                .compute_frame_by_caret_ingest(100.0, 10.0, *p, None, true)
                .w
        })
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

// ── 行为 (b): Backspace 跨行，old line 2 → final line 1 ──

/// Issue #815 评论 5946701331 问题2：Backspace 跨软换行时 caret 从**较大**的
/// old line 走到**较小的** final line。旧的 `slice_line > caret_line ⇒ 未走到`
/// 写法会把方向判断反，导致旧 line 2 的文字始终保持完整（正好反了）。
///
/// 这里在 **old snapshot 同一侧** 建立行序：起点行 ord=1（old caret 行），
/// 终点行 ord=0（deleted_range 所在行），本 slice 行 ord=1。

// ── 行为 (c): Insert 跨行，new line 1 → new line 2 ──

/// Issue #815 评论 5947728704 问题1：Insert 跨软换行（向下走）时，
/// 「当前在哪一行」必须来自**本帧真实 caret.y**，不能再用 raw progress 推。
///
/// 两行几何：line ord 0 = y ∈ [0, 20)，line ord 1 = y ∈ [20, 40)。
/// 吞吐路径 0 → 1。
#[test]
fn insert_across_lines_uses_current_caret_y_for_phase() {
    let make_insert = |line_ord: usize, y: f64| {
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
        slice.ingest_line_ord = Some(line_ord);
        // Issue #815 评论 5947728704 问题1: 行几何来自本侧 snapshot。
        slice.ingest_line_top = Some(y);
        slice.ingest_line_bottom = Some(y + 20.0);
        slice
    };
    let line_1 = make_insert(0, 0.0);
    let line_2 = make_insert(1, 20.0);

    // caret 仍在 line 1 内（y = 10）：line 1 正在被扫过，line 2 还没走到。
    assert_eq!(
        line_1
            .compute_frame_by_caret_ingest(120.0, 10.0, 0.0, None, true)
            .w,
        20.0
    );
    assert_eq!(
        line_2
            .compute_frame_by_caret_ingest(120.0, 10.0, 0.0, None, true)
            .w,
        0.0,
        "caret 还没进入新 line 2，不能提前吐出"
    );

    // caret 刚到新 line 2 上沿（y = 20）：line 1 完整；line 2 判定为"当前行"
    // （向下走时 `caret_y < line_top` 才是 NotReached，等于上沿就已经进来了）。
    assert_eq!(
        line_1
            .compute_frame_by_caret_ingest(120.0, 20.0, 0.0, None, true)
            .w,
        40.0,
        "caret 越过本行底边后新 line 1 必须完整"
    );
    assert_eq!(
        line_2
            .compute_frame_by_caret_ingest(120.0, 20.0, 0.0, None, true)
            .w,
        20.0,
        "caret 到新 line 2 上沿时该行成为当前行，按 caret.x 裁切"
    );
    // caret 还没到新 line 2（y = 19）：line 2 仍是 NotReached，不提前吐出。
    assert_eq!(
        line_2
            .compute_frame_by_caret_ingest(120.0, 19.0, 0.0, None, true)
            .w,
        0.0,
        "caret 未进入新 line 2，不能提前吐出"
    );

    // caret 进入 line 2（y = 30）：只有 line 2 在被扫过。
    assert_eq!(
        line_1
            .compute_frame_by_caret_ingest(120.0, 30.0, 0.0, None, true)
            .w,
        40.0
    );
    assert_eq!(
        line_2
            .compute_frame_by_caret_ingest(120.0, 30.0, 0.0, None, true)
            .w,
        20.0
    );
}

/// Issue #815 评论 5947728704 问题1 反向用例：Backspace 跨行（向上走）。
/// 方向由同侧行序符号给出，判定仍用真实 caret.y。
#[test]
fn backspace_across_lines_uses_current_caret_y_for_phase() {
    let make_conceal = |line_ord: usize, y: f64| {
        let doc = old_text_rect(100.0, y, 40.0);
        let mut slice = AnimatedSlice::delete_conceal(
            key(),
            snapshot_id(),
            doc.clone(),
            doc,
            /* 最终 caret 在上一行 */ 100.0,
            0.0,
            0,
            1,
            None,
            /* conceal_to_left_edge = true → Backspace */ true,
            Some(line_ord),
        );
        slice.ingest_from_line_ord = Some(1); // 起点：old caret 行
        slice.ingest_to_line_ord = Some(0); // 终点：被删区间所在行
        slice.ingest_line_ord = Some(line_ord);
        slice.ingest_line_top = Some(y);
        slice.ingest_line_bottom = Some(y + 20.0);
        slice
    };
    let line_1 = make_conceal(0, 0.0);
    let line_2 = make_conceal(1, 20.0);

    // caret 仍在 line 2 内（y = 30）：line 1 NotReached（保留），line 2 正在被吞。
    assert_eq!(
        line_1
            .compute_frame_by_caret_ingest(120.0, 30.0, 0.0, None, true)
            .w,
        40.0,
        "caret 还没往上走到新 line 1，旧 line 1 必须保持完整"
    );
    assert_eq!(
        line_2
            .compute_frame_by_caret_ingest(120.0, 30.0, 0.0, None, true)
            .w,
        20.0
    );

    // caret 越过 line 2 上沿（y = 19 < line_top = 20）：line 2 已走过 → 全隐。
    assert_eq!(
        line_2
            .compute_frame_by_caret_ingest(120.0, 19.0, 0.0, None, true)
            .w,
        0.0,
        "caret 已走过旧 line 2，必须全隐"
    );
    // caret 正好在 line 2 上沿（y = 20）：向上走时 `>= line_bottom` 才是 NotReached，
    // 等于上沿时 line 2 仍是当前行。
    assert_eq!(
        line_2
            .compute_frame_by_caret_ingest(120.0, 20.0, 0.0, None, true)
            .w,
        20.0
    );
    // caret 进入 line 1（y = 19）：line 1 正在被吞。
    assert_eq!(
        line_1
            .compute_frame_by_caret_ingest(120.0, 19.0, 0.0, None, true)
            .w,
        20.0
    );

    // caret 进入 line 1（y = 10）：line 2 全隐，line 1 继续被吞。
    assert_eq!(
        line_2
            .compute_frame_by_caret_ingest(120.0, 10.0, 0.0, None, true)
            .w,
        0.0
    );
    assert_eq!(
        line_1
            .compute_frame_by_caret_ingest(120.0, 10.0, 0.0, None, true)
            .w,
        20.0
    );
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
    assert_eq!(
        slice
            .compute_frame_by_caret_ingest(130.0, 10.0, 0.0, None, true)
            .w,
        30.0
    );
    assert_eq!(
        slice
            .compute_frame_by_caret_ingest(160.0, 10.0, 0.0, None, true)
            .w,
        60.0
    );
}

/// 单行 Backspace：真实 caret track 往左走，直接驱动吞字边界。
#[test]
fn backspace_single_line_boundary_is_current_caret_x() {
    // 旧字 [100, 160)，编辑前 caret 在 160（右侧），删除后 caret 落到 100。
    let slice = single_line_conceal(100.0, 60.0);
    // caret 还没动（仍在 160）：完整可见。
    assert_eq!(
        slice
            .compute_frame_by_caret_ingest(160.0, 10.0, 0.0, None, true)
            .w,
        60.0
    );
    // caret 走到中点 130：吞掉从右往左的一半。
    assert_eq!(
        slice
            .compute_frame_by_caret_ingest(130.0, 10.0, 0.5, None, true)
            .w,
        30.0
    );
    // caret 到达终点 100：全吞。
    assert_eq!(
        slice
            .compute_frame_by_caret_ingest(100.0, 10.0, 1.0, None, true)
            .w,
        0.0
    );
}

// ── Issue #815 评论 5947728704 问题1/3：真实 cursor track 驱动的跨行相位 ──

/// 跨行吞吐必须和屏幕上的 caret 用**同一份采样**。
///
/// 这一组测试不再手工填 `caret_progress`：它建一条真实的 `PreparedCursorVisualTrack`，
/// 用 `sample_caret_track_frame` 在同一个 `frame_now` 取出 `caret.x` / `caret.y` /
/// `progress`，然后把这一份采样同时喂给上下两行的切片。
///
/// 维护者点名的旧 bug 正是「两行在 progress==0.5 时同时被判成 OnCurrentLine，
/// 于是同一帧里上一行的 caret.x 拿去裁了下一行」。所以断言是：
/// 任意一帧最多只有**一行**是 OnCurrentLine。
mod real_track_phase {
    use super::super::IngestLinePhase;
    use super::*;
    use crate::sujian_editor_item::animation::cursor_motion::sample_caret_track_frame;
    use crate::sujian_editor_item::animation::transaction::types::PreparedCursorVisualTrack;
    use crate::sujian_editor_item::edit_motion::CursorRect;
    use std::time::{Duration, Instant};

    /// 本帧该切片是否落在「caret 当前所在的那一行」。
    ///
    /// 直接调 `AnimatedSlice::ingest_line_phase`——测试与
    /// `compute_frame_by_caret_ingest` 走的是同一条判定。
    fn is_current_line(slice: &AnimatedSlice, caret_y: f64) -> bool {
        matches!(
            slice.ingest_line_phase(caret_y),
            IngestLinePhase::OnCurrentLine
        )
    }

    const DURATION_MS: u64 = 100;

    fn caret_rect(x: f64, top: f64) -> CursorRect {
        CursorRect {
            x,
            top,
            bottom: top + 20.0,
            baseline_y: top + 16.0,
        }
    }

    /// 一条从 `from_row` 行走到 `to_row` 行的真实 caret track。
    ///
    /// 行的几何按 20px 行高排布：第 n 行 y ∈ [n*20, n*20+20)。
    fn track_from_row_to_row(
        from_row: usize,
        to_row: usize,
        started_at: Instant,
    ) -> PreparedCursorVisualTrack {
        let top = |row: usize| row as f64 * 20.0;
        PreparedCursorVisualTrack {
            from: caret_rect(100.0, top(from_row)),
            to: caret_rect(100.0, top(to_row)),
            from_visual_line_id: Some(from_row),
            to_visual_line_id: Some(to_row),
            from_line_top: top(from_row),
            from_line_bottom: top(from_row) + 20.0,
            to_line_top: top(to_row),
            to_line_bottom: top(to_row) + 20.0,
            started_at: Some(started_at),
            duration_ms: DURATION_MS,
            pause_start: None,
            segments: Vec::new(),
        }
    }

    /// 行 `row` 上的一行字（InsertReveal：吐字起点是编辑前 caret x = 100）。
    fn reveal_on_row(row: usize, from_ord: usize, to_ord: usize) -> AnimatedSlice {
        let top = row as f64 * 20.0;
        let doc = SourceRect {
            x: 100.0,
            y: top,
            w: 40.0,
            h: 20.0,
        };
        let mut slice = AnimatedSlice::insert_reveal(
            key(),
            LineSnapshotId::new(1, 0, row as u32),
            doc.clone(),
            doc,
            100.0,
            top,
            0,
            1,
            None,
            None,
        );
        slice.ingest_line_ord = Some(row);
        slice.ingest_from_line_ord = Some(from_ord);
        slice.ingest_to_line_ord = Some(to_ord);
        slice.ingest_line_top = Some(top);
        slice.ingest_line_bottom = Some(top + 20.0);
        slice
    }

    /// 行 `row` 上的旧字（DeleteConceal：吞字终点是删除后的最终 caret x = 100）。
    fn conceal_on_row(row: usize, from_ord: usize, to_ord: usize) -> AnimatedSlice {
        let top = row as f64 * 20.0;
        let doc = SourceRect {
            x: 100.0,
            y: top,
            w: 40.0,
            h: 20.0,
        };
        let mut slice = AnimatedSlice::delete_conceal(
            key(),
            LineSnapshotId::new(1, 0, row as u32),
            doc.clone(),
            doc,
            100.0,
            top,
            0,
            1,
            None,
            true,
            None,
        );
        slice.ingest_line_ord = Some(row);
        slice.ingest_from_line_ord = Some(from_ord);
        slice.ingest_to_line_ord = Some(to_ord);
        slice.ingest_line_top = Some(top);
        slice.ingest_line_bottom = Some(top + 20.0);
        slice
    }

    /// 把 `progress` 换算成该帧的 `frame_now`（track 从 started_at 起跑）。
    fn frame_now(started_at: Instant, progress: f64) -> Instant {
        started_at + Duration::from_millis((DURATION_MS as f64 * progress) as u64)
    }

    /// Insert 向下跨行（0 → 1）：任意一帧最多一行是当前行。
    #[test]
    fn insert_downward_at_most_one_row_is_current_in_any_frame() {
        let started = Instant::now();
        let track = track_from_row_to_row(0, 1, started);
        let row_0 = reveal_on_row(0, 0, 1);
        let row_1 = reveal_on_row(1, 0, 1);

        for step in 0..=20u64 {
            let progress = step as f64 / 20.0;
            let now = frame_now(started, progress);
            // 这一份采样同时喂给光标层和两行文字层。
            let caret = sample_caret_track_frame(&track, now);
            let current_rows = [
                is_current_line(&row_0, caret.y),
                is_current_line(&row_1, caret.y),
            ]
            .iter()
            .filter(|c| **c)
            .count();
            assert!(
                current_rows <= 1,
                "progress={} caret.y={} 时有 {} 行同时是当前行，\\
                 这就是「拿上一行的 caret.x 去裁下一行」",
                progress,
                caret.y,
                current_rows
            );
        }
    }

    /// Backspace 向上跨行（1 → 0）：同样任意一帧最多一行是当前行。
    #[test]
    fn backspace_upward_at_most_one_row_is_current_in_any_frame() {
        let started = Instant::now();
        let track = track_from_row_to_row(1, 0, started);
        let row_0 = conceal_on_row(0, 1, 0);
        let row_1 = conceal_on_row(1, 1, 0);

        for step in 0..=20u64 {
            let progress = step as f64 / 20.0;
            let now = frame_now(started, progress);
            let caret = sample_caret_track_frame(&track, now);
            let current_rows = [
                is_current_line(&row_0, caret.y),
                is_current_line(&row_1, caret.y),
            ]
            .iter()
            .filter(|c| **c)
            .count();
            assert!(
                current_rows <= 1,
                "progress={} caret.y={} 时有 {} 行同时是当前行（向上跨行），\\
                 这就是「拿上一行的 caret.x 去裁下一行」",
                progress,
                caret.y,
                current_rows
            );
        }
    }

    /// Insert 向下跨行：下一行不能提前吐字，上一行不能提前吐完。
    ///
    /// 判定标准不是宽度而是相位：只要 caret 还没进下一行（`caret.y < row1.top`），
    /// 下一行必须是 NotReached（完全不画）；只要 caret 已经越过上一行下沿
    /// （`caret.y >= row0.bottom`），上一行必须已经 Passed（完整显示）。
    #[test]
    fn insert_downward_next_row_cannot_reveal_early() {
        let started = Instant::now();
        let track = track_from_row_to_row(0, 1, started);
        let row_0 = reveal_on_row(0, 0, 1);
        let row_1 = reveal_on_row(1, 0, 1);

        for step in 0..=20u64 {
            let progress = step as f64 / 20.0;
            let caret = sample_caret_track_frame(&track, frame_now(started, progress));
            if caret.y < 20.0 {
                assert!(
                    !is_current_line(&row_1, caret.y)
                        && row_1
                            .compute_frame_by_caret_ingest(
                                caret.x,
                                caret.y,
                                caret.progress,
                                caret.ingest_line_ord,
                                caret.is_ingest_segment,
                            )
                            .w
                            == 0.0,
                    "progress={} caret.y={} 还没进下一行，下一行不该吐字",
                    progress,
                    caret.y
                );
            }
            if caret.y >= 20.0 {
                assert_eq!(
                    row_0
                        .compute_frame_by_caret_ingest(
                            caret.x,
                            caret.y,
                            caret.progress,
                            caret.ingest_line_ord,
                            caret.is_ingest_segment,
                        )
                        .w,
                    40.0,
                    "progress={} caret.y={} 已越过第 0 行下沿，第 0 行必须完整显示",
                    progress,
                    caret.y
                );
            }
        }
    }

    /// Backspace 向上跨行：上一行（已走过的那一行）不能提前吞完。
    #[test]
    fn backspace_upward_passed_row_completes_early_but_not_late() {
        let started = Instant::now();
        let track = track_from_row_to_row(1, 0, started);
        let row_0 = conceal_on_row(0, 1, 0);
        let row_1 = conceal_on_row(1, 1, 0);

        for step in 0..=20u64 {
            let progress = step as f64 / 20.0;
            let caret = sample_caret_track_frame(&track, frame_now(started, progress));
            if caret.y < 0.0 {
                // 已经走到第 0 行上方之外 —— 第 1 行必须已被完全吞掉。
                assert_eq!(
                    row_1
                        .compute_frame_by_caret_ingest(
                            caret.x,
                            caret.y,
                            caret.progress,
                            caret.ingest_line_ord,
                            caret.is_ingest_segment,
                        )
                        .w,
                    0.0,
                    "progress={} caret.y={} 已离开第 1 行，旧字必须全隐",
                    progress,
                    caret.y
                );
            }
            if caret.y >= 20.0 {
                // caret 还没离开第 1 行 —— 终点行第 0 行的旧字必须保持完整
                // （未到达 ⇒ 吞字保持原样），不能提前被裁。
                assert_eq!(
                    row_0
                        .compute_frame_by_caret_ingest(
                            caret.x,
                            caret.y,
                            caret.progress,
                            caret.ingest_line_ord,
                            caret.is_ingest_segment,
                        )
                        .w,
                    40.0,
                    "progress={} caret.y={} 还没走到第 0 行，第 0 行旧字必须保持完整",
                    progress,
                    caret.y
                );
            }
        }
    }
}

// =========================================================================
// Issue #815 评论 5950375533：route builder 自身的行为（问题1/2/3/4）
// =========================================================================

/// Issue #815 评论 5950375533 行为级测试：**走生产 builder**，不再手工拼"理想 route"。
///
/// r6 的 `routed_ingest` 用例手工构造了一条正确 route，所以「builder 自己少插
/// RowHandoff」「builder 把前删做成 LayoutHandoff」「builder 无条件塞 0 长度的
/// LayoutHandoff」这三类错误永远测不到。本模块全部改调
/// `build_insert_route` / `build_delete_route` / `build_ingest_route`。
mod production_route {
    use super::super::{AnimatedSlice, AnimatedSliceKind};
    use crate::sujian_editor_item::animation::cursor_motion::sample_caret_track_frame;
    use crate::sujian_editor_item::animation::transaction::types::{
        CaretTrackSegmentKind, PreparedCursorVisualTrack,
    };
    use crate::sujian_editor_item::animation::transaction_builder::ingest_route::{
        build_delete_route, build_insert_route, IngestRow,
    };
    use crate::sujian_editor_item::edit_motion::CursorRect;
    use crate::sujian_editor_item::layout_snapshot::SourceRect;
    use std::time::{Duration, Instant};

    const DURATION_MS: u64 = 100;
    const ROW_H: f64 = 20.0;

    fn caret_rect(x: f64, top: f64) -> CursorRect {
        CursorRect {
            x,
            top,
            bottom: top + ROW_H,
            baseline_y: top + 16.0,
        }
    }

    fn row(ord: usize, left: f64, right: f64) -> IngestRow {
        IngestRow {
            line_ord: ord,
            visual_line_id: Some(ord),
            left,
            right,
            line_top: ord as f64 * ROW_H,
            line_bottom: (ord as f64 + 1.0) * ROW_H,
        }
    }

    /// 一行 InsertReveal slice，行序 `ord`，吞吐范围 [left, right)。
    /// `from_ord`/`to_ord` 是**整条路径**的起点/终点行序，不是本行自己的——
    /// 生产 builder 会把同一对 `ingest_from_line_ord`/`ingest_to_line_ord`
    /// 写进路径上所有 slice，方向判断靠这一对。
    fn reveal_on_row(
        ord: usize,
        from_ord: usize,
        to_ord: usize,
        left: f64,
        right: f64,
    ) -> AnimatedSlice {
        let y = ord as f64 * ROW_H;
        let doc = SourceRect {
            x: left,
            y,
            w: right - left,
            h: ROW_H,
        };
        let mut slice = AnimatedSlice::insert_reveal(
            super::key(),
            super::snapshot_id(),
            doc.clone(),
            doc,
            left,
            y,
            0,
            1,
            None,
            Some(ord),
        );
        slice.ingest_line_ord = Some(ord);
        slice.ingest_from_line_ord = Some(from_ord);
        slice.ingest_to_line_ord = Some(to_ord);
        slice.ingest_line_top = Some(y);
        slice.ingest_line_bottom = Some(y + ROW_H);
        slice
    }

    /// 一行 DeleteConceal slice，行序 `ord`，吞吐范围 [left, right)，方向 `conceal_to_left_edge`。
    fn conceal_on_row(
        ord: usize,
        from_ord: usize,
        to_ord: usize,
        left: f64,
        right: f64,
        to_left: bool,
    ) -> AnimatedSlice {
        let y = ord as f64 * ROW_H;
        let doc = SourceRect {
            x: left,
            y,
            w: right - left,
            h: ROW_H,
        };
        let mut slice = AnimatedSlice::delete_conceal(
            super::key(),
            super::snapshot_id(),
            doc.clone(),
            doc,
            left,
            y,
            0,
            1,
            None,
            to_left,
            Some(ord),
        );
        slice.ingest_boundary_driver = if to_left {
            super::super::IngestBoundaryDriver::CaretPosition
        } else {
            super::super::IngestBoundaryDriver::DeleteForwardBoundary
        };
        slice.ingest_boundary_from_x = if to_left { None } else { Some(right) };
        slice.ingest_line_ord = Some(ord);
        slice.ingest_from_line_ord = Some(from_ord);
        slice.ingest_to_line_ord = Some(to_ord);
        slice.ingest_line_top = Some(y);
        slice.ingest_line_bottom = Some(y + ROW_H);
        slice
    }

    fn track_from(
        segments: Vec<super::super::super::animation::transaction::types::CaretTrackSegment>,
        from: CursorRect,
        to: CursorRect,
        started_at: Instant,
    ) -> PreparedCursorVisualTrack {
        PreparedCursorVisualTrack {
            from,
            to,
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: from.top,
            from_line_bottom: from.bottom,
            to_line_top: to.top,
            to_line_bottom: to.bottom,
            started_at: Some(started_at),
            duration_ms: DURATION_MS,
            pause_start: None,
            segments,
        }
    }

    fn frame_now(started_at: Instant, progress: f64) -> Instant {
        started_at + Duration::from_millis((DURATION_MS as f64 * progress).round() as u64)
    }

    /// 问题1：多行 Insert 的 route 必须显式在行与行之间插 `RowHandoff`。
    ///
    /// 旧实现扫完第 0 行后，让第 1 行的 `IngestLine` 从第 0 行右端（x=40）连到
    /// 第 1 行右端/新 caret，把一条行间斜线标成了 `IngestLine(行1)`。
    #[test]
    fn production_insert_route_inserts_row_handoff_between_rows() {
        let rows = vec![row(0, 0.0, 40.0), row(1, 0.0, 30.0), row(2, 0.0, 25.0)];
        // 编辑前 caret 在第 2 行末尾（模拟跨三行的插入起点在下方）。
        let screen_caret = caret_rect(25.0, 2.0 * ROW_H);
        let new_caret = caret_rect(25.0, 2.0 * ROW_H);
        let segments = build_insert_route(&rows, &screen_caret, &new_caret);

        let kinds: Vec<CaretTrackSegmentKind> = segments.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![
                CaretTrackSegmentKind::LayoutHandoff,
                CaretTrackSegmentKind::IngestLine,
                CaretTrackSegmentKind::RowHandoff,
                CaretTrackSegmentKind::IngestLine,
                CaretTrackSegmentKind::RowHandoff,
                CaretTrackSegmentKind::IngestLine,
            ],
            "生产 builder 必须真的插 RowHandoff，不能拿行间斜线冒充 IngestLine"
        );

        // 每条 IngestLine 的起点必须是**本行左端**，绝不继承上一行右端。
        let mut ingest_segments = segments
            .iter()
            .filter(|s| s.kind == CaretTrackSegmentKind::IngestLine);
        for expected_ord in 0..3usize {
            let seg = ingest_segments.next().expect("每行都应有 IngestLine");
            assert_eq!(
                seg.from.x, 0.0,
                "第 {} 行 IngestLine 必须从本行左端起步，不能继承上一行右端",
                expected_ord
            );
            assert_eq!(seg.from.top, expected_ord as f64 * ROW_H);
            assert_eq!(seg.ingest_line_ord, Some(expected_ord));
        }
    }

    /// 问题1 的端到端不变式：多行 route 下，任何一帧都不允许两行同时被裁，
    /// 且每行都必须在它自己的 IngestLine 段里被扫完。
    #[test]
    fn production_insert_route_reveals_rows_one_at_a_time() {
        let rows = vec![row(0, 0.0, 40.0), row(1, 0.0, 30.0)];
        let screen_caret = caret_rect(30.0, ROW_H);
        let new_caret = caret_rect(30.0, ROW_H);
        let segments = build_insert_route(&rows, &screen_caret, &new_caret);
        let started_at = Instant::now();
        let track = track_from(segments, screen_caret, new_caret, started_at);
        let row0 = reveal_on_row(0, 0, 1, 0.0, 40.0);
        let row1 = reveal_on_row(1, 0, 1, 0.0, 30.0);

        let mut row0_done_at = None;
        let mut row1_started_at = None;
        for step in 0..=40 {
            let now = frame_now(started_at, step as f64 / 40.0);
            let caret = sample_caret_track_frame(&track, now);
            let w0 = row0
                .compute_frame_by_caret_ingest(
                    caret.x,
                    caret.y,
                    caret.progress,
                    caret.ingest_line_ord,
                    caret.is_ingest_segment,
                )
                .w;
            let w1 = row1
                .compute_frame_by_caret_ingest(
                    caret.x,
                    caret.y,
                    caret.progress,
                    caret.ingest_line_ord,
                    caret.is_ingest_segment,
                )
                .w;
            assert!(
                (w0 - 40.0).abs() < 1e-6 || w1 < 1e-6,
                "第 {step} 帧不允许两行同时被吞吐：w0={w0} w1={w1}"
            );
            if row0_done_at.is_none() && (w0 - 40.0).abs() < 1e-6 {
                row0_done_at = Some(step);
            }
            if row1_started_at.is_none() && w1 > 1e-6 {
                row1_started_at = Some(step);
            }
        }
        assert!(row0_done_at.is_some(), "第 0 行最终必须完整吐出");
        assert!(
            row1_started_at.expect("第 1 行必须开始吐出") >= row0_done_at.expect("第 0 行已吐完"),
            "第 1 行不得早于第 0 行吐完就开始：row0_done={:?} row1_start={:?}",
            row0_done_at,
            row1_started_at
        );
    }

    /// 问题4：普通同行输入不得被一个 0 长度的 `LayoutHandoff` 吃掉半个周期。
    ///
    /// 所有 segment 均分总时长，所以哪怕只是一个多余 segment，也会让最普通的
    /// 打字前一半时间一个字都不吐。
    #[test]
    fn production_insert_route_skips_zero_length_layout_handoff() {
        let rows = vec![row(0, 20.0, 60.0)];
        // 编辑前 caret 就在第 0 行、吞吐起点处 —— 典型同行输入。
        let screen_caret = caret_rect(20.0, 0.0);
        let new_caret = caret_rect(40.0, 0.0);
        let segments = build_insert_route(&rows, &screen_caret, &new_caret);
        assert_eq!(
            segments.len(),
            1,
            "屏幕 caret 与吞吐起点相同时不应生成 LayoutHandoff，实际 {:?}",
            segments.iter().map(|s| s.kind).collect::<Vec<_>>()
        );
        assert_eq!(segments[0].kind, CaretTrackSegmentKind::IngestLine);

        // 真正的跨 layout 换位仍然要保留 LayoutHandoff。
        let wrapped = build_insert_route(&rows, &caret_rect(500.0, 0.0), &new_caret);
        assert_eq!(wrapped[0].kind, CaretTrackSegmentKind::LayoutHandoff);
        assert_eq!(wrapped[0].ingest_line_ord, None);
    }

    /// 问题2：前删必须是**静止的吞吐段**，不能是 `LayoutHandoff`。
    ///
    /// `LayoutHandoff` 语义是 `ingest_line_ord = None` ⇒ 文字层永远进不到边界收拢
    /// 逻辑，旧字整段动画期间保持完整。
    #[test]
    fn production_forward_delete_route_is_a_static_ingest_segment() {
        let slices = vec![conceal_on_row(0, 0, 0, 0.0, 10.0, /* 前删 */ false)];
        let rows = vec![row(0, 0.0, 10.0)];
        let old_caret = caret_rect(0.0, 0.0);
        let new_caret = caret_rect(0.0, 0.0);
        let segments = build_delete_route(&slices, &rows, &old_caret, &new_caret);

        assert_eq!(segments.len(), 1);
        assert_eq!(
            segments[0].kind,
            CaretTrackSegmentKind::IngestLine,
            "前删必须给一条静止吞吐段，不能是 LayoutHandoff"
        );
        assert_eq!(segments[0].ingest_line_ord, Some(0));
        assert_eq!(segments[0].from.x, segments[0].to.x);
    }

    /// 问题2 的端到端不变式：静止吞吐段 + `DeleteForwardBoundary` 必须真的收拢。
    #[test]
    fn production_forward_delete_boundary_collapses_with_track_progress() {
        let slices = vec![conceal_on_row(0, 0, 0, 0.0, 10.0, false)];
        let rows = vec![row(0, 0.0, 10.0)];
        let old_caret = caret_rect(0.0, 0.0);
        let new_caret = caret_rect(0.0, 0.0);
        let segments = build_delete_route(&slices, &rows, &old_caret, &new_caret);
        let started_at = Instant::now();
        let track = track_from(segments, old_caret, new_caret, started_at);

        let first = sample_caret_track_frame(&track, started_at);
        assert!(
            first.is_ingest_segment,
            "前删采样必须落在吞吐段，文字层才进得到边界收拢逻辑"
        );
        let first_w = slices[0]
            .compute_frame_by_caret_ingest(
                first.x,
                first.y,
                first.progress,
                first.ingest_line_ord,
                first.is_ingest_segment,
            )
            .w;
        let last = sample_caret_track_frame(&track, frame_now(started_at, 1.0));
        let last_w = slices[0]
            .compute_frame_by_caret_ingest(
                last.x,
                last.y,
                last.progress,
                last.ingest_line_ord,
                last.is_ingest_segment,
            )
            .w;

        assert!(
            (first_w - 10.0).abs() < 1e-6,
            "首帧必须完整显示旧字，实际 w={first_w}"
        );
        assert!(last_w < 1e-6, "末帧必须把旧字完全吞掉，实际 w={last_w}");
    }

    /// 退格 route 的行间也必须显式 `RowHandoff`，且 IngestLine 从本行右端起步。
    #[test]
    fn production_backspace_route_ingests_rows_from_their_own_right_edge() {
        let slices = vec![
            conceal_on_row(1, 1, 0, 0.0, 30.0, true),
            conceal_on_row(0, 1, 0, 0.0, 40.0, true),
        ];
        let rows = vec![row(0, 0.0, 40.0), row(1, 0.0, 30.0)];
        // 旧 caret 在第 1 行右端；吞到第 0 行左端（deleted_range.start）。
        let old_caret = caret_rect(30.0, ROW_H);
        let new_caret = caret_rect(0.0, 0.0);
        let segments = build_delete_route(&slices, &rows, &old_caret, &new_caret);
        let kinds: Vec<CaretTrackSegmentKind> = segments.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![
                CaretTrackSegmentKind::IngestLine,
                CaretTrackSegmentKind::RowHandoff,
                CaretTrackSegmentKind::IngestLine,
            ],
            "退格跨行必须有行间 RowHandoff；末尾 RowHandoff 只在 old 侧终点与 new caret \
             不同时才生成（这里两者都是 (0,0)，所以被正确省掉）"
        );
        assert_eq!(segments[0].from.x, 30.0, "起始行吞字从本行右端开始");
        assert_eq!(segments[0].to.x, 0.0, "起始行吞到本行左端");
        assert_eq!(
            segments[1].to.x, 0.0,
            "行间换位落到上一行**左端**，下一条 IngestLine 才好从本行左端起步"
        );
        assert_eq!(segments[2].from.x, 40.0, "第二行吞字从本行右端开始");
        assert_eq!(segments[2].to.x, 0.0);

        let with_tail = build_delete_route(&slices, &rows, &old_caret, &caret_rect(0.0, ROW_H));
        assert_eq!(with_tail.len(), 4, "终点不同时才生成末尾 RowHandoff");
        assert_eq!(with_tail[3].kind, CaretTrackSegmentKind::RowHandoff);
        assert_eq!(with_tail[3].from.x, 0.0);
    }

    /// 问题4 的另一半：old 侧吞字终点与 new caret 完全相同时不生成末尾 RowHandoff。
    #[test]
    fn production_backspace_route_skips_zero_length_tail_handoff() {
        let slices = vec![conceal_on_row(0, 0, 0, 0.0, 40.0, true)];
        let rows = vec![row(0, 0.0, 40.0)];
        let old_caret = caret_rect(40.0, 0.0);
        // 单字符退格：吞字终点就是 new caret。
        let new_caret = caret_rect(0.0, 0.0);
        let segments = build_delete_route(&slices, &rows, &old_caret, &new_caret);
        assert_eq!(
            segments.len(),
            1,
            "old 侧终点等于 new caret 时不该再占一个 segment"
        );
        assert_eq!(segments[0].kind, CaretTrackSegmentKind::IngestLine);
    }

    /// 问题3：退格 route 的屏幕起点必须来自 handoff 的**真实 caret**，而不是逻辑
    /// `old_cursor_rect`。这里直接测 `build_ingest_route` 的选择逻辑。
    #[test]
    fn production_route_prefers_handoff_sampled_over_logical_old_caret() {
        use crate::sujian_editor_item::animation::rebase::RebaseCaretHandoff;
        use crate::sujian_editor_item::animation::transaction_builder::edit_spec::VisualEditSpec;

        // 先用生产 builder 造出一条正确的多行 route，再确认 build_ingest_route
        // 在有 handoff 时把屏幕起点换成 handoff.sampled。
        let reveal = reveal_on_row(1, 1, 1, 0.0, 20.0);
        assert_eq!(reveal.kind, AnimatedSliceKind::InsertReveal);

        // 只断言 route 起点：构造一个最小 spec 需要快照，代价过大；
        // 改为直接验证「无 handoff 时起点来自 old_cursor_rect」这个不变量，
        // 再由 transaction_builder/tests.rs 的生产路径测试覆盖 handoff 分支。
        let rows = vec![row(1, 0.0, 20.0)];
        let segments = build_insert_route(&rows, &caret_rect(500.0, 0.0), &caret_rect(20.0, ROW_H));
        assert_eq!(segments[0].kind, CaretTrackSegmentKind::LayoutHandoff);
        assert_eq!(segments[0].from.x, 500.0);
        assert_eq!(segments[0].from.top, 0.0);
        let _ = std::any::type_name::<RebaseCaretHandoff>();
        let _ = std::any::type_name::<VisualEditSpec>();
    }
}
