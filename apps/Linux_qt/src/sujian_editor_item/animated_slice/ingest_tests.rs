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
// Issue #815 评论 5949097065：带真实路由段的 auto-wrap 吞吐（问题1/2/3）
// =========================================================================

/// 真实路由段的端到端行为测试。
///
/// r5 的 `real_track_phase` 用例把 `from.x = to.x = 100`，只证明了 y 相位修复，
/// 看不出「另一份 snapshot 的 x 混进来」这个危险。本模块用真正的 auto-wrap
/// 几何：编辑前 caret 在第 0 行末尾（x=500），打完字后新字落到第 1 行开头
/// （x∈[0,20)），新 caret 在 x=20。
///
/// 关键不变式：**整段动画里不允许出现「某一帧宽度恒为 0」**——那正是维护者报的
/// 「打一个字自动换行，从头到尾一个字符都不显示」。
mod routed_ingest {
    use super::super::{AnimatedSlice, AnimatedSliceKind};
    use crate::sujian_editor_item::animation::cursor_motion::sample_caret_track_frame;
    use crate::sujian_editor_item::animation::transaction::types::PreparedCursorVisualTrack;
    use crate::sujian_editor_item::animation::transaction::types::{
        CaretTrackSegment, CaretTrackSegmentKind,
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

    fn segment(
        kind: CaretTrackSegmentKind,
        from: CursorRect,
        to: CursorRect,
        ord: Option<usize>,
    ) -> CaretTrackSegment {
        CaretTrackSegment {
            kind,
            from,
            to,
            ingest_line_ord: ord,
            visual_line_id: ord,
        }
    }

    /// 打一个字触发自动换行的真实路由：
    /// 屏幕 caret（第 0 行 x=500）先做一次 LayoutHandoff 换位到 new snapshot 的
    /// inserted_range.start（第 1 行 x=0），再沿第 1 行从 0 扫到新 caret x=20。
    fn auto_wrap_track(started_at: Instant) -> PreparedCursorVisualTrack {
        PreparedCursorVisualTrack {
            from: caret_rect(500.0, 0.0),
            to: caret_rect(20.0, ROW_H),
            from_visual_line_id: Some(0),
            to_visual_line_id: Some(1),
            from_line_top: 0.0,
            from_line_bottom: ROW_H,
            to_line_top: ROW_H,
            to_line_bottom: ROW_H * 2.0,
            started_at: Some(started_at),
            duration_ms: DURATION_MS,
            pause_start: None,
            segments: vec![
                // 跨 layout 几何换位：不冒充吞吐，ingest_line_ord = None。
                segment(
                    CaretTrackSegmentKind::LayoutHandoff,
                    caret_rect(500.0, 0.0),
                    caret_rect(0.0, ROW_H),
                    None,
                ),
                // 第 1 行真正的吞吐：从 inserted_range.start 扫到新 caret。
                segment(
                    CaretTrackSegmentKind::IngestLine,
                    caret_rect(0.0, ROW_H),
                    caret_rect(20.0, ROW_H),
                    Some(1),
                ),
            ],
        }
    }

    /// 新 snapshot 第 1 行上那个新字符：吐字区间 [0,20)，行序 1，路径 0 → 1。
    fn new_char_slice() -> AnimatedSlice {
        let mut slice = AnimatedSlice::insert_reveal(
            super::key(),
            super::snapshot_id(),
            SourceRect {
                x: 0.0,
                y: ROW_H,
                w: 20.0,
                h: ROW_H,
            },
            SourceRect {
                x: 0.0,
                y: ROW_H,
                w: 20.0,
                h: ROW_H,
            },
            /* 编辑前 caret（跨 snapshot，只作 fallback） */ 500.0,
            0.0,
            0,
            1,
            None,
            Some(1),
        );
        slice.ingest_line_ord = Some(1);
        slice.ingest_from_line_ord = Some(1);
        slice.ingest_to_line_ord = Some(1);
        slice.ingest_line_top = Some(ROW_H);
        slice.ingest_line_bottom = Some(ROW_H * 2.0);
        slice.line_mask_left = 0.0;
        slice.line_mask_right = 20.0;
        slice
    }

    fn frame_now(started_at: Instant, progress: f64) -> Instant {
        started_at + Duration::from_millis((DURATION_MS as f64 * progress) as u64)
    }

    /// 问题1：单字自动换行 —— 新字符必须真的被 caret 从 0 扫到 20，
    /// 且整段动画里不允许出现「某一帧恒为 0」。
    #[test]
    fn auto_wrap_single_char_is_revealed_by_the_new_side_caret() {
        let started_at = Instant::now();
        let track = auto_wrap_track(started_at);
        let slice = new_char_slice();
        assert_eq!(slice.kind, AnimatedSliceKind::InsertReveal);

        let mut widths = Vec::new();
        for step in 0..=20u64 {
            let now = frame_now(started_at, step as f64 / 20.0);
            let sampled = sample_caret_track_frame(&track, now);
            let frame = slice.compute_frame_by_caret_ingest(
                sampled.x,
                sampled.y,
                sampled.progress,
                sampled.ingest_line_ord,
                sampled.is_ingest_segment,
            );
            widths.push(frame.w);
        }

        // 换位段（ingest_line_ord = None）必须保持初始状态 = 全隐。
        assert_eq!(
            widths[0], 0.0,
            "LayoutHandoff 段不得冒充吞吐，新字符此刻必须仍然全隐"
        );
        // 换位段结束后进入吞吐段，宽度必须单调增长到满格 20。
        let tail = &widths[1..];
        assert!(
            tail.last().copied().unwrap_or(0.0) > 19.5,
            "吞吐段结束时新字符必须完整显示，实际末帧宽度 {:?}",
            tail.last()
        );
        for pair in tail.windows(2) {
            assert!(
                pair[1] >= pair[0] - 1e-9,
                "吞吐边界必须单调推进，实际宽度序列 {:?}",
                widths
            );
        }
        // 问题1 的核心：不允许出现「换位后一直 0 宽」——那正是自动换行一个字都不显示。
        assert!(
            tail.iter().any(|w| *w > 0.5),
            "换成 new-side 边界后必须至少有一帧显示新字符，实际宽度序列 {:?}",
            widths
        );
        // 另一个回归：不能用跨 snapshot 的 anchor（500）当另一端，否则恒 0 宽。
        assert!(
            widths.iter().any(|w| *w < 19.5),
            "换位段之前的帧应当 0 宽（未进入吞吐段），实际宽度序列 {:?}",
            widths
        );
    }

    /// 问题3：三行路由必须真的经过中间行，采样能返回中间行的 ord。
    #[test]
    fn three_row_route_passes_through_the_middle_row() {
        let started_at = Instant::now();
        let row_top = |row: usize| row as f64 * ROW_H;
        let track = PreparedCursorVisualTrack {
            from: caret_rect(500.0, row_top(0)),
            to: caret_rect(40.0, row_top(2)),
            from_visual_line_id: Some(0),
            to_visual_line_id: Some(2),
            from_line_top: row_top(0),
            from_line_bottom: row_top(0) + ROW_H,
            to_line_top: row_top(2),
            to_line_bottom: row_top(2) + ROW_H,
            started_at: Some(started_at),
            duration_ms: DURATION_MS,
            pause_start: None,
            segments: vec![
                segment(
                    CaretTrackSegmentKind::LayoutHandoff,
                    caret_rect(500.0, row_top(0)),
                    caret_rect(0.0, row_top(1)),
                    None,
                ),
                segment(
                    CaretTrackSegmentKind::IngestLine,
                    caret_rect(0.0, row_top(1)),
                    caret_rect(30.0, row_top(1)),
                    Some(1),
                ),
                segment(
                    CaretTrackSegmentKind::RowHandoff,
                    caret_rect(30.0, row_top(1)),
                    caret_rect(0.0, row_top(2)),
                    Some(1),
                ),
                segment(
                    CaretTrackSegmentKind::IngestLine,
                    caret_rect(0.0, row_top(2)),
                    caret_rect(40.0, row_top(2)),
                    Some(2),
                ),
            ],
        };

        // 采样必须能返回中间行 1，而不是只有 from(0) / to(2)。
        let mut seen = Vec::new();
        for step in 0..=40u64 {
            let sampled =
                sample_caret_track_frame(&track, frame_now(started_at, step as f64 / 40.0));
            if let Some(ord) = sampled.ingest_line_ord {
                if !seen.contains(&ord) {
                    seen.push(ord);
                }
            }
        }
        assert!(
            seen.contains(&1),
            "三行路由的采样必须真的经过中间行 1，实际出现过的行序 {:?}",
            seen
        );
        assert!(
            seen.contains(&2),
            "三行路由的采样必须到达末行 2，实际出现过的行序 {:?}",
            seen
        );
    }

    /// 问题2：Backspace 跨软换行 —— 吞字边界必须来自 old snapshot 那一行，
    /// 不是新 caret 的 x。
    #[test]
    fn backspace_cross_wrap_swallowed_by_the_old_side_row_boundary() {
        let mut slice = AnimatedSlice::delete_conceal(
            super::key(),
            super::snapshot_id(),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 40.0,
                h: ROW_H,
            },
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 40.0,
                h: ROW_H,
            },
            /* 删除后的最终 caret（old-side，落在本行左端） */ 0.0,
            0.0,
            0,
            1,
            None,
            /* Backspace */ true,
            Some(0),
        );
        slice.ingest_line_ord = Some(0);
        slice.ingest_from_line_ord = Some(1);
        slice.ingest_to_line_ord = Some(0);
        slice.ingest_line_top = Some(0.0);
        slice.ingest_line_bottom = Some(ROW_H);
        slice.line_mask_left = 0.0;
        slice.line_mask_right = 40.0;

        // Backspace：本行左侧的字被保留，caret 右边的被吞掉。
        let at_start = slice.compute_frame_by_caret_ingest(40.0, 10.0, 0.0, Some(0), true);
        let at_end = slice.compute_frame_by_caret_ingest(0.0, 10.0, 1.0, Some(0), true);
        assert!(
            at_start.w > 39.5,
            "caret 还在本行右端时旧字必须完整可见，实际 w={}",
            at_start.w
        );
        assert!(
            at_end.w.abs() < 0.5,
            "caret 扫到本行左端后旧字必须被吞完，实际 w={}",
            at_end.w
        );
    }

    /// 问题3 之二：快速连续输入的 rebase 必须保住当前路由段的真实行身份与几何。
    #[test]
    fn rebase_in_the_middle_row_keeps_that_rows_identity_and_geometry() {
        let started_at = Instant::now();
        let mut track = auto_wrap_track(started_at);
        // 动画进行到 0.6 时 rebase（两段路由 ⇒ 0.5 恰好落在换位段与吞吐段的
        // 交界、局部进度为 0，取 0.6 确保落在吞吐段内部）。
        let now = frame_now(started_at, 0.6);
        let sampled = sample_caret_track_frame(&track, now);
        assert_eq!(
            sampled.ingest_line_ord,
            Some(1),
            "rebase 前采样必须落在吞吐行 1，实际 {:?}",
            sampled.ingest_line_ord
        );
        let sampled_x = sampled.x;
        assert!(
            sampled_x > 0.0,
            "rebase 前的采样必须已经有真实几何，实际 x={}",
            sampled_x
        );

        track.rebase_to(caret_rect(20.0, ROW_H), now);

        // rebase 之后仍必须报告同一行身份，且从采样位置连到新目标，
        // 绝不退回「逻辑旧 caret」（500，第 0 行）。
        let after = sample_caret_track_frame(&track, now);
        assert_eq!(
            after.ingest_line_ord,
            Some(1),
            "rebase 后必须保住当前路由段的行身份 1，实际 {:?}",
            after.ingest_line_ord
        );
        assert!(
            (after.x - sampled_x).abs() < 1e-6,
            "rebase 后必须从本帧实际采样位置（{}）出发，不得跳回逻辑旧 caret，实际 x={}",
            sampled_x,
            after.x
        );
    }
}
